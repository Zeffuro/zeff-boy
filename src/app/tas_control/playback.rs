use anyhow::{Result, bail, ensure};

use crate::emu_thread::{
    TasExecutionProfile, TasFrameAdvanceRejectedReason, TasFrameAdvanceRequest,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasEditorSession};

use super::lifecycle::{ResponseDisposition, WorkerBoundCommand};
use super::messages::frame_advance_rejection_message;
use super::{
    TasControlCoordinator, TasControlState, TasControlTerminalReason, TasEditorControlSnapshot,
};

impl TasControlCoordinator {
    pub(in crate::app) fn start_playback(&mut self, session: &TasEditorSession) -> Result<()> {
        let TasControlState::AwaitingDecision {
            project,
            candidate_executed_project_frames,
            ..
        } = &self.state
        else {
            bail!("the loaded game is not paused at a linked TAS boundary");
        };
        ensure!(
            !self.realtime_recording_active,
            "pause TAS recording before playing the movie"
        );
        ensure!(
            *candidate_executed_project_frames < project.branch_frame_count,
            "the linked game is already at the end of the TAS movie"
        );
        ensure!(
            TasEditorControlSnapshot::capture_at(session, *candidate_executed_project_frames)?
                == *project,
            "the TAS project changed before playback started"
        );
        self.playback_active = true;
        Ok(())
    }

    pub(in crate::app) fn pause_playback(&mut self) {
        self.playback_active = false;
    }

    pub(in crate::app) fn playback_active(&self) -> bool {
        self.playback_active
    }

    pub(in crate::app) fn can_advance_playback(&self) -> bool {
        self.playback_active
            && matches!(
                &self.state,
                TasControlState::AwaitingDecision {
                    project,
                    candidate_executed_project_frames,
                    ..
                } if *candidate_executed_project_frames < project.branch_frame_count
            )
    }

    pub(in crate::app) fn begin_playback_frame(
        &mut self,
        session: &TasEditorSession,
    ) -> Result<WorkerBoundCommand> {
        ensure!(self.playback_active, "TAS movie playback is paused");
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
            bail!("the loaded game is not paused at a linked TAS boundary");
        };
        ensure!(
            *candidate_executed_project_frames < project.branch_frame_count,
            "the linked game is already at the end of the TAS movie"
        );
        let current =
            TasEditorControlSnapshot::capture_at(session, *candidate_executed_project_frames)?;
        let input = TasEditorControlSnapshot::input_at_linked(
            session,
            project,
            &current,
            *candidate_executed_project_frames,
        )?;
        let advance_id = *next_advance_id;
        let next_advance_id = advance_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS playback advance IDs are exhausted"))?;
        let starts_next_segment =
            *candidate_segment_frame_count == MAX_EDITOR_SEEK_EXECUTION_FRAMES;
        let segment_id = if starts_next_segment {
            candidate_segment_id
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS playback segment IDs are exhausted"))?
        } else {
            *candidate_segment_id
        };
        let expected_segment_frame_count = if starts_next_segment {
            1
        } else {
            candidate_segment_frame_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS playback segment length overflows"))?
        };
        let expected_executed_project_frames = candidate_executed_project_frames
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS playback position overflows"))?;
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let profile = project.profile;
        let expected_frame_count = *candidate_frame_count;
        let expected_state_sha256 = *candidate_state_sha256;
        let previous_segment_frame_count = *candidate_segment_frame_count;
        let previous_executed_project_frames = *candidate_executed_project_frames;
        let proof = proof.clone();
        let project = project.clone();
        self.state = TasControlState::PlaybackPending {
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
                expected_segment_frame_count: previous_segment_frame_count,
                expected_executed_project_frames: previous_executed_project_frames,
                expected_frame_count,
                expected_state_sha256,
                input,
                snapshot: None,
            },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app::tas_control) fn consume_playback_advanced(
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
        rumble: bool,
        audio_samples: Vec<f32>,
        ui_data: Option<Box<crate::ui::UiFrameData>>,
        current_project: Option<&TasEditorControlSnapshot>,
    ) -> ResponseDisposition {
        let TasControlState::PlaybackPending {
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
            return self.terminalize_playback_response(worker_generation);
        }
        let Some(current_project) = current_project.filter(|current| {
            current.cursor == executed_project_frames
                && project.matches_linked_project(Some(current))
        }) else {
            return self.rollback_playback(
                worker_generation,
                lease_id,
                proof.clone(),
                "The TAS project changed during playback; the loaded game will be restored"
                    .to_owned(),
            );
        };
        let reached_end = executed_project_frames == current_project.branch_frame_count;
        if executed_project_frames > current_project.branch_frame_count {
            return self.terminalize_playback_response(worker_generation);
        }
        let next_advance_id = *next_advance_id;
        let proof = proof.clone();
        let project = current_project.clone();
        self.state = TasControlState::AwaitingDecision {
            worker_generation,
            lease_id,
            run_id,
            next_advance_id,
            proof,
            project,
            candidate_segment_id: segment_id,
            candidate_segment_frame_count: segment_frame_count,
            candidate_executed_project_frames: executed_project_frames,
            candidate_frame_count: frame_count,
            candidate_state_sha256: state_sha256,
        };
        if reached_end {
            self.pause_playback();
        }
        self.framebuffer_refresh_pending = true;
        ResponseDisposition::PresentPlaybackFrame {
            rumble,
            audio_samples,
            ui_data,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app::tas_control) fn consume_playback_rejection(
        &mut self,
        worker_generation: u64,
        profile: TasExecutionProfile,
        requested_lease_id: u64,
        run_id: u64,
        advance_id: u64,
        segment_id: u64,
        reason: TasFrameAdvanceRejectedReason,
    ) -> ResponseDisposition {
        let TasControlState::PlaybackPending {
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
            return self.terminalize_playback_response(worker_generation);
        }
        if matches!(
            reason,
            TasFrameAdvanceRejectedReason::NoActiveLease
                | TasFrameAdvanceRejectedReason::WrongLease { .. }
        ) {
            self.pause_playback();
            self.state = TasControlState::Terminal {
                worker_generation,
                reason: TasControlTerminalReason::FrameAdvanceAuthorityMismatch,
            };
            return ResponseDisposition::Consumed { follow_up: None };
        }
        self.rollback_playback(
            worker_generation,
            *lease_id,
            proof.clone(),
            frame_advance_rejection_message(reason).to_owned(),
        )
    }

    fn terminalize_playback_response(&mut self, worker_generation: u64) -> ResponseDisposition {
        self.pause_playback();
        self.state = TasControlState::Terminal {
            worker_generation,
            reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
        };
        ResponseDisposition::Consumed { follow_up: None }
    }

    fn rollback_playback(
        &mut self,
        worker_generation: u64,
        lease_id: u64,
        proof: super::TasControlHeldProof,
        error: String,
    ) -> ResponseDisposition {
        self.pause_playback();
        self.pending_error = Some(error);
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
