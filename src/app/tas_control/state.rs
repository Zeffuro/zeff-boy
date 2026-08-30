use super::{TasControlCoordinator, TasControlHeldProof, TasEditorControlSnapshot};
use crate::tas_project::TasDigest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TasControlState {
    Detached,
    AcquireQueued {
        worker_generation: u64,
        project: TasEditorControlSnapshot,
    },
    AcquirePending {
        worker_generation: u64,
        request_id: u64,
        cancelled: bool,
        project: TasEditorControlSnapshot,
    },
    ExecutionPending {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        proof: TasControlHeldProof,
        project: TasEditorControlSnapshot,
        total_input_frames: u64,
    },
    ExecutionReplayReady {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        next_advance_id: u64,
        proof: TasControlHeldProof,
        project: TasEditorControlSnapshot,
        candidate_segment_id: u64,
        candidate_segment_frame_count: u64,
        candidate_executed_project_frames: u64,
        candidate_frame_count: u64,
        candidate_state_sha256: TasDigest,
        total_input_frames: u64,
    },
    ExecutionReplayPending {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        advance_id: u64,
        next_advance_id: u64,
        segment_id: u64,
        expected_segment_frame_count: u64,
        expected_executed_project_frames: u64,
        proof: TasControlHeldProof,
        project: TasEditorControlSnapshot,
        total_input_frames: u64,
    },
    AwaitingDecision {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        next_advance_id: u64,
        proof: TasControlHeldProof,
        project: TasEditorControlSnapshot,
        candidate_segment_id: u64,
        candidate_segment_frame_count: u64,
        candidate_executed_project_frames: u64,
        candidate_frame_count: u64,
        candidate_state_sha256: TasDigest,
    },
    FrameAdvancePending {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        advance_id: u64,
        next_advance_id: u64,
        segment_id: u64,
        expected_segment_frame_count: u64,
        expected_executed_project_frames: u64,
        proof: TasControlHeldProof,
        project: TasEditorControlSnapshot,
    },
    FrameRecordCommitPending {
        worker_generation: u64,
        lease_id: u64,
        run_id: u64,
        advance_id: u64,
        next_advance_id: u64,
        candidate_segment_id: u64,
        candidate_segment_frame_count: u64,
        candidate_executed_project_frames: u64,
        proof: TasControlHeldProof,
        candidate_frame_count: u64,
        candidate_state_sha256: TasDigest,
    },
    RollbackPending {
        worker_generation: u64,
        lease_id: u64,
        checkpoint_sha256: TasDigest,
        checkpoint_frame_count: u64,
    },
    CommitPending {
        worker_generation: u64,
        lease_id: u64,
    },
    Terminal {
        worker_generation: u64,
        reason: TasControlTerminalReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TasControlTerminalReason {
    RollbackRejected,
    RollbackResponseMismatch,
    CommitRejected,
    ExecutionAuthorityMismatch,
    ExecutionResponseMismatch,
    FrameAdvanceAuthorityMismatch,
    FrameAdvanceResponseMismatch,
    RuntimeFault,
    CommandChannelClosed,
    ResponseChannelClosed,
}

impl TasControlCoordinator {
    pub(in crate::app) fn new() -> Self {
        Self {
            state: TasControlState::Detached,
            next_request_id: 1,
            next_run_id: 1,
            pending_live_frame: None,
            realtime_recording_active: false,
            start_mode: super::TasControlStartMode::Preview,
            pending_error: None,
            framebuffer_refresh_pending: false,
        }
    }

    pub(in crate::app) fn gameplay_commands_allowed(&self) -> bool {
        self.state == TasControlState::Detached
    }

    pub(super) fn live_status(&self) -> crate::debug::TasEditorLiveStatus {
        if self.realtime_recording_active
            && matches!(
                self.state,
                TasControlState::AwaitingDecision { .. }
                    | TasControlState::FrameAdvancePending { .. }
                    | TasControlState::FrameRecordCommitPending { .. }
            )
        {
            return crate::debug::TasEditorLiveStatus::Recording;
        }
        match &self.state {
            TasControlState::Detached => crate::debug::TasEditorLiveStatus::Ready {
                recording_available: false,
            },
            TasControlState::AcquireQueued { .. } | TasControlState::AcquirePending { .. } => {
                crate::debug::TasEditorLiveStatus::Acquiring
            }
            TasControlState::ExecutionPending {
                total_input_frames, ..
            } => crate::debug::TasEditorLiveStatus::Staging {
                completed: 0,
                total: *total_input_frames,
            },
            TasControlState::ExecutionReplayReady {
                candidate_executed_project_frames,
                total_input_frames,
                ..
            } => crate::debug::TasEditorLiveStatus::Staging {
                completed: *candidate_executed_project_frames,
                total: *total_input_frames,
            },
            TasControlState::ExecutionReplayPending {
                expected_executed_project_frames: candidate_executed_project_frames,
                total_input_frames,
                ..
            } => crate::debug::TasEditorLiveStatus::Staging {
                completed: candidate_executed_project_frames.saturating_sub(1),
                total: *total_input_frames,
            },
            TasControlState::AwaitingDecision {
                project,
                candidate_executed_project_frames,
                ..
            } => crate::debug::TasEditorLiveStatus::Linked {
                cursor: *candidate_executed_project_frames,
                recording_available: project.profile
                    == crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
            },
            TasControlState::FrameAdvancePending { .. }
            | TasControlState::FrameRecordCommitPending { .. } => {
                crate::debug::TasEditorLiveStatus::AdvancingFrame
            }
            TasControlState::RollbackPending { .. } => crate::debug::TasEditorLiveStatus::Returning,
            TasControlState::CommitPending { .. } => crate::debug::TasEditorLiveStatus::Keeping,
            TasControlState::Terminal { reason, .. } => {
                crate::debug::TasEditorLiveStatus::Terminal(terminal_message(*reason).to_owned())
            }
        }
    }

    pub(super) fn take_error(&mut self) -> Option<String> {
        self.pending_error.take()
    }

    pub(super) fn take_framebuffer_refresh(&mut self) -> bool {
        std::mem::take(&mut self.framebuffer_refresh_pending)
    }
}

fn terminal_message(reason: TasControlTerminalReason) -> &'static str {
    match reason {
        TasControlTerminalReason::RollbackRejected => "The loaded game could not be restored",
        TasControlTerminalReason::RollbackResponseMismatch => {
            "The restored game state could not be verified"
        }
        TasControlTerminalReason::CommitRejected => {
            "The staged game state could not be kept safely"
        }
        TasControlTerminalReason::ExecutionAuthorityMismatch => {
            "The live run lost its emulator authority"
        }
        TasControlTerminalReason::ExecutionResponseMismatch => {
            "The live run returned an unexpected result"
        }
        TasControlTerminalReason::FrameAdvanceAuthorityMismatch => {
            "The live input advance lost its emulator authority"
        }
        TasControlTerminalReason::FrameAdvanceResponseMismatch => {
            "The live input advance returned an unexpected result"
        }
        TasControlTerminalReason::RuntimeFault => "The emulator stopped during the live run",
        TasControlTerminalReason::CommandChannelClosed
        | TasControlTerminalReason::ResponseChannelClosed => {
            "The emulator worker became unavailable"
        }
    }
}
