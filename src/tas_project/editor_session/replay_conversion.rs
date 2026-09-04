use super::*;
use std::sync::atomic::AtomicBool;

use crate::tas_project::verification::{
    TasExecutionSession, TasExecutionWitness, TasVerifiedReplayExportPhase,
};

impl TasEditorSession {
    #[cfg(test)]
    pub(crate) fn verify_save_and_export_active_branch(
        &mut self,
        replay_path: &Path,
        witness: &TasExecutionWitness,
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<PathBuf> {
        let branch_id = self.selected_branch_id.clone();
        self.project
            .verify_branch_with_factory(&branch_id, witness, &mut load_backend)?;
        self.project_sha256 = project_sha256(&self.project)?;
        self.save_manual()?;
        self.project.export_verified_zrpl_with_factory(
            &branch_id,
            replay_path,
            witness,
            load_backend,
        )
    }

    pub(crate) fn verify_save_and_export_active_branch_cancellable(
        &mut self,
        replay_path: &Path,
        witness: &TasExecutionWitness,
        cancellation: &AtomicBool,
        progress: &mut impl FnMut(TasVerifiedReplayExportPhase),
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<PathBuf> {
        let branch_id = self.selected_branch_id.clone();
        self.project.verify_branch_with_factory_cancellable(
            &branch_id,
            witness,
            cancellation,
            progress,
            &mut load_backend,
        )?;
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            anyhow::bail!("verified replay export was canceled");
        }
        self.project_sha256 = project_sha256(&self.project)?;
        progress(TasVerifiedReplayExportPhase::SavingProject);
        self.save_manual()?;
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            anyhow::bail!("verified replay export was canceled");
        }
        progress(TasVerifiedReplayExportPhase::PublishingReplay);
        self.project.export_verified_zrpl_with_factory_cancellable(
            &branch_id,
            replay_path,
            witness,
            cancellation,
            load_backend,
        )
    }
}
