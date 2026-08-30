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

#[path = "editor_session/live_frame.rs"]
mod live_frame;

use live_frame::TasLiveFrameHistoryGroup;
pub use live_frame::TasPreparedLiveFrame;

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
            project_bytes: self.project.encode()?,
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
        self.project_sha256 = TasDigest::from_bytes(&target.project_bytes);
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
mod tests {
    use super::*;
    use crate::tas_project::{TasAutosaveConfig, TasInputFrame};

    fn stores(root: &Path, manual_path: &Path) -> (TasAutosaveStore, TasSeekStateCache) {
        let autosaves =
            TasAutosaveStore::beside_manual_save(manual_path, TasAutosaveConfig::default())
                .unwrap();
        let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
        (autosaves, seek_cache)
    }

    #[test]
    fn clean_dirty_and_manual_autosave_generations_remain_separate() {
        let root = crate::test_support::test_directory("tas-editor-session-dirty").unwrap();
        let manual_path = root.path().join("movie.ztas");
        crate::tas_project::tests::project()
            .save_atomic(&manual_path)
            .unwrap();
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::open(&manual_path, autosaves, seek_cache).unwrap();

        assert_eq!(session.source(), TasEditorSessionSource::Primary);
        assert!(!session.is_dirty());
        assert_eq!(session.manual_saved_generation(), Some(3));
        assert_eq!(session.last_autosaved_generation(), Some(3));
        assert!(session.autosave_if_changed().unwrap().is_none());

        session
            .edit_transaction(|edit| {
                edit.set_project_comment("edited");
                Ok(())
            })
            .unwrap();
        assert!(session.is_dirty());
        assert_eq!(session.project().edit_generation(), 4);
        assert_eq!(session.manual_saved_generation(), Some(3));

        let autosave = session.autosave_if_changed().unwrap().unwrap();
        assert_eq!(session.last_autosaved_generation(), Some(4));
        assert_eq!(session.manual_saved_generation(), Some(3));
        assert!(session.is_dirty());
        assert!(session.autosave_if_changed().unwrap().is_none());

        session.save_manual().unwrap();
        assert!(!session.is_dirty());
        assert_eq!(session.manual_saved_generation(), Some(4));
        assert_eq!(session.last_autosaved_generation(), Some(4));
        assert_eq!(TasProject::load(&manual_path).unwrap(), *session.project());
        assert_eq!(
            TasProject::load(&autosave.path).unwrap(),
            *session.project()
        );
    }

    #[test]
    fn backup_open_and_autosave_recovery_are_explicit_and_non_destructive() {
        let root = crate::test_support::test_directory("tas-editor-session-recovery").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let original = crate::tas_project::tests::project();
        original.save_atomic(&manual_path).unwrap();
        let mut newer = original.clone();
        newer
            .edit_transaction(|edit| {
                edit.set_project_comment("newer manual");
                Ok(())
            })
            .unwrap();
        newer.save_atomic(&manual_path).unwrap();
        std::fs::write(&manual_path, b"corrupt primary").unwrap();
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);

        let backup_session =
            TasEditorSession::open(&manual_path, autosaves.clone(), seek_cache.clone()).unwrap();
        assert_eq!(backup_session.source(), TasEditorSessionSource::Backup);
        assert!(backup_session.is_dirty());
        assert_eq!(backup_session.project(), &original);

        let mut backup_session = backup_session;
        assert!(backup_session.autosave_if_changed().unwrap().is_some());
        assert_eq!(backup_session.last_autosaved_generation(), Some(3));
        assert!(backup_session.autosave_if_changed().unwrap().is_none());
        assert_eq!(std::fs::read(&manual_path).unwrap(), b"corrupt primary");

        let mut recovered = newer.clone();
        recovered
            .edit_transaction(|edit| {
                edit.set_project_comment("autosaved recovery");
                Ok(())
            })
            .unwrap();
        let recovered_autosave = autosaves.save(&recovered).unwrap();
        let corrupt_manual_bytes = std::fs::read(&manual_path).unwrap();

        let installed = backup_session.install_newest_autosave().unwrap().unwrap();
        assert_eq!(installed.generation, recovered_autosave.generation);
        assert_eq!(installed.path, recovered_autosave.path);
        assert_eq!(backup_session.source(), TasEditorSessionSource::Autosave);
        assert_eq!(backup_session.project(), &recovered);
        assert!(backup_session.is_dirty());
        assert_eq!(std::fs::read(&manual_path).unwrap(), corrupt_manual_bytes);

        let recovered_session = TasEditorSession::recover_newest_autosave(
            original.project_id(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap()
        .unwrap();
        assert_eq!(recovered_session.source(), TasEditorSessionSource::Autosave);
        assert!(recovered_session.is_dirty());
        assert_eq!(recovered_session.project(), &recovered);
        assert_eq!(std::fs::read(&manual_path).unwrap(), corrupt_manual_bytes);

        let valid_manual_path = root.path().join("recoverable.ztas");
        original.save_atomic(&valid_manual_path).unwrap();
        let valid_manual_bytes = std::fs::read(&valid_manual_path).unwrap();
        let (valid_autosaves, valid_seek_cache) = stores(root.path(), &valid_manual_path);
        valid_autosaves.save(&recovered).unwrap();
        let mut recovered_session = TasEditorSession::recover_newest_autosave(
            original.project_id(),
            &valid_manual_path,
            valid_autosaves,
            valid_seek_cache,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            std::fs::read(&valid_manual_path).unwrap(),
            valid_manual_bytes
        );
        recovered_session.save_manual().unwrap();
        assert!(!recovered_session.is_dirty());
        assert_eq!(
            TasProject::load(&valid_manual_path).unwrap(),
            *recovered_session.project()
        );
    }

    #[test]
    fn selection_is_validated_and_cursor_clamps_after_timeline_edits() {
        let root = crate::test_support::test_directory("tas-editor-session-cursor").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();

        session.set_cursor(12).unwrap();
        assert!(session.set_cursor(13).is_err());
        assert!(session.select_branch_at_cursor("missing", 0).is_err());
        assert_eq!(session.selected_branch_id(), "main");
        assert_eq!(session.cursor(), 12);

        session
            .edit_transaction(|edit| edit.delete_frames("main", 5, 7))
            .unwrap();
        assert_eq!(session.cursor(), 5);
        assert_eq!(session.selected_branch().frame_count(), 5);

        session.select_branch("alternate").unwrap();
        assert_eq!(session.selected_branch_id(), "alternate");
        assert_eq!(session.project().active_branch_id(), "alternate");
        assert_eq!(session.cursor(), 5);
        assert!(session.select_branch_at_cursor("alternate", 13).is_err());
        assert_eq!(session.cursor(), 5);
    }

    #[test]
    fn edit_at_n_keeps_cursor_n_seek_state_and_misses_later_prefixes() {
        let root = crate::test_support::test_directory("tas-editor-session-seek").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();

        session.set_cursor(6).unwrap();
        session.store_seek_state(b"before frame six").unwrap();
        session.set_cursor(7).unwrap();
        session.store_seek_state(b"after frame six").unwrap();

        session
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    6,
                    1,
                    TasInputFrame {
                        tilt_x_bits: 1,
                        ..TasInputFrame::default()
                    },
                )
            })
            .unwrap();

        session.set_cursor(6).unwrap();
        assert_eq!(
            session.load_seek_state().unwrap().as_deref(),
            Some(b"before frame six".as_slice())
        );
        session.set_cursor(7).unwrap();
        assert!(session.load_seek_state().unwrap().is_none());
    }

    #[test]
    fn seek_selection_uses_newest_still_eligible_prefix_at_or_before_target() {
        let root = crate::test_support::test_directory("tas-editor-session-seek-newest").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();

        session.set_cursor(2).unwrap();
        session.store_seek_state(b"cursor two").unwrap();
        session.set_cursor(5).unwrap();
        session.store_seek_state(b"cursor five").unwrap();
        assert_eq!(
            session.load_seek_state_at_or_before(7).unwrap(),
            Some((5, b"cursor five".to_vec()))
        );

        session
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    4,
                    1,
                    TasInputFrame {
                        tilt_x_bits: 1,
                        ..TasInputFrame::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(
            session.load_seek_state_at_or_before(7).unwrap(),
            Some((2, b"cursor two".to_vec()))
        );
        assert!(session.load_seek_state_at_or_before(13).is_err());
    }

    #[test]
    fn undo_and_redo_restore_exact_project_selection_and_cursor() {
        let root = crate::test_support::test_directory("tas-editor-session-history-exact").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();

        session.set_cursor(12).unwrap();
        let original_bytes = session.project().encode().unwrap();
        session.select_branch_at_cursor("alternate", 5).unwrap();
        let alternate_bytes = session.project().encode().unwrap();

        assert_eq!(session.undo_count(), 1);
        assert_eq!(session.redo_count(), 0);
        assert!(session.undo().unwrap());
        assert_eq!(session.project().encode().unwrap(), original_bytes);
        assert_eq!(session.selected_branch_id(), "main");
        assert_eq!(session.cursor(), 12);
        assert_eq!(session.project().active_branch_id(), "main");

        assert!(session.redo().unwrap());
        assert_eq!(session.project().encode().unwrap(), alternate_bytes);
        assert_eq!(session.selected_branch_id(), "alternate");
        assert_eq!(session.cursor(), 5);
        assert_eq!(session.project().active_branch_id(), "alternate");
        assert!(!session.redo().unwrap());
    }

    #[test]
    fn failed_noop_and_divergent_edits_manage_redo_exactly() {
        let root = crate::test_support::test_directory("tas-editor-session-history-redo").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();

        session
            .edit_transaction(|edit| {
                edit.set_project_comment("first");
                Ok(())
            })
            .unwrap();
        assert!(session.undo().unwrap());
        assert!(session.can_redo());

        assert!(
            session
                .edit_transaction(|edit| edit.set_input_range("missing", 0, 1, Default::default()))
                .is_err()
        );
        assert!(session.can_redo());

        let unchanged_comment = session.project().project_comment().to_owned();
        let outcome = session
            .edit_transaction(|edit| {
                edit.set_project_comment(&unchanged_comment);
                Ok(())
            })
            .unwrap();
        assert!(!outcome.changed);
        assert!(session.can_redo());

        session
            .edit_transaction(|edit| {
                edit.set_project_comment("divergent");
                Ok(())
            })
            .unwrap();
        assert!(!session.can_redo());
        assert_eq!(session.project().project_comment(), "divergent");
    }

    #[test]
    fn divergent_same_generation_never_aliases_manual_or_autosave_witnesses() {
        let root =
            crate::test_support::test_directory("tas-editor-session-history-witness").unwrap();
        let manual_path = root.path().join("movie.ztas");
        crate::tas_project::tests::project()
            .save_atomic(&manual_path)
            .unwrap();
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::open(&manual_path, autosaves, seek_cache).unwrap();

        session
            .edit_transaction(|edit| {
                edit.set_project_comment("saved future");
                Ok(())
            })
            .unwrap();
        session.save_manual().unwrap();
        assert!(session.autosave_if_changed().unwrap().is_some());
        assert_eq!(session.project().edit_generation(), 4);
        assert!(!session.is_dirty());

        assert!(session.undo().unwrap());
        assert_eq!(session.project().edit_generation(), 3);
        assert!(session.is_dirty());
        session
            .edit_transaction(|edit| {
                edit.set_project_comment("different future");
                Ok(())
            })
            .unwrap();
        assert_eq!(session.project().edit_generation(), 4);
        assert!(session.is_dirty());
        assert!(session.autosave_if_changed().unwrap().is_some());
    }

    #[test]
    fn history_evicts_oldest_snapshots_at_its_entry_bound() {
        let root = crate::test_support::test_directory("tas-editor-session-history-bound").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();
        session.history = TasEditorHistory::new(2, MAX_TAS_EDITOR_HISTORY_BYTES);

        for comment in ["one", "two", "three"] {
            session
                .edit_transaction(|edit| {
                    edit.set_project_comment(comment);
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(session.undo_count(), 2);
        assert!(session.undo().unwrap());
        assert_eq!(session.project().project_comment(), "two");
        assert!(session.undo().unwrap());
        assert_eq!(session.project().project_comment(), "one");
        assert!(!session.undo().unwrap());
        assert_eq!(session.redo_count(), 2);
    }
}
