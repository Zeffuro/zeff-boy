use super::{
    TasControlCoordinator, TasControlHeldProof, TasControlState, TasControlTerminalReason,
    TasEditorControlSnapshot,
};
use crate::emu_thread::{EmuCommand, EmuResponse, TasExecutionRequest, TasFrameAdvanceRequest};
use crate::tas_project::TasPreparedLiveFrame;

pub(super) enum ResponseDisposition {
    Unrelated(EmuResponse),
    Consumed {
        follow_up: Option<WorkerBoundCommand>,
    },
    CommitLiveFrame {
        prepared: Box<TasPreparedLiveFrame>,
        rumble: bool,
        audio_samples: Vec<f32>,
        ui_data: Option<Box<crate::ui::UiFrameData>>,
    },
    PresentPlaybackFrame {
        rumble: bool,
        audio_samples: Vec<f32>,
        ui_data: Option<Box<crate::ui::UiFrameData>>,
    },
    ContinueExecutionReplay,
}

pub(in crate::app) struct WorkerBoundCommand {
    pub(super) worker_generation: u64,
    command: EmuCommand,
}

impl WorkerBoundCommand {
    pub(super) fn execute(worker_generation: u64, request: TasExecutionRequest) -> Self {
        Self {
            worker_generation,
            command: EmuCommand::ExecuteTasControl(Box::new(request)),
        }
    }

    pub(super) fn advance_frame(worker_generation: u64, request: TasFrameAdvanceRequest) -> Self {
        Self {
            worker_generation,
            command: EmuCommand::AdvanceTasControl(Box::new(request)),
        }
    }

    pub(super) fn rollback(worker_generation: u64, lease_id: u64) -> Self {
        Self {
            worker_generation,
            command: EmuCommand::RollbackTasControl { lease_id },
        }
    }

    pub(super) fn commit(worker_generation: u64, lease_id: u64) -> Self {
        Self {
            worker_generation,
            command: EmuCommand::CommitTasControl { lease_id },
        }
    }

    pub(super) fn into_parts_for_worker(self, worker_generation: u64) -> Option<(u64, EmuCommand)> {
        (self.worker_generation == worker_generation)
            .then_some((self.worker_generation, self.command))
    }
}

impl TasControlCoordinator {
    pub(super) fn detach(&mut self) -> ResponseDisposition {
        self.pending_live_frame = None;
        self.stop_realtime_recording();
        self.pause_playback();
        self.state = TasControlState::Detached;
        ResponseDisposition::Consumed { follow_up: None }
    }

    pub(super) fn stale_response() -> ResponseDisposition {
        ResponseDisposition::Consumed { follow_up: None }
    }

    pub(super) fn stale_acquired(
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
            | TasControlState::PlaybackPending {
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

    pub(super) fn terminalize_worker(
        &mut self,
        worker_generation: u64,
        reason: TasControlTerminalReason,
    ) -> bool {
        match &self.state {
            TasControlState::AcquireQueued {
                worker_generation: expected,
                ..
            }
            | TasControlState::AcquirePending {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionReplayReady {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionReplayPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::AwaitingDecision {
                worker_generation: expected,
                ..
            }
            | TasControlState::FrameAdvancePending {
                worker_generation: expected,
                ..
            }
            | TasControlState::PlaybackPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::FrameRecordCommitPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::RollbackPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::CommitPending {
                worker_generation: expected,
                ..
            } if worker_generation == *expected => {}
            TasControlState::Terminal {
                worker_generation: expected,
                ..
            } if worker_generation == *expected => {
                self.stop_realtime_recording();
                return true;
            }
            _ => return false,
        }
        self.pending_live_frame = None;
        self.stop_realtime_recording();
        self.pause_playback();
        self.state = TasControlState::Terminal {
            worker_generation,
            reason,
        };
        true
    }

    pub(super) fn reconcile_project(
        &mut self,
        current: Option<&TasEditorControlSnapshot>,
    ) -> Option<WorkerBoundCommand> {
        match &self.state {
            TasControlState::AcquireQueued { project, .. } if current != Some(project) => {
                self.stop_realtime_recording();
                self.state = TasControlState::Detached;
                return None;
            }
            TasControlState::AcquirePending {
                worker_generation,
                request_id,
                cancelled,
                project,
            } if current != Some(project) && !cancelled => {
                let next = TasControlState::AcquirePending {
                    worker_generation: *worker_generation,
                    request_id: *request_id,
                    cancelled: true,
                    project: project.clone(),
                };
                self.stop_realtime_recording();
                self.state = next;
                return None;
            }
            _ => {}
        }

        let rollback = match &mut self.state {
            TasControlState::ExecutionPending {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            }
            | TasControlState::ExecutionReplayReady {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            }
            | TasControlState::ExecutionReplayPending {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            }
            | TasControlState::AwaitingDecision {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            }
            | TasControlState::FrameAdvancePending {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            } => {
                if project.matches_linked_project(current) {
                    return None;
                }
                if project.can_rebind_at_same_execution(current) {
                    *project = current
                        .expect("equivalent TAS binding should be present")
                        .clone();
                    return None;
                }
                Some((
                    *worker_generation,
                    *lease_id,
                    proof.current_state_sha256,
                    proof.frame_count,
                ))
            }
            TasControlState::PlaybackPending {
                worker_generation,
                lease_id,
                proof,
                project,
                ..
            } => {
                if project.matches_linked_project(current) {
                    return None;
                }
                Some((
                    *worker_generation,
                    *lease_id,
                    proof.current_state_sha256,
                    proof.frame_count,
                ))
            }
            _ => return None,
        };
        let (worker_generation, lease_id, checkpoint_sha256, checkpoint_frame_count) = rollback?;
        let command = WorkerBoundCommand::rollback(worker_generation, lease_id);
        self.pending_live_frame = None;
        self.stop_realtime_recording();
        self.pause_playback();
        self.state = TasControlState::RollbackPending {
            worker_generation,
            lease_id,
            checkpoint_sha256,
            checkpoint_frame_count,
        };
        Some(command)
    }

    pub(super) fn retire_worker(&mut self, worker_generation: u64) -> bool {
        match &self.state {
            TasControlState::Detached => return false,
            TasControlState::AcquireQueued {
                worker_generation: expected,
                ..
            }
            | TasControlState::AcquirePending {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionReplayReady {
                worker_generation: expected,
                ..
            }
            | TasControlState::ExecutionReplayPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::AwaitingDecision {
                worker_generation: expected,
                ..
            }
            | TasControlState::FrameAdvancePending {
                worker_generation: expected,
                ..
            }
            | TasControlState::PlaybackPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::FrameRecordCommitPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::RollbackPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::CommitPending {
                worker_generation: expected,
                ..
            }
            | TasControlState::Terminal {
                worker_generation: expected,
                ..
            } if worker_generation == *expected => {}
            _ => return false,
        }
        self.pending_live_frame = None;
        self.stop_realtime_recording();
        self.pause_playback();
        self.worker_cache_cursors.clear();
        self.state = TasControlState::Detached;
        true
    }
}
