use anyhow::{Result, bail};

use crate::emu_thread::TasExecutionRequest;
use crate::tas_project::TasDigest;

use super::lifecycle::WorkerBoundCommand;
use super::{
    TasAcquiredProjectBinding, TasControlCoordinator, TasControlStartMode, TasControlState,
};

impl TasControlCoordinator {
    pub(super) fn linked_identity(
        &self,
    ) -> Option<(crate::emu_thread::TasExecutionProfile, TasDigest)> {
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            return None;
        };
        Some((project.profile, project.sync_identity_sha256))
    }

    pub(super) fn linked_cursor(&self) -> Option<u64> {
        let TasControlState::AwaitingDecision {
            candidate_executed_project_frames,
            ..
        } = &self.state
        else {
            return None;
        };
        Some(*candidate_executed_project_frames)
    }

    pub(super) fn begin_linked_seek(
        &mut self,
        binding: TasAcquiredProjectBinding,
    ) -> Result<WorkerBoundCommand> {
        let TasControlState::AwaitingDecision {
            worker_generation,
            lease_id,
            proof,
            project,
            ..
        } = &self.state
        else {
            bail!("the loaded game is not linked to the TAS editor");
        };
        if binding.snapshot.profile != project.profile
            || binding.snapshot.sync_identity_sha256 != project.sync_identity_sha256
        {
            bail!("the TAS project identity changed while linked");
        }

        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let proof = proof.clone();
        let run_id = self.next_run_id;
        self.next_run_id = run_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS linked-run identifiers are exhausted"))?;
        self.stop_realtime_recording();
        self.start_mode = TasControlStartMode::Preview;
        self.state = TasControlState::ExecutionPending {
            worker_generation,
            lease_id,
            run_id,
            proof,
            project: binding.snapshot.clone(),
            total_input_frames: binding.total_input_frames,
        };
        Ok(WorkerBoundCommand::execute(
            worker_generation,
            TasExecutionRequest {
                profile: binding.snapshot.profile,
                lease_id,
                run_id,
                start_state_bytes: binding.start_state_bytes,
                input_prefix: binding.input_prefix,
            },
        ))
    }
}
