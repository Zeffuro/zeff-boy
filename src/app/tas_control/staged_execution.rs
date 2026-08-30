use anyhow::Result;

use crate::emu_thread::{
    TasExecutionProfile, TasFrameAdvanceRejectedReason, TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasEditorSession};

use super::lifecycle::{ResponseDisposition, WorkerBoundCommand};
use super::messages::frame_advance_rejection_message;
use super::{
    TasControlCoordinator, TasControlState, TasControlTerminalReason, TasEditorControlSnapshot,
};

impl TasControlCoordinator {
    pub(in crate::app) fn execution_replay_pending(&self) -> bool {
        matches!(self.state, TasControlState::ExecutionReplayPending { .. })
    }

    pub(in crate::app::tas_control) fn continue_execution_replay(
        &mut self,
        session: Option<&TasEditorSession>,
    ) -> ResponseDisposition {
        let TasControlState::ExecutionReplayReady {
            worker_generation,
            lease_id,
            run_id,
            next_advance_id,
            proof,
            project,
            candidate_segment_id,
            candidate_segment_frame_count,
            candidate_executed_project_frames,
            candidate_frame_count,
            candidate_state_sha256,
            total_input_frames,
        } = &self.state
        else {
            return Self::stale_response();
        };
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let advance_id = *next_advance_id;
        let proof = proof.clone();
        let project = project.clone();
        let profile = project.profile;
        let candidate_segment_id = *candidate_segment_id;
        let candidate_segment_frame_count = *candidate_segment_frame_count;
        let candidate_executed_project_frames = *candidate_executed_project_frames;
        let candidate_frame_count = *candidate_frame_count;
        let candidate_state_sha256 = *candidate_state_sha256;
        let total_input_frames = *total_input_frames;
        let result = session
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))
            .and_then(|session| {
                staged_input(
                    session,
                    &project,
                    candidate_executed_project_frames,
                    total_input_frames,
                )
            });
        let input = match result {
            Ok(input) => input,
            Err(error) => {
                return self.rollback_staged_execution(
                    worker_generation,
                    lease_id,
                    &proof,
                    format!("Could not continue staging the TAS project; the loaded game will be restored: {error:#}"),
                );
            }
        };
        let Some(next_advance_id) = advance_id.checked_add(1) else {
            return self.rollback_staged_execution(
                worker_generation,
                lease_id,
                &proof,
                "TAS staged-execution advance IDs are exhausted".to_owned(),
            );
        };
        let starts_next_segment = candidate_segment_frame_count == MAX_EDITOR_SEEK_EXECUTION_FRAMES;
        let segment_id = if starts_next_segment {
            match candidate_segment_id.checked_add(1) {
                Some(segment_id) => segment_id,
                None => {
                    return self.rollback_staged_execution(
                        worker_generation,
                        lease_id,
                        &proof,
                        "TAS staged-execution segment IDs are exhausted".to_owned(),
                    );
                }
            }
        } else {
            candidate_segment_id
        };
        let expected_segment_frame_count = if starts_next_segment {
            1
        } else {
            match candidate_segment_frame_count.checked_add(1) {
                Some(frame_count) => frame_count,
                None => {
                    return self.rollback_staged_execution(
                        worker_generation,
                        lease_id,
                        &proof,
                        "TAS staged-execution segment length overflows".to_owned(),
                    );
                }
            }
        };
        let Some(expected_executed_project_frames) =
            candidate_executed_project_frames.checked_add(1)
        else {
            return self.rollback_staged_execution(
                worker_generation,
                lease_id,
                &proof,
                "TAS staged-execution project position overflows".to_owned(),
            );
        };
        self.state = TasControlState::ExecutionReplayPending {
            worker_generation,
            lease_id,
            run_id,
            advance_id,
            next_advance_id,
            segment_id,
            expected_segment_frame_count,
            expected_executed_project_frames,
            proof,
            project,
            total_input_frames,
        };
        ResponseDisposition::Consumed {
            follow_up: Some(WorkerBoundCommand::advance_frame(
                worker_generation,
                TasFrameAdvanceRequest {
                    profile,
                    lease_id,
                    run_id,
                    advance_id,
                    segment_id,
                    expected_segment_frame_count: candidate_segment_frame_count,
                    expected_executed_project_frames: candidate_executed_project_frames,
                    expected_frame_count: candidate_frame_count,
                    expected_state_sha256: candidate_state_sha256,
                    input,
                },
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app::tas_control) fn consume_execution_replay_advanced(
        &mut self,
        worker_generation: u64,
        profile: TasExecutionProfile,
        lease_id: u64,
        run_id: u64,
        advance_id: u64,
        segment_id: u64,
        segment_frame_count: u64,
        executed_project_frames: u64,
        frame_count: u64,
        state_sha256: crate::tas_project::TasDigest,
        current_session: Option<&TasEditorSession>,
    ) -> ResponseDisposition {
        let TasControlState::ExecutionReplayPending {
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
            || advance_id != *expected_advance_id
            || segment_id != *expected_segment_id
            || segment_frame_count != *expected_segment_frame_count
            || executed_project_frames != *expected_executed_project_frames
        {
            self.stop_realtime_recording();
            self.state = TasControlState::Terminal {
                worker_generation,
                reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
            };
            return ResponseDisposition::Consumed { follow_up: None };
        }
        let proof = proof.clone();
        let project = project.clone();
        let total_input_frames = *total_input_frames;
        let project_matches = if executed_project_frames == total_input_frames {
            current_session
                .and_then(|session| TasEditorControlSnapshot::capture(session).ok())
                .as_ref()
                == Some(&project)
        } else {
            current_session.is_some_and(|session| project.matches_session_guard(session))
        };
        if !project_matches {
            return self.rollback_staged_execution(
                worker_generation,
                lease_id,
                &proof,
                "The TAS project changed during staged execution; the loaded game will be restored"
                    .to_owned(),
            );
        }
        if executed_project_frames > total_input_frames {
            self.stop_realtime_recording();
            self.state = TasControlState::Terminal {
                worker_generation,
                reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
            };
            return ResponseDisposition::Consumed { follow_up: None };
        }
        if executed_project_frames == total_input_frames {
            self.state = TasControlState::AwaitingDecision {
                worker_generation,
                lease_id,
                run_id,
                next_advance_id: *next_advance_id,
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
                next_advance_id: *next_advance_id,
                proof,
                project,
                candidate_segment_id: segment_id,
                candidate_segment_frame_count: segment_frame_count,
                candidate_executed_project_frames: executed_project_frames,
                candidate_frame_count: frame_count,
                candidate_state_sha256: state_sha256,
                total_input_frames,
            };
            ResponseDisposition::ContinueExecutionReplay
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app::tas_control) fn consume_execution_replay_rejection(
        &mut self,
        worker_generation: u64,
        profile: TasExecutionProfile,
        requested_lease_id: u64,
        run_id: u64,
        advance_id: u64,
        segment_id: u64,
        reason: TasFrameAdvanceRejectedReason,
    ) -> ResponseDisposition {
        let TasControlState::ExecutionReplayPending {
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
            self.stop_realtime_recording();
            self.state = TasControlState::Terminal {
                worker_generation,
                reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
            };
            return ResponseDisposition::Consumed { follow_up: None };
        }
        let lease_id = *lease_id;
        let proof = proof.clone();
        self.pending_error = Some(frame_advance_rejection_message(reason).to_owned());
        if matches!(
            reason,
            TasFrameAdvanceRejectedReason::NoActiveLease
                | TasFrameAdvanceRejectedReason::WrongLease { .. }
        ) {
            self.stop_realtime_recording();
            self.state = TasControlState::Terminal {
                worker_generation,
                reason: TasControlTerminalReason::FrameAdvanceAuthorityMismatch,
            };
            return ResponseDisposition::Consumed { follow_up: None };
        }
        self.rollback_staged_execution(worker_generation, lease_id, &proof, String::new())
    }

    fn rollback_staged_execution(
        &mut self,
        worker_generation: u64,
        lease_id: u64,
        proof: &super::TasControlHeldProof,
        error: String,
    ) -> ResponseDisposition {
        if !error.is_empty() {
            self.pending_error = Some(error);
        }
        self.stop_realtime_recording();
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
}

fn staged_input(
    session: &TasEditorSession,
    project: &TasEditorControlSnapshot,
    cursor: u64,
    total_input_frames: u64,
) -> Result<TasInputFrame> {
    anyhow::ensure!(
        cursor < total_input_frames,
        "staged TAS execution has no remaining input"
    );
    TasEditorControlSnapshot::input_at(session, project, cursor)
}
