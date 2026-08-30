#![cfg(not(target_arch = "wasm32"))]

use anyhow::{Result, bail};

use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionRejectedReason, TasExecutionRequest,
    TasFrameAdvanceRejectedReason,
};
use crate::tas_project::TasPreparedLiveFrame;

mod integration;
mod lifecycle;
mod linked_session;
mod live_recording;
mod messages;
mod project_binding;
pub(super) mod realtime;
mod staged_execution;
mod state;
#[cfg(test)]
use integration::acquisition_delivery_quiesced;
use lifecycle::{ResponseDisposition, WorkerBoundCommand};
use messages::{
    acquire_rejection_message, execution_rejection_message, frame_advance_rejection_message,
};
use project_binding::{TasAcquiredProjectBinding, TasControlHeldProof, TasEditorControlSnapshot};
use state::{TasControlState, TasControlTerminalReason};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TasControlStartMode {
    Preview,
    Record,
}

pub(super) struct TasControlCoordinator {
    state: TasControlState,
    next_request_id: u64,
    next_run_id: u64,
    pending_live_frame: Option<TasPreparedLiveFrame>,
    realtime_recording_active: bool,
    start_mode: TasControlStartMode,
    pending_error: Option<String>,
    framebuffer_refresh_pending: bool,
}

impl TasControlCoordinator {
    #[allow(dead_code)]
    fn begin_acquire(
        &mut self,
        worker_generation: u64,
        project: TasEditorControlSnapshot,
    ) -> Result<EmuCommand> {
        if self.state != TasControlState::Detached {
            bail!("TAS control is already changing authority");
        }
        self.start_mode = TasControlStartMode::Preview;
        let request_id = self.next_request_id;
        self.next_request_id = request_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS control request IDs are exhausted"))?;
        let profile = project.profile;
        self.state = TasControlState::AcquirePending {
            worker_generation,
            request_id,
            cancelled: false,
            project,
        };
        Ok(EmuCommand::AcquireTasControl {
            request_id,
            profile,
        })
    }

    fn queue_acquire(
        &mut self,
        worker_generation: u64,
        project: TasEditorControlSnapshot,
        mode: TasControlStartMode,
    ) -> Result<()> {
        if self.state != TasControlState::Detached {
            bail!("TAS control is already changing authority");
        }
        if mode == TasControlStartMode::Record
            && project.profile != crate::emu_thread::TasExecutionProfile::DirectNesCartridge
        {
            bail!("live host-input recording is unavailable for this TAS profile");
        }
        self.start_mode = mode;
        self.state = TasControlState::AcquireQueued {
            worker_generation,
            project,
        };
        Ok(())
    }

    fn begin_queued_acquire(&mut self) -> Result<Option<EmuCommand>> {
        let TasControlState::AcquireQueued {
            worker_generation,
            project,
        } = &self.state
        else {
            return Ok(None);
        };
        let worker_generation = *worker_generation;
        let project = project.clone();
        let request_id = self.next_request_id;
        self.next_request_id = request_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS control request IDs are exhausted"))?;
        let profile = project.profile;
        self.state = TasControlState::AcquirePending {
            worker_generation,
            request_id,
            cancelled: false,
            project,
        };
        Ok(Some(EmuCommand::AcquireTasControl {
            request_id,
            profile,
        }))
    }

    fn cancel(&mut self) -> Option<WorkerBoundCommand> {
        self.start_mode = TasControlStartMode::Preview;
        self.stop_realtime_recording();
        match &self.state {
            TasControlState::Detached
            | TasControlState::RollbackPending { .. }
            | TasControlState::CommitPending { .. }
            | TasControlState::Terminal { .. } => None,
            TasControlState::AcquireQueued { .. } => {
                self.state = TasControlState::Detached;
                None
            }
            TasControlState::AcquirePending {
                worker_generation,
                request_id,
                project,
                ..
            } => {
                let next = TasControlState::AcquirePending {
                    worker_generation: *worker_generation,
                    request_id: *request_id,
                    cancelled: true,
                    project: project.clone(),
                };
                self.state = next;
                None
            }
            TasControlState::ExecutionPending {
                worker_generation,
                lease_id,
                proof,
                ..
            }
            | TasControlState::ExecutionReplayReady {
                worker_generation,
                lease_id,
                proof,
                ..
            }
            | TasControlState::ExecutionReplayPending {
                worker_generation,
                lease_id,
                proof,
                ..
            }
            | TasControlState::AwaitingDecision {
                worker_generation,
                lease_id,
                proof,
                ..
            }
            | TasControlState::FrameAdvancePending {
                worker_generation,
                lease_id,
                proof,
                ..
            }
            | TasControlState::FrameRecordCommitPending {
                worker_generation,
                lease_id,
                proof,
                ..
            } => {
                let worker_generation = *worker_generation;
                let lease_id = *lease_id;
                self.pending_live_frame = None;
                let next = TasControlState::RollbackPending {
                    worker_generation,
                    lease_id,
                    checkpoint_sha256: proof.current_state_sha256,
                    checkpoint_frame_count: proof.frame_count,
                };
                let command = WorkerBoundCommand::rollback(worker_generation, lease_id);
                self.state = next;
                Some(command)
            }
        }
    }

    #[allow(dead_code)]
    fn commit(&mut self, current: Option<&TasEditorControlSnapshot>) -> Option<WorkerBoundCommand> {
        self.stop_realtime_recording();
        let TasControlState::AwaitingDecision {
            worker_generation,
            lease_id,
            project,
            proof,
            ..
        } = &self.state
        else {
            return None;
        };
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        if current != Some(project) {
            self.state = TasControlState::RollbackPending {
                worker_generation,
                lease_id,
                checkpoint_sha256: proof.current_state_sha256,
                checkpoint_frame_count: proof.frame_count,
            };
            return Some(WorkerBoundCommand::rollback(worker_generation, lease_id));
        }
        self.state = TasControlState::CommitPending {
            worker_generation,
            lease_id,
        };
        Some(WorkerBoundCommand::commit(worker_generation, lease_id))
    }

    #[cfg(test)]
    fn consume_response(
        &mut self,
        worker_generation: u64,
        response: EmuResponse,
        acquired_project: Option<TasAcquiredProjectBinding>,
        current_project: Option<&TasEditorControlSnapshot>,
    ) -> ResponseDisposition {
        self.consume_response_with_session(
            worker_generation,
            response,
            acquired_project,
            current_project,
            None,
        )
    }

    fn consume_response_with_session(
        &mut self,
        worker_generation: u64,
        response: EmuResponse,
        acquired_project: Option<TasAcquiredProjectBinding>,
        current_project: Option<&TasEditorControlSnapshot>,
        current_session: Option<&crate::tas_project::TasEditorSession>,
    ) -> ResponseDisposition {
        if matches!(self.state, TasControlState::Terminal { .. })
            && matches!(
                &response,
                EmuResponse::TasControlAcquired { .. }
                    | EmuResponse::TasControlAcquireRejected { .. }
                    | EmuResponse::TasExecutionCompleted { .. }
                    | EmuResponse::TasExecutionRejected { .. }
                    | EmuResponse::TasFrameAdvanced { .. }
                    | EmuResponse::TasFrameAdvanceRejected { .. }
                    | EmuResponse::TasControlRolledBack { .. }
                    | EmuResponse::TasControlRollbackRejected { .. }
                    | EmuResponse::TasControlCommitted { .. }
                    | EmuResponse::TasControlCommitRejected { .. }
            )
        {
            return Self::stale_response();
        }
        match response {
            EmuResponse::TasControlAcquired {
                request_id,
                lease_id,
                witness,
            } => {
                let proof = TasControlHeldProof::from_witness(&witness);
                let TasControlState::AcquirePending {
                    worker_generation: expected_worker_generation,
                    request_id: expected_request_id,
                    cancelled,
                    project,
                } = &self.state
                else {
                    return self.stale_acquired(worker_generation, lease_id, proof);
                };
                if worker_generation != *expected_worker_generation
                    || request_id != *expected_request_id
                {
                    return self.stale_acquired(worker_generation, lease_id, proof);
                }
                if *cancelled
                    || acquired_project
                        .as_ref()
                        .is_none_or(|binding| binding.snapshot != *project)
                {
                    if !*cancelled {
                        self.pending_error = Some(
                            "The TAS project changed or no longer matches the loaded game"
                                .to_owned(),
                        );
                    }
                    self.state = TasControlState::RollbackPending {
                        worker_generation,
                        lease_id,
                        checkpoint_sha256: proof.current_state_sha256,
                        checkpoint_frame_count: proof.frame_count,
                    };
                    ResponseDisposition::Consumed {
                        follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                    }
                } else {
                    let acquired =
                        acquired_project.expect("validated TAS project binding should be present");
                    let run_id = 1;
                    self.next_run_id = 2;
                    self.state = TasControlState::ExecutionPending {
                        worker_generation,
                        lease_id,
                        run_id,
                        proof,
                        project: acquired.snapshot.clone(),
                        total_input_frames: acquired.total_input_frames,
                    };
                    ResponseDisposition::Consumed {
                        follow_up: Some(WorkerBoundCommand::execute(
                            worker_generation,
                            TasExecutionRequest {
                                profile: acquired.snapshot.profile,
                                lease_id,
                                run_id,
                                start_state_bytes: acquired.start_state_bytes,
                                input_prefix: acquired.input_prefix,
                            },
                        )),
                    }
                }
            }
            EmuResponse::TasControlAcquireRejected { request_id, reason } => {
                let TasControlState::AcquirePending {
                    worker_generation: expected_worker_generation,
                    request_id: expected_request_id,
                    ..
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation
                    || request_id != *expected_request_id
                {
                    return Self::stale_response();
                }
                self.pending_error = Some(acquire_rejection_message(reason).to_owned());
                self.detach()
            }
            EmuResponse::TasExecutionCompleted {
                profile,
                lease_id,
                run_id,
                segment_id,
                segment_frame_count,
                executed_project_frames,
                frame_count,
                state_sha256,
            } => {
                let TasControlState::ExecutionPending {
                    worker_generation: expected_worker_generation,
                    lease_id: expected_lease_id,
                    run_id: expected_run_id,
                    proof,
                    project,
                    total_input_frames,
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation {
                    return Self::stale_response();
                }
                if profile != project.profile
                    || lease_id != *expected_lease_id
                    || run_id != *expected_run_id
                {
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::ExecutionResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                let worker_generation = *expected_worker_generation;
                let proof = proof.clone();
                let project = project.clone();
                let initial_input_frames =
                    (*total_input_frames).min(crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES);
                if segment_id != 1
                    || segment_frame_count != executed_project_frames
                    || executed_project_frames != initial_input_frames
                {
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::ExecutionResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                if current_project != Some(&project) {
                    self.stop_realtime_recording();
                    self.state = TasControlState::RollbackPending {
                        worker_generation,
                        lease_id,
                        checkpoint_sha256: proof.current_state_sha256,
                        checkpoint_frame_count: proof.frame_count,
                    };
                    return ResponseDisposition::Consumed {
                        follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                    };
                }
                if executed_project_frames == *total_input_frames {
                    self.state = TasControlState::AwaitingDecision {
                        worker_generation,
                        lease_id,
                        run_id,
                        next_advance_id: 1,
                        proof,
                        project,
                        candidate_segment_id: segment_id,
                        candidate_segment_frame_count: segment_frame_count,
                        candidate_executed_project_frames: executed_project_frames,
                        candidate_frame_count: frame_count,
                        candidate_state_sha256: state_sha256,
                    };
                    self.framebuffer_refresh_pending = true;
                    ResponseDisposition::Consumed { follow_up: None }
                } else {
                    self.state = TasControlState::ExecutionReplayReady {
                        worker_generation,
                        lease_id,
                        run_id,
                        next_advance_id: 1,
                        proof,
                        project,
                        candidate_segment_id: segment_id,
                        candidate_segment_frame_count: segment_frame_count,
                        candidate_executed_project_frames: executed_project_frames,
                        candidate_frame_count: frame_count,
                        candidate_state_sha256: state_sha256,
                        total_input_frames: *total_input_frames,
                    };
                    ResponseDisposition::ContinueExecutionReplay
                }
            }
            EmuResponse::TasExecutionRejected {
                profile,
                requested_lease_id,
                run_id,
                reason,
            } => {
                let TasControlState::ExecutionPending {
                    worker_generation: expected_worker_generation,
                    lease_id,
                    run_id: expected_run_id,
                    proof,
                    project,
                    ..
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation {
                    return Self::stale_response();
                }
                if profile != project.profile
                    || requested_lease_id != *lease_id
                    || run_id != *expected_run_id
                {
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::ExecutionResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                let worker_generation = *expected_worker_generation;
                let lease_id = *lease_id;
                let proof = proof.clone();
                self.pending_error = Some(execution_rejection_message(reason).to_owned());
                self.stop_realtime_recording();
                if matches!(
                    reason,
                    TasExecutionRejectedReason::NoActiveLease
                        | TasExecutionRejectedReason::WrongLease { .. }
                        | TasExecutionRejectedReason::RunAlreadyAttempted { .. }
                ) {
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::ExecutionAuthorityMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                self.state = TasControlState::RollbackPending {
                    worker_generation,
                    lease_id,
                    checkpoint_sha256: proof.current_state_sha256,
                    checkpoint_frame_count: proof.frame_count,
                };
                ResponseDisposition::Consumed {
                    follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                }
            }
            EmuResponse::TasFrameAdvanced {
                profile,
                lease_id,
                run_id,
                advance_id,
                segment_id,
                segment_frame_count,
                executed_project_frames,
                frame_count,
                state_sha256,
            } => {
                if matches!(self.state, TasControlState::ExecutionReplayPending { .. }) {
                    return self.consume_execution_replay_advanced(
                        worker_generation,
                        profile,
                        lease_id,
                        run_id,
                        advance_id,
                        segment_id,
                        segment_frame_count,
                        executed_project_frames,
                        frame_count,
                        state_sha256,
                        current_session,
                    );
                }
                let TasControlState::FrameAdvancePending {
                    worker_generation: expected_worker_generation,
                    lease_id: expected_lease_id,
                    run_id: expected_run_id,
                    advance_id: expected_advance_id,
                    next_advance_id,
                    segment_id: expected_segment_id,
                    expected_segment_frame_count,
                    expected_executed_project_frames,
                    proof,
                    project,
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation {
                    return Self::stale_response();
                }
                if profile != project.profile
                    || lease_id != *expected_lease_id
                    || run_id != *expected_run_id
                    || advance_id != *expected_advance_id
                    || segment_id != *expected_segment_id
                    || segment_frame_count != *expected_segment_frame_count
                    || executed_project_frames != *expected_executed_project_frames
                {
                    self.pending_live_frame = None;
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                let worker_generation = *expected_worker_generation;
                let proof = proof.clone();
                if current_project != Some(project) {
                    self.pending_live_frame = None;
                    self.stop_realtime_recording();
                    self.state = TasControlState::RollbackPending {
                        worker_generation,
                        lease_id,
                        checkpoint_sha256: proof.current_state_sha256,
                        checkpoint_frame_count: proof.frame_count,
                    };
                    return ResponseDisposition::Consumed {
                        follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                    };
                }
                let Some(prepared) = self.pending_live_frame.take() else {
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                };
                self.state = TasControlState::FrameRecordCommitPending {
                    worker_generation,
                    lease_id,
                    run_id,
                    advance_id,
                    next_advance_id: *next_advance_id,
                    candidate_segment_id: segment_id,
                    candidate_segment_frame_count: segment_frame_count,
                    candidate_executed_project_frames: executed_project_frames,
                    proof,
                    candidate_frame_count: frame_count,
                    candidate_state_sha256: state_sha256,
                };
                ResponseDisposition::CommitLiveFrame {
                    prepared: Box::new(prepared),
                }
            }
            EmuResponse::TasFrameAdvanceRejected {
                profile,
                requested_lease_id,
                run_id,
                advance_id,
                segment_id,
                reason,
            } => {
                if matches!(self.state, TasControlState::ExecutionReplayPending { .. }) {
                    return self.consume_execution_replay_rejection(
                        worker_generation,
                        profile,
                        requested_lease_id,
                        run_id,
                        advance_id,
                        segment_id,
                        reason,
                    );
                }
                let TasControlState::FrameAdvancePending {
                    worker_generation: expected_worker_generation,
                    lease_id,
                    run_id: expected_run_id,
                    advance_id: expected_advance_id,
                    segment_id: expected_segment_id,
                    proof,
                    project,
                    ..
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation {
                    return Self::stale_response();
                }
                if profile != project.profile
                    || requested_lease_id != *lease_id
                    || run_id != *expected_run_id
                    || advance_id != *expected_advance_id
                    || segment_id != *expected_segment_id
                {
                    self.pending_live_frame = None;
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                let worker_generation = *expected_worker_generation;
                let lease_id = *lease_id;
                let proof = proof.clone();
                self.pending_live_frame = None;
                self.stop_realtime_recording();
                self.pending_error = Some(frame_advance_rejection_message(reason).to_owned());
                if matches!(
                    reason,
                    TasFrameAdvanceRejectedReason::NoActiveLease
                        | TasFrameAdvanceRejectedReason::WrongLease { .. }
                ) {
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::FrameAdvanceAuthorityMismatch,
                    };
                    return ResponseDisposition::Consumed { follow_up: None };
                }
                self.state = TasControlState::RollbackPending {
                    worker_generation,
                    lease_id,
                    checkpoint_sha256: proof.current_state_sha256,
                    checkpoint_frame_count: proof.frame_count,
                };
                ResponseDisposition::Consumed {
                    follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                }
            }
            EmuResponse::TasControlRolledBack {
                lease_id,
                restored_state_sha256,
                frame_count,
            } => {
                let TasControlState::RollbackPending {
                    worker_generation: expected_worker_generation,
                    lease_id: expected_lease_id,
                    checkpoint_sha256,
                    checkpoint_frame_count,
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation
                    || lease_id != *expected_lease_id
                {
                    return Self::stale_response();
                }
                if restored_state_sha256 != *checkpoint_sha256
                    || frame_count != *checkpoint_frame_count
                {
                    self.stop_realtime_recording();
                    self.state = TasControlState::Terminal {
                        worker_generation,
                        reason: TasControlTerminalReason::RollbackResponseMismatch,
                    };
                    ResponseDisposition::Consumed { follow_up: None }
                } else {
                    self.framebuffer_refresh_pending = true;
                    self.detach()
                }
            }
            EmuResponse::TasControlRollbackRejected {
                requested_lease_id, ..
            } => {
                let TasControlState::RollbackPending {
                    worker_generation: expected_worker_generation,
                    lease_id,
                    ..
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation
                    || requested_lease_id != *lease_id
                {
                    return Self::stale_response();
                }
                self.state = TasControlState::Terminal {
                    worker_generation,
                    reason: TasControlTerminalReason::RollbackRejected,
                };
                self.stop_realtime_recording();
                ResponseDisposition::Consumed { follow_up: None }
            }
            EmuResponse::TasControlCommitted { lease_id } => {
                let TasControlState::CommitPending {
                    worker_generation: expected_worker_generation,
                    lease_id: expected_lease_id,
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation
                    || lease_id != *expected_lease_id
                {
                    return Self::stale_response();
                }
                self.detach()
            }
            EmuResponse::TasControlCommitRejected {
                requested_lease_id, ..
            } => {
                let TasControlState::CommitPending {
                    worker_generation: expected_worker_generation,
                    lease_id,
                } = &self.state
                else {
                    return Self::stale_response();
                };
                if worker_generation != *expected_worker_generation
                    || requested_lease_id != *lease_id
                {
                    return Self::stale_response();
                }
                self.state = TasControlState::Terminal {
                    worker_generation,
                    reason: TasControlTerminalReason::CommitRejected,
                };
                self.stop_realtime_recording();
                ResponseDisposition::Consumed { follow_up: None }
            }
            response => ResponseDisposition::Unrelated(response),
        }
    }

    fn detach(&mut self) -> ResponseDisposition {
        self.pending_live_frame = None;
        self.stop_realtime_recording();
        self.state = TasControlState::Detached;
        ResponseDisposition::Consumed { follow_up: None }
    }

    fn stale_response() -> ResponseDisposition {
        ResponseDisposition::Consumed { follow_up: None }
    }

    fn stale_acquired(
        &mut self,
        worker_generation: u64,
        lease_id: u64,
        proof: TasControlHeldProof,
    ) -> ResponseDisposition {
        let state_generation = match &self.state {
            TasControlState::Detached => None,
            TasControlState::AcquireQueued {
                worker_generation, ..
            }
            | TasControlState::AcquirePending {
                worker_generation, ..
            }
            | TasControlState::ExecutionPending {
                worker_generation, ..
            }
            | TasControlState::ExecutionReplayReady {
                worker_generation, ..
            }
            | TasControlState::ExecutionReplayPending {
                worker_generation, ..
            }
            | TasControlState::AwaitingDecision {
                worker_generation, ..
            }
            | TasControlState::FrameAdvancePending {
                worker_generation, ..
            }
            | TasControlState::FrameRecordCommitPending {
                worker_generation, ..
            }
            | TasControlState::RollbackPending {
                worker_generation, ..
            }
            | TasControlState::CommitPending {
                worker_generation, ..
            }
            | TasControlState::Terminal {
                worker_generation, ..
            } => Some(*worker_generation),
        };
        if state_generation.is_none_or(|generation| generation == worker_generation) {
            self.pending_live_frame = None;
            self.stop_realtime_recording();
            self.state = TasControlState::RollbackPending {
                worker_generation,
                lease_id,
                checkpoint_sha256: proof.current_state_sha256,
                checkpoint_frame_count: proof.frame_count,
            };
        }
        ResponseDisposition::Consumed {
            follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
        }
    }
}

#[cfg(test)]
mod tests;
