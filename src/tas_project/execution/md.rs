use anyhow::{Result, ensure};
use zeff_emu_common::time::MachineTiming;

use super::{
    EmuBackend, TasEditorExecutionOutcome, TasEditorFramebuffer, TasEditorSession,
    materialize_input, validate_project_checkpoint,
};
use crate::emu_backend::md::tas::{MdTasEditorCheckpoint, MdTasEditorOwner};

pub(super) struct MdEditorExecutionState {
    owner: MdTasEditorOwner,
    baseline: MdTasEditorCheckpoint,
}

impl MdEditorExecutionState {
    pub(super) fn new(backend: &mut EmuBackend) -> Result<Self> {
        let EmuBackend::Md(md) = backend else {
            anyhow::bail!("MD editor requires MD backend")
        };
        let owner = md.begin_tas_editor()?;
        let baseline = md.capture_tas_editor(&owner)?;
        ensure!(baseline.frame() == 0, "MD editor baseline is not fresh");
        Ok(Self { owner, baseline })
    }

    pub(super) fn seek(
        &self,
        backend: &mut EmuBackend,
        editor: &mut TasEditorSession,
        target: u64,
        budget: u64,
    ) -> Result<TasEditorExecutionOutcome> {
        let EmuBackend::Md(md) = backend else {
            anyhow::bail!("MD editor lost MD backend")
        };
        md.validate_tas_editor(&self.owner)?;
        let original = md.capture_tas_editor(&self.owner)?;
        let result = self.execute(backend, editor, target, budget);
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let EmuBackend::Md(md) = backend else {
                    unreachable!()
                };
                if let Err(restore) = md.restore_tas_editor(&self.owner, &original) {
                    return Err(error.context(format!("MD preseek restoration failed: {restore}")));
                }
                Err(error)
            }
        }
    }

    fn execute(
        &self,
        backend: &mut EmuBackend,
        editor: &mut TasEditorSession,
        target: u64,
        budget: u64,
    ) -> Result<TasEditorExecutionOutcome> {
        let project = editor.project();
        let branch_id = editor.selected_branch_id();
        let branch = editor.selected_branch();
        let restored =
            editor
                .seek_cache()
                .restore_md_newest(&self.owner, backend, target, |cursor| {
                    project.seek_cache_identity(branch_id, cursor)
                })?;
        let start = restored.unwrap_or(0);
        if restored.is_none() {
            let EmuBackend::Md(md) = backend else {
                unreachable!()
            };
            md.restore_tas_editor(&self.owner, &self.baseline)?;
        }
        validate_project_checkpoint(branch.verification(), start, backend)?;
        let reached = target.min(start.saturating_add(budget));
        for cursor in start..reached {
            backend.apply_replay_input(&materialize_input(project, branch.input_at(cursor))?);
            let frame = backend.frame_count();
            let EmuBackend::Md(md) = &*backend else {
                unreachable!()
            };
            let before = md.core().timing_snapshot().now();
            backend.step_frame();
            ensure!(
                frame.checked_add(1) == Some(backend.frame_count()),
                "MD seek failed exact field progress"
            );
            let EmuBackend::Md(md) = &*backend else {
                unreachable!()
            };
            ensure!(
                md.core().timing_snapshot().now() > before,
                "MD seek failed master progress"
            );
            validate_project_checkpoint(branch.verification(), cursor + 1, backend)?;
        }
        let EmuBackend::Md(md) = &*backend else {
            unreachable!()
        };
        md.validate_tas_editor(&self.owner)?;
        let checkpoint = md.capture_tas_editor(&self.owner)?;
        ensure!(
            checkpoint.frame() == reached,
            "MD editor cursor differs from runtime"
        );
        let identity = project.seek_cache_identity(branch_id, reached)?;
        let (rgba, dimensions) = backend.display_framebuffer();
        let (width, height) =
            dimensions.ok_or_else(|| anyhow::anyhow!("MD display dimensions missing"))?;
        let framebuffer = TasEditorFramebuffer::from_rgba(width, height, rgba.to_vec())?;
        editor.set_cursor(reached)?;
        let cache_store_error = editor
            .seek_cache()
            .store_md(identity, checkpoint, &self.owner, &self.baseline, backend)
            .err()
            .map(|error| error.to_string());
        Ok(TasEditorExecutionOutcome {
            requested_cursor: target,
            cursor: reached,
            restored_cache_cursor: restored,
            executed_frames: reached - start,
            events_applied: 0,
            framebuffer,
            cache_store_error,
        })
    }
}

#[cfg(test)]
mod tests;
