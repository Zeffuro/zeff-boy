use anyhow::{Result, bail};

use super::{TasEditOutcome, TasEditorSession};
use crate::tas_project::TasInputPattern;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TasFrameRange {
    start: u64,
    end: u64,
}

impl TasFrameRange {
    pub fn new(start: u64, end: u64) -> Result<Self> {
        if start >= end {
            bail!("TAS frame range must be non-empty and ordered");
        }
        Ok(Self { start, end })
    }

    pub fn start(self) -> u64 {
        self.start
    }

    pub fn end(self) -> u64 {
        self.end
    }

    pub fn length(self) -> u64 {
        self.end - self.start
    }
}

impl TasEditorSession {
    pub fn input_pattern_in_range(&self, range: TasFrameRange) -> Result<TasInputPattern> {
        self.validate_frame_range(range)?;
        self.selected_branch()
            .input_pattern(range.start(), range.length())
    }

    pub fn delete_frame_range(&mut self, range: TasFrameRange) -> Result<TasEditOutcome> {
        self.validate_frame_range(range)?;
        let before = self.capture_history_entry()?;
        let branch_id = self.selected_branch_id.clone();
        let outcome = self.project.edit_transaction(|edit| {
            edit.delete_frames(&branch_id, range.start(), range.length())
        })?;
        self.cursor =
            rebase_cursor_after_deletion(self.cursor, range.start(), range.end(), range.length())?;
        self.finish_edit(&outcome, before)?;
        Ok(outcome)
    }

    pub fn insert_neutral_frames(&mut self, cursor: u64, count: u64) -> Result<TasEditOutcome> {
        self.validate_insertion(cursor, count)?;
        let before = self.capture_history_entry()?;
        let branch_id = self.selected_branch_id.clone();
        let outcome = self
            .project
            .edit_transaction(|edit| edit.insert_frames(&branch_id, cursor, count))?;
        self.finish_edit(&outcome, before)?;
        Ok(outcome)
    }

    pub fn insert_input_pattern(
        &mut self,
        cursor: u64,
        pattern: &TasInputPattern,
    ) -> Result<TasEditOutcome> {
        self.validate_insertion(cursor, pattern.length())?;
        let before = self.capture_history_entry()?;
        let branch_id = self.selected_branch_id.clone();
        let outcome = self.project.edit_transaction(|edit| {
            edit.insert_frames(&branch_id, cursor, pattern.length())?;
            edit.replace_input_pattern(&branch_id, cursor, pattern)
        })?;
        self.finish_edit(&outcome, before)?;
        Ok(outcome)
    }

    fn validate_frame_range(&self, range: TasFrameRange) -> Result<()> {
        if range.end() > self.selected_branch().frame_count() {
            bail!("TAS frame range extends past selected branch end");
        }
        Ok(())
    }

    fn validate_insertion(&self, cursor: u64, count: u64) -> Result<()> {
        if count == 0 {
            bail!("TAS frame insertion cannot be empty");
        }
        if cursor > self.selected_branch().frame_count() {
            bail!("TAS frame insertion cursor is past selected branch end");
        }
        Ok(())
    }
}

fn rebase_cursor_after_deletion(cursor: u64, start: u64, end: u64, count: u64) -> Result<u64> {
    if cursor <= start {
        Ok(cursor)
    } else if cursor < end {
        Ok(start)
    } else {
        cursor
            .checked_sub(count)
            .ok_or_else(|| anyhow::anyhow!("TAS editor cursor deletion underflows"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tas_project::{TasAutosaveConfig, TasAutosaveStore, TasSeekStateCache};

    fn stores(root: &Path, manual_path: &Path) -> (TasAutosaveStore, TasSeekStateCache) {
        let autosaves =
            TasAutosaveStore::beside_manual_save(manual_path, TasAutosaveConfig::default())
                .unwrap();
        let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
        (autosaves, seek_cache)
    }

    #[test]
    fn half_open_frame_ranges_copy_delete_and_reinsert_input_transactionally() {
        let root = crate::test_support::test_directory("tas-editor-session-frame-range").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut project = crate::tas_project::tests::project();
        project.branches[0].events.clear();
        project.markers.clear();
        project.annotations.clear();
        project.validate().unwrap();
        let mut session =
            TasEditorSession::new(project, &manual_path, autosaves, seek_cache).unwrap();
        let range = TasFrameRange::new(2, 4).unwrap();
        let original_inputs = (0..session.selected_branch().frame_count())
            .map(|frame| session.selected_branch().input_at(frame))
            .collect::<Vec<_>>();
        let original_bytes = session.project().encode().unwrap();
        let copied = session.input_pattern_in_range(range).unwrap();

        session.set_cursor(3).unwrap();
        let deleted = session.delete_frame_range(range).unwrap();
        assert!(deleted.changed);
        assert_eq!(session.selected_branch().frame_count(), 10);
        assert_eq!(session.cursor(), 2);
        assert_eq!(session.undo_count(), 1);

        let reinserted = session.insert_input_pattern(2, &copied).unwrap();
        assert!(reinserted.changed);
        assert_eq!(session.selected_branch().frame_count(), 12);
        assert_eq!(
            (0..session.selected_branch().frame_count())
                .map(|frame| session.selected_branch().input_at(frame))
                .collect::<Vec<_>>(),
            original_inputs
        );
        assert_eq!(session.cursor(), 2);
        assert_eq!(session.undo_count(), 2);

        assert!(session.undo().unwrap());
        assert_eq!(session.selected_branch().frame_count(), 10);
        assert!(session.undo().unwrap());
        assert_eq!(session.project().encode().unwrap(), original_bytes);
        assert_eq!(session.cursor(), 3);
    }

    #[test]
    fn frame_range_operations_reject_invalid_bounds_without_mutating_session() {
        let root =
            crate::test_support::test_directory("tas-editor-session-frame-range-errors").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();
        session.set_cursor(7).unwrap();
        let before_bytes = session.project().encode().unwrap();

        assert!(TasFrameRange::new(4, 4).is_err());
        assert!(TasFrameRange::new(5, 4).is_err());
        assert!(
            session
                .delete_frame_range(TasFrameRange::new(11, 13).unwrap())
                .is_err()
        );
        assert!(session.insert_neutral_frames(13, 1).is_err());
        let copied = session
            .input_pattern_in_range(TasFrameRange::new(2, 4).unwrap())
            .unwrap();
        assert!(session.insert_input_pattern(13, &copied).is_err());

        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), 7);
        assert_eq!(session.undo_count(), 0);
        assert_eq!(session.redo_count(), 0);
    }

    #[test]
    fn neutral_range_insertion_keeps_the_current_boundary_and_is_undoable() {
        let root = crate::test_support::test_directory("tas-editor-session-neutral-range").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let (autosaves, seek_cache) = stores(root.path(), &manual_path);
        let mut session = TasEditorSession::new(
            crate::tas_project::tests::project(),
            &manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap();
        let before = session.project().encode().unwrap();
        session.set_cursor(4).unwrap();

        session.insert_neutral_frames(4, 3).unwrap();
        assert_eq!(session.selected_branch().frame_count(), 15);
        assert_eq!(session.cursor(), 4);
        assert_eq!(
            session.selected_branch().input_at(4),
            crate::tas_project::TasInputFrame::default()
        );
        assert_eq!(
            session.selected_branch().input_at(6),
            crate::tas_project::TasInputFrame::default()
        );
        assert!(session.undo().unwrap());
        assert_eq!(session.project().encode().unwrap(), before);
        assert_eq!(session.cursor(), 4);
    }
}
