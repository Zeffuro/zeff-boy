use anyhow::{Result, bail};

use crate::emu_thread::{TasFrameAdvanceRequest, TasInputFrame};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasPreparedLiveFrame};

use super::lifecycle::{ResponseDisposition, WorkerBoundCommand};
use super::project_binding::TasEditorControlSnapshot;
use super::{TasControlCoordinator, TasControlState};

impl TasControlCoordinator {
    pub(in crate::app) fn can_record_live_input(&self) -> bool {
        matches!(
            self.state,
            TasControlState::AwaitingDecision {
                project: TasEditorControlSnapshot {
                    profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                    ..
                },
                ..
            }
        )
    }

    pub(in crate::app) fn live_frame_in_flight(&self) -> bool {
        matches!(
            self.state,
            TasControlState::FrameAdvancePending { .. }
                | TasControlState::FrameRecordCommitPending { .. }
        )
    }

    #[allow(dead_code)]
    pub(in crate::app) fn start_realtime_recording(&mut self) -> Result<()> {
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            bail!("no completed TAS execution is awaiting a realtime recording frame");
        };
        if project.profile != crate::emu_thread::TasExecutionProfile::DirectNesCartridge {
            bail!("live host-input recording is unavailable for this TAS profile");
        }
        self.start_mode = super::TasControlStartMode::Preview;
        self.realtime_recording_active = true;
        Ok(())
    }

    pub(in crate::app) fn take_realtime_recording_start_request(&mut self) -> bool {
        if self.start_mode == super::TasControlStartMode::Record && self.can_record_live_input() {
            self.start_mode = super::TasControlStartMode::Preview;
            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub(in crate::app) fn stop_realtime_recording(&mut self) {
        self.start_mode = super::TasControlStartMode::Preview;
        self.realtime_recording_active = false;
    }

    #[allow(dead_code)]
    pub(in crate::app) fn realtime_recording_active(&self) -> bool {
        self.realtime_recording_active
    }

    pub(in crate::app) fn begin_live_frame_advance(
        &mut self,
        prepared: TasPreparedLiveFrame,
    ) -> Result<WorkerBoundCommand> {
        let TasControlState::AwaitingDecision {
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
        } = &self.state
        else {
            bail!("no completed TAS execution is awaiting a live input frame");
        };
        if project.profile != crate::emu_thread::TasExecutionProfile::DirectNesCartridge {
            bail!("live host-input recording is unavailable for this TAS profile");
        }
        let advance_id = *next_advance_id;
        let next_advance_id = advance_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS live-frame advance IDs are exhausted"))?;
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let proof = proof.clone();
        let project = project.clone();
        let profile = project.profile;
        let candidate_segment_id = *candidate_segment_id;
        let candidate_segment_frame_count = *candidate_segment_frame_count;
        let candidate_executed_project_frames = *candidate_executed_project_frames;
        let candidate_frame_count = *candidate_frame_count;
        let candidate_state_sha256 = *candidate_state_sha256;
        let starts_next_segment = candidate_segment_frame_count == MAX_EDITOR_SEEK_EXECUTION_FRAMES;
        let segment_id = if starts_next_segment {
            candidate_segment_id
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS live-frame segment IDs are exhausted"))?
        } else {
            candidate_segment_id
        };
        let expected_segment_frame_count = if starts_next_segment {
            1
        } else {
            candidate_segment_frame_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS live-frame segment length overflows"))?
        };
        let expected_executed_project_frames = candidate_executed_project_frames
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS live-frame project position overflows"))?;
        let input = direct_nes_input_from_prepared(&prepared);
        self.pending_live_frame = Some(prepared);
        self.state = TasControlState::FrameAdvancePending {
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
        };
        Ok(WorkerBoundCommand::advance_frame(
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
        ))
    }

    pub(in crate::app::tas_control) fn finish_live_frame_commit(
        &mut self,
        committed: Result<TasEditorControlSnapshot>,
    ) -> ResponseDisposition {
        let TasControlState::FrameRecordCommitPending {
            worker_generation,
            lease_id,
            run_id,
            advance_id: _,
            next_advance_id,
            candidate_segment_id,
            candidate_segment_frame_count,
            candidate_executed_project_frames,
            proof,
            candidate_frame_count,
            candidate_state_sha256,
        } = &self.state
        else {
            return Self::stale_response();
        };
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let proof = proof.clone();
        let next_advance_id = *next_advance_id;
        let candidate_segment_id = *candidate_segment_id;
        let candidate_segment_frame_count = *candidate_segment_frame_count;
        let candidate_executed_project_frames = *candidate_executed_project_frames;
        let candidate_frame_count = *candidate_frame_count;
        let candidate_state_sha256 = *candidate_state_sha256;
        match committed {
            Ok(project) => {
                self.state = TasControlState::AwaitingDecision {
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
                };
                self.framebuffer_refresh_pending = true;
                ResponseDisposition::Consumed { follow_up: None }
            }
            Err(error) => {
                self.stop_realtime_recording();
                self.pending_error = Some(format!(
                    "Could not record the live TAS input; the loaded game will be restored: {error:#}"
                ));
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
    }
}

fn direct_nes_input_from_prepared(prepared: &TasPreparedLiveFrame) -> TasInputFrame {
    let input = prepared.input();
    TasInputFrame {
        p1_buttons: input.players[0].buttons,
        p1_dpad: input.players[0].dpad,
        p2_buttons: input.players[1].buttons,
        p2_dpad: input.players[1].dpad,
    }
}
