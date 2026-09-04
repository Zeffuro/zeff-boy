use std::ops::ControlFlow;

use crate::emu_thread::{
    EmuCommand, EmuResponse, TasControlAcquireRejectedReason, TasControlCommitRejectedReason,
    TasControlLeaseWitness, TasControlRollbackRejectedReason, TasExecutionProfile,
    TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRejectedReason,
    TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasDigest};

use super::EmuLoop;

mod advance;
mod checkpoint;
pub(super) mod execution;
mod retirement;
mod state_cache;
pub(in crate::emu_thread) mod witness;

use advance::TasFrameAdvanceResult;
use checkpoint::{restore_backend_checkpoint, verify_tas_execution_candidate};
use execution::TasExecutionResult;
use state_cache::WorkerTasStateCache;

#[derive(Clone, Debug, PartialEq, Eq)]
enum BackendAuthority {
    Gameplay,
    Leased {
        lease_id: u64,
        profile: TasExecutionProfile,
        state_format_compatibility_id: &'static str,
        checkpoint: Box<TasControlCheckpoint>,
        attempted_run_id: Option<u64>,
        execution_failed: bool,
        candidate: Option<Box<TasExecutionResult>>,
        intermediate_cache_proofs: Vec<crate::emu_thread::TasExecutionCacheProof>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TasControlCheckpoint {
    pub(super) profile: TasExecutionProfile,
    pub(super) state_bytes: Vec<u8>,
    pub(super) state_sha256: TasDigest,
    pub(super) frame_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TasRestoredCheckpoint {
    pub(super) state_sha256: TasDigest,
    pub(super) frame_count: u64,
}

fn tas_state_digest(profile: TasExecutionProfile, state: &[u8]) -> TasDigest {
    if !matches!(
        profile,
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb
    ) {
        return TasDigest::from_bytes(state);
    }

    let mut canonical = state.to_vec();
    zeff_gb_core::save_state::canonicalize_bess_rtc_timestamp(&mut canonical);
    TasDigest::from_bytes(&canonical)
}

pub(super) struct TasControl {
    authority: BackendAuthority,
    next_lease_id: u64,
    replay_activity_unwitnessed: bool,
    state_cache: WorkerTasStateCache,
}

pub(super) struct TasControlContext {
    pub(super) uncapped_execution: bool,
    pub(super) audio_recording_active: bool,
    pub(super) link_activity: bool,
    pub(super) pending_frame_delivery: bool,
    pub(super) runtime_fault: bool,
}

impl EmuLoop {
    pub(super) fn tas_control_context(&self) -> TasControlContext {
        TasControlContext {
            uncapped_execution: self.uncapped_mode,
            audio_recording_active: self.audio_recording_capture.active,
            link_activity: self.pending_tcp_link.is_some()
                || self.tcp_link.is_some()
                || self.game_boy_replay_link.is_some()
                || self.wonder_swan_replay_link.is_some(),
            pending_frame_delivery: !self.drain_rx.is_empty(),
            runtime_fault: !self.runtime_fault.can_step(),
        }
    }

    pub(super) fn dispatch_tas_control(
        &mut self,
        command: EmuCommand,
    ) -> ControlFlow<bool, EmuCommand> {
        let context = self.tas_control_context();
        let cheats_present = !self.last_cheats.is_empty();
        let dispatch = match command {
            EmuCommand::InspectTasReadiness {
                request_id,
                profile,
            } => {
                let mut observation =
                    witness::observe_loaded_profile(&self.backend, cheats_present, profile);
                if let Some(identity) = self.tas_repair.identity
                    && identity.profile == profile
                    && witness::build_tas_witness_for_persistence(
                        &self.backend,
                        cheats_present,
                        profile,
                        identity.persistence,
                    )
                    .is_ok()
                {
                    observation.project_owned_persistence = Some(identity.persistence);
                }
                ControlFlow::Break(EmuResponse::TasReadinessObserved {
                    request_id,
                    observation: Box::new(observation),
                })
            }
            EmuCommand::RollbackTasControl { lease_id } => {
                let backend = &mut self.backend;
                let persistence = self.tas_repair.identity.map_or(
                    crate::emu_thread::TasPersistenceContract::Absent,
                    |identity| identity.persistence,
                );
                let response = self.tas_control.rollback(lease_id, |checkpoint| {
                    restore_backend_checkpoint(backend, checkpoint, persistence)
                });
                if matches!(response, EmuResponse::TasControlRolledBack { .. }) {
                    self.finalize_tas_loaded_observables("(TAS rollback)");
                }
                ControlFlow::Break(response)
            }
            EmuCommand::ExecuteTasControl(request) => {
                let request = *request;
                let cached_state = self.tas_control.cached_state(&request);
                let backend = &mut self.backend;
                let runtime_fault = &mut self.runtime_fault;
                let persistence = self.tas_repair.identity.map_or(
                    crate::emu_thread::TasPersistenceContract::Absent,
                    |identity| identity.persistence,
                );
                let response = self.tas_control.execute(request.clone(), || {
                    execution::execute_tas(
                        backend,
                        runtime_fault,
                        request,
                        cached_state,
                        persistence,
                    )
                });
                if matches!(response, EmuResponse::TasExecutionCompleted { .. }) {
                    if self.tas_control.candidate_is_cacheable()
                        && let Ok(state_bytes) = self.backend.encode_state_bytes()
                    {
                        self.tas_control.cache_candidate(state_bytes);
                    }
                    self.finalize_tas_loaded_observables("(TAS execution)");
                }
                ControlFlow::Break(response)
            }
            EmuCommand::AdvanceTasControl(request) => {
                let request = *request;
                let backend = &mut self.backend;
                let runtime_fault = &mut self.runtime_fault;
                let persistence = self.tas_repair.identity.map_or(
                    crate::emu_thread::TasPersistenceContract::Absent,
                    |identity| identity.persistence,
                );
                let response = self
                    .tas_control
                    .advance(request, |candidate, input, snapshot| {
                        advance::advance_tas_frame(
                            backend,
                            runtime_fault,
                            candidate,
                            input,
                            snapshot,
                            persistence,
                        )
                    });
                if matches!(response, EmuResponse::TasFrameAdvanced { .. }) {
                    if self.tas_control.candidate_is_cacheable()
                        && let Ok(state_bytes) = self.backend.encode_state_bytes()
                    {
                        self.tas_control.cache_candidate(state_bytes);
                    }
                    self.finalize_tas_loaded_observables("(TAS frame advance)");
                }
                ControlFlow::Break(response)
            }
            EmuCommand::CommitTasControl { lease_id } => {
                let backend = &self.backend;
                let persistence = self.tas_repair.identity.map_or(
                    crate::emu_thread::TasPersistenceContract::Absent,
                    |identity| identity.persistence,
                );
                ControlFlow::Break(self.tas_control.commit(lease_id, |candidate| {
                    verify_tas_execution_candidate(backend, candidate, persistence)
                }))
            }
            command => self.tas_control.dispatch(command, context, |profile| {
                if profile == TasExecutionProfile::DirectGbaCartridge {
                    self.backend.drain_audio_samples_into(&mut Vec::new());
                }
                if let Some(identity) = self.tas_repair.identity {
                    witness::build_tas_witness_for_persistence(
                        &self.backend,
                        cheats_present,
                        profile,
                        identity.persistence,
                    )
                } else {
                    witness::build_tas_witness(&self.backend, cheats_present, profile)
                }
            }),
        };
        match dispatch {
            ControlFlow::Continue(command) => ControlFlow::Continue(command),
            ControlFlow::Break(response) => ControlFlow::Break(self.send_resp(response)),
        }
    }
}

impl TasControl {
    pub(super) fn new() -> Self {
        Self {
            authority: BackendAuthority::Gameplay,
            next_lease_id: 1,
            replay_activity_unwitnessed: false,
            state_cache: WorkerTasStateCache::new(),
        }
    }

    pub(super) fn dispatch<F>(
        &mut self,
        command: EmuCommand,
        context: TasControlContext,
        build_witness: F,
    ) -> ControlFlow<EmuResponse, EmuCommand>
    where
        F: FnOnce(
            TasExecutionProfile,
        ) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason>,
    {
        match command {
            EmuCommand::AcquireTasControl {
                request_id,
                profile,
            } => ControlFlow::Break(self.acquire(request_id, profile, context, build_witness)),
            EmuCommand::RollbackTasControl { .. } | EmuCommand::CommitTasControl { .. } => {
                unreachable!("TAS finalization escaped worker dispatch")
            }
            EmuCommand::ExecuteTasControl(_) => {
                unreachable!("TAS execution escaped worker dispatch")
            }
            EmuCommand::AdvanceTasControl(_) => {
                unreachable!("TAS frame advance escaped worker dispatch")
            }
            EmuCommand::InspectTasReadiness { .. } => {
                unreachable!("TAS readiness inspection escaped worker dispatch")
            }
            EmuCommand::Shutdown => ControlFlow::Continue(EmuCommand::Shutdown),
            command => match self.authority {
                BackendAuthority::Gameplay => {
                    if observes_replay_activity(&command) {
                        self.replay_activity_unwitnessed = true;
                    }
                    ControlFlow::Continue(command)
                }
                BackendAuthority::Leased { lease_id, .. } => {
                    let crate::emu_thread::EmuCommandAuthority::Gameplay(command) =
                        command.authority_classification()
                    else {
                        unreachable!("authority transition was handled before gameplay dispatch");
                    };
                    ControlFlow::Break(EmuResponse::TasControlCommandRejected { lease_id, command })
                }
            },
        }
    }

    pub(super) fn is_leased(&self) -> bool {
        self.authority != BackendAuthority::Gameplay
    }

    fn active_lease_id(&self) -> Option<u64> {
        match &self.authority {
            BackendAuthority::Gameplay => None,
            BackendAuthority::Leased { lease_id, .. } => Some(*lease_id),
        }
    }

    fn acquire<F>(
        &mut self,
        request_id: u64,
        profile: TasExecutionProfile,
        context: TasControlContext,
        build_witness: F,
    ) -> EmuResponse
    where
        F: FnOnce(
            TasExecutionProfile,
        ) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason>,
    {
        if let Some(reason) = self.acquire_blocker(&context) {
            return EmuResponse::TasControlAcquireRejected { request_id, reason };
        }

        let lease_id = self.next_lease_id;
        let Some(next_lease_id) = lease_id.checked_add(1) else {
            return EmuResponse::TasControlAcquireRejected {
                request_id,
                reason: TasControlAcquireRejectedReason::LeaseIdExhausted,
            };
        };
        let witness = match build_witness(profile) {
            Ok(witness) => witness,
            Err(reason) => {
                return EmuResponse::TasControlAcquireRejected { request_id, reason };
            }
        };
        if witness.profile != profile {
            return EmuResponse::TasControlAcquireRejected {
                request_id,
                reason: TasControlAcquireRejectedReason::StateWitnessUnavailable,
            };
        }
        let checkpoint_sha256 = TasDigest::from_bytes(&witness.current_state_bytes);
        if checkpoint_sha256 != witness.current_state_sha256 {
            return EmuResponse::TasControlAcquireRejected {
                request_id,
                reason: TasControlAcquireRejectedReason::StateWitnessUnavailable,
            };
        }
        self.next_lease_id = next_lease_id;
        let checkpoint = TasControlCheckpoint {
            profile,
            state_bytes: witness.current_state_bytes.clone(),
            state_sha256: checkpoint_sha256,
            frame_count: witness.frame_count,
        };
        let state_format_compatibility_id = witness.state_format_compatibility_id;
        self.authority = BackendAuthority::Leased {
            lease_id,
            profile,
            state_format_compatibility_id,
            checkpoint: Box::new(checkpoint),
            attempted_run_id: None,
            execution_failed: false,
            candidate: None,
            intermediate_cache_proofs: Vec::new(),
        };
        EmuResponse::TasControlAcquired {
            request_id,
            lease_id,
            witness: Box::new(witness),
        }
    }

    fn execute<F>(&mut self, request: TasExecutionRequest, execute: F) -> EmuResponse
    where
        F: FnOnce() -> Result<TasExecutionResult, TasExecutionRejectedReason>,
    {
        let lease_id = request.lease_id;
        let run_id = request.run_id;
        let profile = request.profile;
        let (attempted_run_id, execution_failed, candidate, intermediate_cache_proofs) =
            match &mut self.authority {
                BackendAuthority::Gameplay => {
                    return EmuResponse::TasExecutionRejected {
                        profile,
                        requested_lease_id: lease_id,
                        run_id,
                        reason: TasExecutionRejectedReason::NoActiveLease,
                    };
                }
                BackendAuthority::Leased {
                    lease_id: active_lease_id,
                    ..
                } if *active_lease_id != lease_id => {
                    return EmuResponse::TasExecutionRejected {
                        profile,
                        requested_lease_id: lease_id,
                        run_id,
                        reason: TasExecutionRejectedReason::WrongLease {
                            active_lease_id: *active_lease_id,
                        },
                    };
                }
                BackendAuthority::Leased {
                    profile: active_profile,
                    ..
                } if *active_profile != profile => {
                    return EmuResponse::TasExecutionRejected {
                        profile,
                        requested_lease_id: lease_id,
                        run_id,
                        reason: TasExecutionRejectedReason::WrongExecutionProfile {
                            active_profile: *active_profile,
                        },
                    };
                }
                BackendAuthority::Leased {
                    attempted_run_id,
                    execution_failed,
                    candidate,
                    intermediate_cache_proofs,
                    ..
                } => (
                    attempted_run_id,
                    execution_failed,
                    candidate,
                    intermediate_cache_proofs,
                ),
            };
        if run_id == 0 {
            return EmuResponse::TasExecutionRejected {
                profile,
                requested_lease_id: lease_id,
                run_id,
                reason: TasExecutionRejectedReason::InvalidRunId,
            };
        }
        if *execution_failed {
            return EmuResponse::TasExecutionRejected {
                profile,
                requested_lease_id: lease_id,
                run_id,
                reason: TasExecutionRejectedReason::RunAlreadyAttempted {
                    active_run_id: attempted_run_id.unwrap_or_default(),
                },
            };
        }
        let expected_run_id = attempted_run_id
            .map(|active_run_id| active_run_id.checked_add(1))
            .unwrap_or(Some(1));
        if expected_run_id != Some(run_id) {
            return EmuResponse::TasExecutionRejected {
                profile,
                requested_lease_id: lease_id,
                run_id,
                reason: TasExecutionRejectedReason::RunAlreadyAttempted {
                    active_run_id: attempted_run_id.unwrap_or_default(),
                },
            };
        }
        *attempted_run_id = Some(run_id);
        *candidate = None;
        *intermediate_cache_proofs = request.intermediate_cache_proofs.clone();
        match execute() {
            Ok(result) => {
                *candidate = Some(Box::new(result));
                EmuResponse::TasExecutionCompleted {
                    profile,
                    lease_id,
                    run_id,
                    segment_id: result.segment_id,
                    segment_frame_count: result.segment_frame_count,
                    executed_project_frames: result.executed_project_frames,
                    frame_count: result.frame_count,
                    state_sha256: result.state_sha256,
                }
            }
            Err(reason) => {
                *execution_failed = true;
                EmuResponse::TasExecutionRejected {
                    profile,
                    requested_lease_id: lease_id,
                    run_id,
                    reason,
                }
            }
        }
    }

    fn advance<F>(&mut self, request: TasFrameAdvanceRequest, advance: F) -> EmuResponse
    where
        F: FnOnce(
            TasExecutionResult,
            TasInputFrame,
            Option<crate::emu_thread::TasFrameAdvanceSnapshot>,
        ) -> Result<TasFrameAdvanceResult, TasFrameAdvanceRejectedReason>,
    {
        let candidate = match &mut self.authority {
            BackendAuthority::Gameplay => {
                return reject_advance(&request, TasFrameAdvanceRejectedReason::NoActiveLease);
            }
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                ..
            } if *active_lease_id != request.lease_id => {
                return reject_advance(
                    &request,
                    TasFrameAdvanceRejectedReason::WrongLease {
                        active_lease_id: *active_lease_id,
                    },
                );
            }
            BackendAuthority::Leased { profile, .. } if *profile != request.profile => {
                return reject_advance(
                    &request,
                    TasFrameAdvanceRejectedReason::WrongExecutionProfile {
                        active_profile: *profile,
                    },
                );
            }
            BackendAuthority::Leased {
                attempted_run_id: Some(active_run_id),
                candidate: Some(candidate),
                ..
            } => {
                if *active_run_id != request.run_id {
                    return reject_advance(
                        &request,
                        TasFrameAdvanceRejectedReason::WrongRun {
                            active_run_id: *active_run_id,
                        },
                    );
                }
                candidate
            }
            BackendAuthority::Leased { .. } => {
                return reject_advance(
                    &request,
                    TasFrameAdvanceRejectedReason::NoCompletedExecution,
                );
            }
        };
        if candidate.profile != request.profile {
            return reject_advance(
                &request,
                TasFrameAdvanceRejectedReason::WrongExecutionProfile {
                    active_profile: candidate.profile,
                },
            );
        }
        if request.advance_id == 0 {
            return reject_advance(&request, TasFrameAdvanceRejectedReason::InvalidAdvanceId);
        }
        let Some(expected_advance_id) = candidate.last_advance_id.checked_add(1) else {
            return reject_advance(&request, TasFrameAdvanceRejectedReason::AdvanceIdExhausted);
        };
        if request.advance_id != expected_advance_id {
            return reject_advance(
                &request,
                TasFrameAdvanceRejectedReason::UnexpectedAdvanceId {
                    expected_advance_id,
                },
            );
        }
        if request.expected_segment_frame_count != candidate.segment_frame_count
            || request.expected_executed_project_frames != candidate.executed_project_frames
        {
            return reject_advance(
                &request,
                TasFrameAdvanceRejectedReason::SegmentProofMismatch,
            );
        }
        if request.expected_frame_count != candidate.frame_count
            || request.expected_state_sha256 != candidate.state_sha256
        {
            return reject_advance(
                &request,
                TasFrameAdvanceRejectedReason::CandidateProofMismatch,
            );
        }
        let starts_next_segment = candidate.segment_frame_count == MAX_EDITOR_SEEK_EXECUTION_FRAMES;
        let expected_segment_id = if starts_next_segment {
            let Some(segment_id) = candidate.segment_id.checked_add(1) else {
                return reject_advance(&request, TasFrameAdvanceRejectedReason::SegmentIdExhausted);
            };
            segment_id
        } else {
            candidate.segment_id
        };
        if request.segment_id != expected_segment_id {
            return reject_advance(
                &request,
                TasFrameAdvanceRejectedReason::UnexpectedSegmentId {
                    expected_segment_id,
                },
            );
        }
        let segment_frame_count = if starts_next_segment {
            1
        } else {
            let Some(frame_count) = candidate.segment_frame_count.checked_add(1) else {
                return reject_advance(&request, TasFrameAdvanceRejectedReason::FrameLimitExceeded);
            };
            frame_count
        };
        if segment_frame_count > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
            return reject_advance(&request, TasFrameAdvanceRejectedReason::FrameLimitExceeded);
        }
        let Some(executed_project_frames) = candidate.executed_project_frames.checked_add(1) else {
            return reject_advance(&request, TasFrameAdvanceRejectedReason::FrameLimitExceeded);
        };
        match advance(**candidate, request.input, request.snapshot) {
            Ok(result) => {
                **candidate = TasExecutionResult {
                    profile: request.profile,
                    frame_count: result.frame_count,
                    state_sha256: result.state_sha256,
                    executed_project_frames,
                    segment_id: request.segment_id,
                    segment_frame_count,
                    last_advance_id: request.advance_id,
                    cache_proof: candidate.cache_proof,
                };
                EmuResponse::TasFrameAdvanced {
                    profile: request.profile,
                    lease_id: request.lease_id,
                    run_id: request.run_id,
                    advance_id: request.advance_id,
                    segment_id: request.segment_id,
                    segment_frame_count,
                    executed_project_frames,
                    frame_count: result.frame_count,
                    state_sha256: result.state_sha256,
                    rumble: result.rumble,
                    audio_samples: result.audio_samples,
                    ui_data: result.ui_data,
                }
            }
            Err(reason) => EmuResponse::TasFrameAdvanceRejected {
                profile: request.profile,
                requested_lease_id: request.lease_id,
                run_id: request.run_id,
                advance_id: request.advance_id,
                segment_id: request.segment_id,
                reason,
            },
        }
    }

    pub(super) fn rollback<F>(&mut self, lease_id: u64, restore: F) -> EmuResponse
    where
        F: FnOnce(
            &TasControlCheckpoint,
        ) -> Result<TasRestoredCheckpoint, TasControlRollbackRejectedReason>,
    {
        let checkpoint = match &self.authority {
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                checkpoint,
                ..
            } if *active_lease_id == lease_id => checkpoint,
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                ..
            } => {
                return EmuResponse::TasControlRollbackRejected {
                    requested_lease_id: lease_id,
                    reason: TasControlRollbackRejectedReason::WrongLease {
                        active_lease_id: *active_lease_id,
                    },
                };
            }
            BackendAuthority::Gameplay => {
                return EmuResponse::TasControlRollbackRejected {
                    requested_lease_id: lease_id,
                    reason: TasControlRollbackRejectedReason::NoActiveLease,
                };
            }
        };
        let restored = match restore(checkpoint) {
            Ok(restored) => restored,
            Err(reason) => {
                return EmuResponse::TasControlRollbackRejected {
                    requested_lease_id: lease_id,
                    reason,
                };
            }
        };
        if restored.state_sha256 != checkpoint.state_sha256 {
            return EmuResponse::TasControlRollbackRejected {
                requested_lease_id: lease_id,
                reason: TasControlRollbackRejectedReason::StateDigestMismatch,
            };
        }
        if restored.frame_count != checkpoint.frame_count {
            return EmuResponse::TasControlRollbackRejected {
                requested_lease_id: lease_id,
                reason: TasControlRollbackRejectedReason::FrameCountMismatch,
            };
        }
        self.authority = BackendAuthority::Gameplay;
        EmuResponse::TasControlRolledBack {
            lease_id,
            restored_state_sha256: restored.state_sha256,
            frame_count: restored.frame_count,
        }
    }

    fn commit<F>(&mut self, lease_id: u64, verify: F) -> EmuResponse
    where
        F: FnOnce(TasExecutionResult) -> Result<(), TasControlCommitRejectedReason>,
    {
        let candidate = match &self.authority {
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                candidate: Some(candidate),
                ..
            } if *active_lease_id == lease_id => **candidate,
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                candidate: None,
                ..
            } if *active_lease_id == lease_id => {
                return EmuResponse::TasControlCommitRejected {
                    requested_lease_id: lease_id,
                    reason: TasControlCommitRejectedReason::NoCompletedExecution,
                };
            }
            BackendAuthority::Leased {
                lease_id: active_lease_id,
                ..
            } => {
                return EmuResponse::TasControlCommitRejected {
                    requested_lease_id: lease_id,
                    reason: TasControlCommitRejectedReason::WrongLease {
                        active_lease_id: *active_lease_id,
                    },
                };
            }
            BackendAuthority::Gameplay => {
                return EmuResponse::TasControlCommitRejected {
                    requested_lease_id: lease_id,
                    reason: TasControlCommitRejectedReason::NoActiveLease,
                };
            }
        };
        if let Err(reason) = verify(candidate) {
            return EmuResponse::TasControlCommitRejected {
                requested_lease_id: lease_id,
                reason,
            };
        }
        self.authority = BackendAuthority::Gameplay;
        EmuResponse::TasControlCommitted { lease_id }
    }

    pub(in crate::emu_thread::emu_loop) fn acquire_blocker(
        &self,
        context: &TasControlContext,
    ) -> Option<TasControlAcquireRejectedReason> {
        if let BackendAuthority::Leased { lease_id, .. } = &self.authority {
            return Some(TasControlAcquireRejectedReason::AlreadyLeased {
                lease_id: *lease_id,
            });
        }
        if context.uncapped_execution {
            return Some(TasControlAcquireRejectedReason::UncappedExecution);
        }
        if context.audio_recording_active {
            return Some(TasControlAcquireRejectedReason::AudioRecordingActive);
        }
        if context.link_activity {
            return Some(TasControlAcquireRejectedReason::LinkActivity);
        }
        if context.pending_frame_delivery {
            return Some(TasControlAcquireRejectedReason::PendingFrameDelivery);
        }
        if context.runtime_fault {
            return Some(TasControlAcquireRejectedReason::RuntimeFault);
        }
        if self.replay_activity_unwitnessed {
            return Some(TasControlAcquireRejectedReason::ReplayActivityUnwitnessed);
        }
        None
    }
}

fn reject_advance(
    request: &TasFrameAdvanceRequest,
    reason: TasFrameAdvanceRejectedReason,
) -> EmuResponse {
    EmuResponse::TasFrameAdvanceRejected {
        profile: request.profile,
        requested_lease_id: request.lease_id,
        run_id: request.run_id,
        advance_id: request.advance_id,
        segment_id: request.segment_id,
        reason,
    }
}

fn observes_replay_activity(command: &EmuCommand) -> bool {
    match command {
        EmuCommand::CaptureReplayStart { .. } | EmuCommand::CaptureReplayCheckpoint { .. } => true,
        EmuCommand::StepFrames(input) => input.replay_joypad_frames.is_some(),
        EmuCommand::LoadStateBytes {
            replay_events,
            game_boy_link_start_state,
            game_boy_link_coordinator_start_state,
            game_boy_link_start_tick,
            wonder_swan_link_start_tick,
            ..
        } => {
            replay_events.is_some()
                || game_boy_link_start_state.is_some()
                || game_boy_link_coordinator_start_state.is_some()
                || game_boy_link_start_tick.is_some()
                || wonder_swan_link_start_tick.is_some()
        }
        EmuCommand::RestoreGameBoyLinkState(_) => true,
        _ => false,
    }
}
