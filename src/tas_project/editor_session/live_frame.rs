use anyhow::{Result, bail};

use super::{
    TasDigest, TasEditOutcome, TasEditorHistoryEntry, TasEditorProjectWitness, TasEditorSession,
    TasProject, project_sha256,
};
use crate::tas_project::TasInputFrame;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TasLiveFrameSourceWitness {
    project: TasEditorProjectWitness,
    selected_branch_id: String,
    cursor: u64,
    history_revision: u128,
    history_group_epoch: u128,
}

#[derive(Clone, Debug)]
pub(super) struct TasLiveFrameHistoryGroup {
    before: TasEditorHistoryEntry,
    changed: bool,
}

#[derive(Debug)]
pub struct TasPreparedLiveFrame {
    source: TasLiveFrameSourceWitness,
    input: TasInputFrame,
    target_cursor: u64,
    candidate_project: TasProject,
    candidate_project_sha256: TasDigest,
    outcome: TasEditOutcome,
    history_before: Option<TasEditorHistoryEntry>,
}

impl TasPreparedLiveFrame {
    pub fn input(&self) -> TasInputFrame {
        self.input
    }

    pub fn branch_id(&self) -> &str {
        &self.source.selected_branch_id
    }

    pub fn cursor(&self) -> u64 {
        self.source.cursor
    }

    pub fn next_cursor(&self) -> u64 {
        self.target_cursor
    }
}

impl TasEditorSession {
    pub fn live_recording_history_group_active(&self) -> bool {
        self.live_frame_history_group.is_some()
    }

    pub fn begin_live_recording_history_group(&mut self) -> Result<()> {
        if self.live_frame_history_group.is_some() {
            bail!("a TAS live recording history group is already active");
        }
        self.live_frame_history_group = Some(TasLiveFrameHistoryGroup {
            before: self.capture_history_entry()?,
            changed: false,
        });
        self.live_frame_history_epoch = self.live_frame_history_epoch.wrapping_add(1);
        Ok(())
    }

    pub fn end_live_recording_history_group(&mut self) -> Result<bool> {
        let group = self
            .live_frame_history_group
            .take()
            .ok_or_else(|| anyhow::anyhow!("no TAS live recording history group is active"))?;
        self.live_frame_history_epoch = self.live_frame_history_epoch.wrapping_add(1);
        if !group.changed {
            return Ok(false);
        }
        self.history.undo.push(group.before);
        self.history.redo.clear();
        self.note_history_mutation();
        Ok(true)
    }

    pub fn prepare_live_frame(&self, input: TasInputFrame) -> Result<TasPreparedLiveFrame> {
        let source = self.live_frame_source_witness();
        let next_cursor = source
            .cursor
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS live-frame cursor overflows"))?;
        let frame_count = self.selected_branch().frame_count();
        if source.cursor > frame_count {
            bail!("TAS live-frame target is past selected branch end");
        }
        let target_cursor = next_cursor.min(frame_count);

        let mut candidate_project = self.project.clone();
        let branch_id = source.selected_branch_id.clone();
        let outcome = candidate_project.edit_transaction(|edit| {
            edit.insert_frames(&branch_id, target_cursor, 1)?;
            edit.set_input_range(&branch_id, target_cursor, 1, input)
        })?;
        let history_before = if outcome.changed && self.live_frame_history_group.is_none() {
            Some(self.capture_history_entry()?)
        } else {
            None
        };
        let candidate_project_sha256 = project_sha256(&candidate_project)?;

        Ok(TasPreparedLiveFrame {
            source,
            input,
            target_cursor,
            candidate_project,
            candidate_project_sha256,
            outcome,
            history_before,
        })
    }

    pub fn commit_prepared_live_frame(
        &mut self,
        prepared: TasPreparedLiveFrame,
    ) -> Result<TasEditOutcome> {
        if prepared.source != self.live_frame_source_witness() {
            bail!("prepared TAS live frame is stale");
        }
        if prepared.outcome.changed {
            self.project = prepared.candidate_project;
            self.project_sha256 = prepared.candidate_project_sha256;
            if let Some(group) = self.live_frame_history_group.as_mut() {
                group.changed = true;
            } else {
                let history_before = prepared
                    .history_before
                    .expect("changed prepared TAS live frame must retain undo history");
                self.history.undo.push(history_before);
            }
            self.history.redo.clear();
            self.note_history_mutation();
        }
        self.cursor = prepared.target_cursor;
        Ok(prepared.outcome)
    }

    fn live_frame_source_witness(&self) -> TasLiveFrameSourceWitness {
        TasLiveFrameSourceWitness {
            project: self.current_project_witness(),
            selected_branch_id: self.selected_branch_id.clone(),
            cursor: self.cursor,
            history_revision: self.history_revision,
            history_group_epoch: self.live_frame_history_epoch,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tas_project::{TasAutosaveConfig, TasAutosaveStore, TasSeekStateCache};

    fn session(root: &Path) -> TasEditorSession {
        let manual_path = root.join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())
                .unwrap();
        let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
        TasEditorSession::new(
            crate::tas_project::tests::project(),
            manual_path,
            autosaves,
            seek_cache,
        )
        .unwrap()
    }

    #[test]
    fn end_cursor_appends_after_mutation_free_preparation() {
        let root = crate::test_support::test_directory("tas-prepared-live-frame-append").unwrap();
        let mut session = session(root.path());
        session
            .set_cursor(session.selected_branch().frame_count())
            .unwrap();
        let before_bytes = session.project().encode().unwrap();
        let before_generation = session.project().edit_generation();
        let before_rerecords = session.project().rerecord_count();

        let prepared = session
            .prepare_live_frame(TasInputFrame::default())
            .unwrap();
        assert_eq!(prepared.cursor(), 12);
        assert_eq!(prepared.next_cursor(), 12);
        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), 12);
        assert_eq!(session.undo_count(), 0);

        let outcome = session.commit_prepared_live_frame(prepared).unwrap();
        assert!(outcome.changed);
        assert_eq!(session.selected_branch().frame_count(), 13);
        assert_eq!(session.cursor(), 12);
        assert_eq!(session.project().edit_generation(), before_generation + 1);
        assert_eq!(session.project().rerecord_count(), before_rerecords + 1);
        assert_eq!(session.undo_count(), 1);
        assert_eq!(session.redo_count(), 0);
    }

    #[test]
    fn equal_input_inserts_a_row_and_shifts_the_existing_future() {
        let root = crate::test_support::test_directory("tas-prepared-live-frame-noop").unwrap();
        let mut session = session(root.path());
        let input = TasInputFrame {
            tilt_x_bits: 1,
            ..TasInputFrame::default()
        };
        session
            .edit_transaction(|edit| edit.set_input_range("main", 1, 1, input))
            .unwrap();
        let future = TasInputFrame {
            tilt_y_bits: 2,
            ..TasInputFrame::default()
        };
        session
            .edit_transaction(|edit| edit.set_input_range("main", 2, 1, future))
            .unwrap();
        session.save_manual().unwrap();
        assert!(!session.is_dirty());
        session.set_cursor(0).unwrap();
        let before_bytes = session.project().encode().unwrap();
        let before_frame_count = session.selected_branch().frame_count();
        let before_generation = session.project().edit_generation();
        let before_rerecords = session.project().rerecord_count();
        let before_undo = session.undo_count();

        let prepared = session.prepare_live_frame(input).unwrap();
        let outcome = session.commit_prepared_live_frame(prepared).unwrap();

        assert!(outcome.changed);
        assert_ne!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(
            session.selected_branch().frame_count(),
            before_frame_count + 1
        );
        assert_eq!(session.selected_branch().input_at(1), input);
        assert_eq!(session.selected_branch().input_at(2), input);
        assert_eq!(session.selected_branch().input_at(3), future);
        assert_eq!(session.project().edit_generation(), before_generation + 1);
        assert_eq!(session.project().rerecord_count(), before_rerecords + 1);
        assert_eq!(session.cursor(), 1);
        assert_eq!(session.undo_count(), before_undo + 1);
        assert_eq!(session.redo_count(), 0);
        assert!(session.is_dirty());
    }

    #[test]
    fn stale_or_invalid_prepared_frames_leave_the_session_exactly_unchanged() {
        let root = crate::test_support::test_directory("tas-prepared-live-frame-stale").unwrap();
        let mut session = session(root.path());
        let prepared = session
            .prepare_live_frame(TasInputFrame::default())
            .unwrap();
        session
            .edit_transaction(|edit| {
                edit.set_project_comment("changed after preparation");
                Ok(())
            })
            .unwrap();
        let before_bytes = session.project().encode().unwrap();
        let before_cursor = session.cursor();
        let before_undo = session.undo_count();
        let before_redo = session.redo_count();

        assert!(session.commit_prepared_live_frame(prepared).is_err());
        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), before_cursor);
        assert_eq!(session.undo_count(), before_undo);
        assert_eq!(session.redo_count(), before_redo);

        session.cursor = u64::MAX;
        let before_bytes = session.project().encode().unwrap();
        let before_undo = session.undo_count();
        assert!(
            session
                .prepare_live_frame(TasInputFrame::default())
                .is_err()
        );
        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.undo_count(), before_undo);
    }

    #[test]
    fn history_mutation_invalidates_an_otherwise_matching_prepared_frame() {
        let root = crate::test_support::test_directory("tas-prepared-live-frame-history").unwrap();
        let mut session = session(root.path());
        let prepared = session
            .prepare_live_frame(TasInputFrame::default())
            .unwrap();
        session
            .edit_transaction(|edit| {
                edit.set_project_comment("temporary history mutation");
                Ok(())
            })
            .unwrap();
        assert!(session.undo().unwrap());
        assert_eq!(session.cursor(), 0);
        assert!(session.commit_prepared_live_frame(prepared).is_err());
    }

    #[test]
    fn live_recording_history_group_restores_the_exact_pre_burst_snapshot() {
        let root = crate::test_support::test_directory("tas-live-frame-history-group").unwrap();
        let mut session = session(root.path());
        let before_bytes = session.project().encode().unwrap();
        let before_cursor = session.cursor();
        let before_revision = session.history_revision;

        assert!(!session.live_recording_history_group_active());
        session.begin_live_recording_history_group().unwrap();
        assert!(session.live_recording_history_group_active());
        assert!(session.begin_live_recording_history_group().is_err());
        for tilt_x_bits in [1, 2, 3] {
            let prepared = session
                .prepare_live_frame(TasInputFrame {
                    tilt_x_bits,
                    ..TasInputFrame::default()
                })
                .unwrap();
            assert!(
                session
                    .commit_prepared_live_frame(prepared)
                    .unwrap()
                    .changed
            );
        }
        assert_eq!(session.cursor(), 3);
        assert_eq!(session.undo_count(), 0);
        assert_eq!(session.history_revision, before_revision + 3);

        assert!(session.end_live_recording_history_group().unwrap());
        assert!(!session.live_recording_history_group_active());
        assert_eq!(session.undo_count(), 1);
        assert_eq!(session.redo_count(), 0);
        assert_eq!(session.history_revision, before_revision + 4);
        assert!(session.undo().unwrap());
        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), before_cursor);
    }

    #[test]
    fn empty_live_recording_history_groups_do_not_create_history() {
        let root =
            crate::test_support::test_directory("tas-live-frame-history-group-empty").unwrap();
        let mut session = session(root.path());
        let before_bytes = session.project().encode().unwrap();
        let before_cursor = session.cursor();
        let before_revision = session.history_revision;

        session.begin_live_recording_history_group().unwrap();
        assert!(!session.end_live_recording_history_group().unwrap());
        assert_eq!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), before_cursor);
        assert_eq!(session.undo_count(), 0);
        assert_eq!(session.redo_count(), 0);
        assert_eq!(session.history_revision, before_revision);

        let input = session.selected_branch().input_at(1);
        session.begin_live_recording_history_group().unwrap();
        let prepared = session.prepare_live_frame(input).unwrap();
        assert!(
            session
                .commit_prepared_live_frame(prepared)
                .unwrap()
                .changed
        );
        assert!(session.end_live_recording_history_group().unwrap());
        assert_ne!(session.project().encode().unwrap(), before_bytes);
        assert_eq!(session.cursor(), 1);
        assert_eq!(session.undo_count(), 1);
        assert_eq!(session.redo_count(), 0);
        assert_eq!(session.history_revision, before_revision + 2);
    }

    #[test]
    fn live_recording_history_group_boundaries_invalidate_prepared_frames() {
        let root =
            crate::test_support::test_directory("tas-live-frame-history-group-stale").unwrap();
        let mut session = session(root.path());
        let prepared_before_group = session
            .prepare_live_frame(TasInputFrame::default())
            .unwrap();

        session.begin_live_recording_history_group().unwrap();
        assert!(
            session
                .commit_prepared_live_frame(prepared_before_group)
                .is_err()
        );

        let prepared_before_end = session
            .prepare_live_frame(TasInputFrame::default())
            .unwrap();
        assert!(!session.end_live_recording_history_group().unwrap());
        assert!(
            session
                .commit_prepared_live_frame(prepared_before_end)
                .is_err()
        );
    }
}
