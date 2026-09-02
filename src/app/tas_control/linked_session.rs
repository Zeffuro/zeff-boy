use anyhow::{Result, bail, ensure};

use crate::emu_thread::TasExecutionRequest;
use crate::tas_project::TasDigest;

use super::lifecycle::WorkerBoundCommand;
use super::{
    TasAcquiredProjectBinding, TasControlCoordinator, TasControlStartMode, TasControlState,
};

impl TasControlCoordinator {
    pub(in crate::app) fn linked_identity(
        &self,
    ) -> Option<(crate::emu_thread::TasExecutionProfile, TasDigest)> {
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            return None;
        };
        Some((project.profile, project.sync_identity_sha256))
    }

    pub(in crate::app) fn linked_cursor(&self) -> Option<u64> {
        let TasControlState::AwaitingDecision {
            candidate_executed_project_frames,
            ..
        } = &self.state
        else {
            return None;
        };
        Some(*candidate_executed_project_frames)
    }

    pub(in crate::app) fn project_binding_cursor(&self) -> Option<u64> {
        match &self.state {
            TasControlState::AcquireQueued { project, .. }
            | TasControlState::AcquirePending { project, .. }
            | TasControlState::ExecutionPending { project, .. }
            | TasControlState::ExecutionReplayReady { project, .. }
            | TasControlState::ExecutionReplayPending { project, .. }
            | TasControlState::AwaitingDecision { project, .. }
            | TasControlState::FrameAdvancePending { project, .. } => Some(project.cursor),
            TasControlState::PlaybackPending {
                expected_executed_project_frames,
                ..
            } => Some(*expected_executed_project_frames),
            TasControlState::Detached
            | TasControlState::FrameRecordCommitPending { .. }
            | TasControlState::RollbackPending { .. }
            | TasControlState::CommitPending { .. }
            | TasControlState::Terminal { .. } => None,
        }
    }

    pub(super) fn begin_linked_seek(
        &mut self,
        binding: TasAcquiredProjectBinding,
    ) -> Result<WorkerBoundCommand> {
        self.validate_linked_binding(&binding)?;
        self.begin_linked_run(binding)
    }

    pub(super) fn begin_linked_edit_follow(
        &mut self,
        binding: TasAcquiredProjectBinding,
        edited_start: u64,
        edited_end: u64,
    ) -> Result<WorkerBoundCommand> {
        self.validate_linked_binding(&binding)?;
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            bail!("the loaded game is not linked to the TAS editor");
        };
        ensure!(edited_start < edited_end, "the edited TAS range is empty");
        ensure!(
            edited_end == binding.total_input_frames
                && edited_end <= binding.snapshot.branch_frame_count,
            "the TAS edit-follow boundary does not match the edited range"
        );
        ensure!(
            binding.snapshot.branch_id == project.branch_id,
            "the selected TAS branch changed during the input edit"
        );
        ensure!(
            binding.snapshot.branch_frame_count == project.branch_frame_count,
            "TAS edit-follow accepts fixed-length input edits only"
        );
        ensure!(
            project.edit_generation.checked_add(1) == Some(binding.snapshot.edit_generation),
            "TAS edit-follow requires exactly one committed edit transaction"
        );
        ensure!(
            binding.snapshot.project_content_sha256 != project.project_content_sha256,
            "the TAS edit-follow transaction did not change the project"
        );
        self.begin_linked_run(binding)
    }

    fn validate_linked_binding(&self, binding: &TasAcquiredProjectBinding) -> Result<()> {
        ensure!(
            !self.playback_active && !matches!(self.state, TasControlState::PlaybackPending { .. }),
            "pause TAS movie playback before moving the linked game"
        );
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            bail!("the loaded game is not linked to the TAS editor");
        };
        ensure!(
            binding.snapshot.profile == project.profile
                && binding.snapshot.sync_identity_sha256 == project.sync_identity_sha256,
            "the TAS project identity changed while linked"
        );
        Ok(())
    }

    fn begin_linked_run(
        &mut self,
        binding: TasAcquiredProjectBinding,
    ) -> Result<WorkerBoundCommand> {
        let TasControlState::AwaitingDecision {
            worker_generation,
            lease_id,
            proof,
            ..
        } = &self.state
        else {
            bail!("the loaded game is not linked to the TAS editor");
        };

        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let proof = proof.clone();
        let run_id = self.next_run_id;
        self.next_run_id = run_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS linked-run identifiers are exhausted"))?;
        self.stop_realtime_recording();
        self.pause_playback();
        self.start_mode = TasControlStartMode::Preview;
        let predecessor_source_cursors = binding
            .predecessor_window
            .as_ref()
            .map(|window| {
                window
                    .source_proofs
                    .iter()
                    .map(|proof| proof.target_cursor)
                    .collect()
            })
            .unwrap_or_default();
        self.state = TasControlState::ExecutionPending {
            worker_generation,
            lease_id,
            run_id,
            proof,
            project: binding.snapshot.clone(),
            total_input_frames: binding.total_input_frames,
            predecessor_source_cursors,
        };
        Ok(WorkerBoundCommand::execute(
            worker_generation,
            TasExecutionRequest {
                profile: binding.snapshot.profile,
                lease_id,
                run_id,
                cache_proof: binding.snapshot.cache_proof(),
                intermediate_cache_proofs: binding.intermediate_cache_proofs,
                predecessor_window: binding.predecessor_window,
                start_state_bytes: binding.start_state_bytes,
                input_prefix: binding.input_prefix,
            },
        ))
    }
}
