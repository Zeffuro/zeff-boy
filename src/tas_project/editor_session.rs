#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::editor_history::{
    MAX_TAS_EDITOR_HISTORY_BYTES, MAX_TAS_EDITOR_HISTORY_ENTRIES, TasEditorHistory,
    TasEditorHistoryEntry, TasEditorProjectWitness, project_sha256, project_witness,
};
use super::{
    TasAutosaveRecovery, TasAutosaveSave, TasAutosaveStore, TasBranch, TasDigest, TasEditOutcome,
    TasProject, TasProjectEdit, TasProjectLoadSource, TasSeekStateCache,
};

#[path = "editor_session/frame_range.rs"]
mod frame_range;
#[path = "editor_session/live_frame.rs"]
mod live_frame;
#[path = "editor_session/replay_conversion.rs"]
mod replay_conversion;

pub use frame_range::TasFrameRange;
use live_frame::TasLiveFrameHistoryGroup;
pub use live_frame::{TasLiveRecordingMode, TasPreparedLiveFrame};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TasEditorSessionSource {
    Unsaved,
    Primary,
    Backup,
    Autosave,
}

impl From<TasProjectLoadSource> for TasEditorSessionSource {
    fn from(source: TasProjectLoadSource) -> Self {
        match source {
            TasProjectLoadSource::Primary => Self::Primary,
            TasProjectLoadSource::Backup => Self::Backup,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TasEditorAutosaveRecovery {
    pub generation: u64,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug)]
struct TasEditorPersistenceBaseline {
    source: TasEditorSessionSource,
    manual_saved: Option<TasEditorProjectWitness>,
    last_autosaved: Option<TasEditorProjectWitness>,
}

#[derive(Clone, Debug)]
pub struct TasEditorSession {
    project: TasProject,
    manual_path: PathBuf,
    selected_branch_id: String,
    cursor: u64,
    project_sha256: TasDigest,
    manual_saved: Option<TasEditorProjectWitness>,
    last_autosaved: Option<TasEditorProjectWitness>,
    history: TasEditorHistory,
    history_revision: u128,
    live_frame_history_group: Option<TasLiveFrameHistoryGroup>,
    live_frame_history_epoch: u128,
    autosave_store: TasAutosaveStore,
    seek_cache: TasSeekStateCache,
    source: TasEditorSessionSource,
}

impl TasEditorSession {
    pub fn new(
        project: TasProject,
        manual_path: impl Into<PathBuf>,
        autosave_store: TasAutosaveStore,
        seek_cache: TasSeekStateCache,
    ) -> Result<Self> {
        let manual_path = manual_path.into();
        validate_manual_path(&manual_path)?;
        project.validate()?;
        Self::from_project(
            project,
            manual_path,
            autosave_store,
            seek_cache,
            TasEditorPersistenceBaseline {
                source: TasEditorSessionSource::Unsaved,
                manual_saved: None,
                last_autosaved: None,
            },
        )
    }

    pub fn open(
        manual_path: impl Into<PathBuf>,
        autosave_store: TasAutosaveStore,
        seek_cache: TasSeekStateCache,
    ) -> Result<Self> {
        let manual_path = manual_path.into();
        validate_manual_path(&manual_path)?;
        let (project, load_source) = TasProject::load_with_backup(&manual_path)?;
        let project_witness = project_witness(&project)?;
        let (manual_saved, last_autosaved) = match load_source {
            TasProjectLoadSource::Primary => (Some(project_witness), Some(project_witness)),
            TasProjectLoadSource::Backup => (None, None),
        };
        Self::from_project(
            project,
            manual_path,
            autosave_store,
            seek_cache,
            TasEditorPersistenceBaseline {
                source: load_source.into(),
                manual_saved,
                last_autosaved,
            },
        )
    }

    pub fn recover_newest_autosave(
        project_id: &str,
        manual_path: impl Into<PathBuf>,
        autosave_store: TasAutosaveStore,
        seek_cache: TasSeekStateCache,
    ) -> Result<Option<Self>> {
        let manual_path = manual_path.into();
        validate_manual_path(&manual_path)?;
        let Some(recovery) = autosave_store.recover_newest(project_id)? else {
            return Ok(None);
        };
        let project_witness = project_witness(&recovery.project)?;
        Ok(Some(Self::from_project(
            recovery.project,
            manual_path,
            autosave_store,
            seek_cache,
            TasEditorPersistenceBaseline {
                source: TasEditorSessionSource::Autosave,
                manual_saved: None,
                last_autosaved: Some(project_witness),
            },
        )?))
    }

    fn from_project(
        project: TasProject,
        manual_path: PathBuf,
        autosave_store: TasAutosaveStore,
        seek_cache: TasSeekStateCache,
        baseline: TasEditorPersistenceBaseline,
    ) -> Result<Self> {
        let selected_branch_id = project.active_branch_id().to_owned();
        let project_sha256 = project_sha256(&project)?;
        Ok(Self {
            project,
            manual_path,
            selected_branch_id,
            cursor: 0,
            project_sha256,
            manual_saved: baseline.manual_saved,
            last_autosaved: baseline.last_autosaved,
            history: TasEditorHistory::new(
                MAX_TAS_EDITOR_HISTORY_ENTRIES,
                MAX_TAS_EDITOR_HISTORY_BYTES,
            ),
            history_revision: 0,
            live_frame_history_group: None,
            live_frame_history_epoch: 0,
            autosave_store,
            seek_cache,
            source: baseline.source,
        })
    }

    pub fn project(&self) -> &TasProject {
        &self.project
    }

    pub fn manual_path(&self) -> &Path {
        &self.manual_path
    }

    pub fn source(&self) -> TasEditorSessionSource {
        self.source
    }

    pub fn selected_branch_id(&self) -> &str {
        &self.selected_branch_id
    }

    pub fn selected_branch(&self) -> &TasBranch {
        self.project
            .branch(&self.selected_branch_id)
            .expect("TAS editor selection should always name a validated branch")
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    pub fn manual_saved_generation(&self) -> Option<u64> {
        self.manual_saved.map(|witness| witness.generation)
    }

    pub fn last_autosaved_generation(&self) -> Option<u64> {
        self.last_autosaved.map(|witness| witness.generation)
    }

    pub fn is_dirty(&self) -> bool {
        self.manual_saved != Some(self.current_project_witness())
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.history.undo.len()
    }

    pub fn redo_count(&self) -> usize {
        self.history.redo.len()
    }

    pub(crate) fn discard_edits_after(&mut self, undo_count: usize) -> Result<()> {
        if undo_count > self.history.undo.len() {
            bail!("TAS editor discard point is no longer available");
        }
        while self.history.undo.len() > undo_count {
            self.restore_history_direction(true)?;
        }
        if !self.history.redo.is_empty() {
            self.history.redo.clear();
            self.note_history_mutation();
        }
        Ok(())
    }

    pub fn autosave_directory(&self) -> &Path {
        self.autosave_store.directory()
    }

    pub fn seek_cache_root(&self) -> &Path {
        self.seek_cache.root()
    }

    pub fn select_branch(&mut self, branch_id: &str) -> Result<TasEditOutcome> {
        let frame_count = self.branch_frame_count(branch_id)?;
        self.select_branch_at_cursor(branch_id, self.cursor.min(frame_count))
    }

    pub fn select_branch_at_cursor(
        &mut self,
        branch_id: &str,
        cursor: u64,
    ) -> Result<TasEditOutcome> {
        let frame_count = self.branch_frame_count(branch_id)?;
        if cursor > frame_count {
            bail!("TAS editor cursor is past selected branch end");
        }

        let before = self.capture_history_entry()?;
        let outcome = self
            .project
            .edit_transaction(|edit| edit.set_active_branch(branch_id))?;
        self.selected_branch_id = branch_id.to_owned();
        self.cursor = cursor;
        self.finish_edit(&outcome, before)?;
        Ok(outcome)
    }

    pub fn set_cursor(&mut self, cursor: u64) -> Result<()> {
        if cursor > self.selected_branch().frame_count() {
            bail!("TAS editor cursor is past selected branch end");
        }
        self.cursor = cursor;
        Ok(())
    }

    pub fn edit_transaction<F>(&mut self, edit: F) -> Result<TasEditOutcome>
    where
        F: FnOnce(&mut TasProjectEdit<'_>) -> Result<()>,
    {
        let before = self.capture_history_entry()?;
        let outcome = self.project.edit_transaction(edit)?;
        self.selected_branch_id = self.project.active_branch_id().to_owned();
        self.cursor = self.cursor.min(self.selected_branch().frame_count());
        self.finish_edit(&outcome, before)?;
        Ok(outcome)
    }

    pub fn undo(&mut self) -> Result<bool> {
        self.restore_history_direction(true)
    }

    pub fn redo(&mut self) -> Result<bool> {
        self.restore_history_direction(false)
    }

    pub fn save_manual(&mut self) -> Result<()> {
        self.project.save_atomic(&self.manual_path)?;
        self.manual_saved = Some(self.current_project_witness());
        self.source = TasEditorSessionSource::Primary;
        Ok(())
    }

    pub fn autosave_if_changed(&mut self) -> Result<Option<TasAutosaveSave>> {
        let witness = self.current_project_witness();
        if self.last_autosaved == Some(witness) {
            return Ok(None);
        }
        Ok(Some(self.autosave_now()?))
    }

    pub fn autosave_now(&mut self) -> Result<TasAutosaveSave> {
        let saved = self.autosave_store.save(&self.project)?;
        self.last_autosaved = Some(self.current_project_witness());
        Ok(saved)
    }

    pub fn install_newest_autosave(&mut self) -> Result<Option<TasEditorAutosaveRecovery>> {
        let project_id = self.project.project_id().to_owned();
        let Some(TasAutosaveRecovery {
            generation,
            path,
            project,
        }) = self.autosave_store.recover_newest(&project_id)?
        else {
            return Ok(None);
        };

        self.project = project;
        self.selected_branch_id = self.project.active_branch_id().to_owned();
        self.cursor = self.cursor.min(self.selected_branch().frame_count());
        self.project_sha256 = project_sha256(&self.project)?;
        self.manual_saved = None;
        self.last_autosaved = Some(self.current_project_witness());
        if !self.history.undo.is_empty() || !self.history.redo.is_empty() {
            self.history.clear();
            self.note_history_mutation();
        }
        self.source = TasEditorSessionSource::Autosave;
        Ok(Some(TasEditorAutosaveRecovery { generation, path }))
    }

    pub fn load_seek_state(&self) -> Result<Option<Vec<u8>>> {
        let identity = self
            .project
            .seek_cache_identity(&self.selected_branch_id, self.cursor)?;
        self.seek_cache.load(&identity)
    }

    pub(crate) fn project_content_sha256(&self) -> TasDigest {
        self.project_sha256
    }

    pub(crate) fn seek_cache(&self) -> &TasSeekStateCache {
        &self.seek_cache
    }

    #[cfg(test)]
    pub(crate) fn load_seek_state_at_or_before(
        &self,
        target_cursor: u64,
    ) -> Result<Option<(u64, Vec<u8>)>> {
        if target_cursor > self.selected_branch().frame_count() {
            bail!("TAS editor seek target is past selected branch end");
        }
        self.seek_cache
            .load_newest_matching(target_cursor, |cursor| {
                self.project
                    .seek_cache_identity(&self.selected_branch_id, cursor)
            })
    }

    pub fn store_seek_state(&self, state: &[u8]) -> Result<()> {
        let identity = self
            .project
            .seek_cache_identity(&self.selected_branch_id, self.cursor)?;
        self.seek_cache.store(&identity, state)
    }

    fn branch_frame_count(&self, branch_id: &str) -> Result<u64> {
        self.project
            .branch(branch_id)
            .map(TasBranch::frame_count)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))
    }

    fn current_project_witness(&self) -> TasEditorProjectWitness {
        TasEditorProjectWitness {
            generation: self.project.edit_generation(),
            project_sha256: self.project_sha256,
        }
    }

    fn capture_history_entry(&self) -> Result<TasEditorHistoryEntry> {
        let entry = TasEditorHistoryEntry {
            project_bytes: self.project.encode_editor_history_snapshot()?,
            selected_branch_id: self.selected_branch_id.clone(),
            cursor: self.cursor,
        };
        if !self.history.undo.can_retain(&entry) {
            bail!(
                "TAS project snapshot exceeds the bounded editor history budget of {} bytes",
                self.history.undo.max_bytes
            );
        }
        Ok(entry)
    }

    fn finish_edit(
        &mut self,
        outcome: &TasEditOutcome,
        before: TasEditorHistoryEntry,
    ) -> Result<()> {
        if !outcome.changed {
            return Ok(());
        }
        self.project_sha256 = project_sha256(&self.project)?;
        self.history.undo.push(before);
        self.history.redo.clear();
        self.note_history_mutation();
        Ok(())
    }

    fn restore_history_direction(&mut self, undo: bool) -> Result<bool> {
        let source = if undo {
            &self.history.undo
        } else {
            &self.history.redo
        };
        let Some(target) = source.last() else {
            return Ok(false);
        };
        let restored_project = TasProject::decode(&target.project_bytes)?;
        let restored_branch = restored_project
            .branch(&target.selected_branch_id)
            .ok_or_else(|| anyhow::anyhow!("TAS editor history names an unknown branch"))?;
        if target.cursor > restored_branch.frame_count() {
            bail!("TAS editor history cursor is past the restored branch end");
        }
        let current = self.capture_history_entry()?;
        let target = if undo {
            self.history
                .undo
                .pop()
                .expect("validated TAS undo entry should still exist")
        } else {
            self.history
                .redo
                .pop()
                .expect("validated TAS redo entry should still exist")
        };
        if undo {
            self.history.redo.push(current);
        } else {
            self.history.undo.push(current);
        }
        self.project_sha256 = project_sha256(&restored_project)?;
        self.project = restored_project;
        self.selected_branch_id = target.selected_branch_id;
        self.cursor = target.cursor;
        self.note_history_mutation();
        Ok(true)
    }

    fn note_history_mutation(&mut self) {
        self.history_revision = self.history_revision.wrapping_add(1);
    }
}

fn validate_manual_path(path: &Path) -> Result<()> {
    if !TasProject::is_project_path(path) {
        bail!("manual TAS project must use the .ztas extension");
    }
    Ok(())
}

#[cfg(test)]
#[path = "editor_session/tests.rs"]
mod tests;
