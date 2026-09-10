use super::*;
use crate::tas_project::{TasAutosaveConfig, TasInputFrame};

fn stores(root: &Path, manual_path: &Path) -> (TasAutosaveStore, TasSeekStateCache) {
    let autosaves =
        TasAutosaveStore::beside_manual_save(manual_path, TasAutosaveConfig::default()).unwrap();
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
fn corrupt_undo_and_redo_history_fail_atomically() {
    let root = crate::test_support::test_directory("tas-editor-session-history-corrupt").unwrap();

    let undo_path = root.path().join("undo.ztas");
    let (autosaves, seek_cache) = stores(root.path(), &undo_path);
    let mut undo_session = TasEditorSession::new(
        crate::tas_project::tests::project(),
        &undo_path,
        autosaves,
        seek_cache,
    )
    .unwrap();
    undo_session
        .edit_transaction(|edit| {
            edit.set_project_comment("undo target");
            Ok(())
        })
        .unwrap();
    assert_corrupt_history_restore_fails_atomically(&mut undo_session, true);

    let redo_path = root.path().join("redo.ztas");
    let (autosaves, seek_cache) = stores(root.path(), &redo_path);
    let mut redo_session = TasEditorSession::new(
        crate::tas_project::tests::project(),
        &redo_path,
        autosaves,
        seek_cache,
    )
    .unwrap();
    redo_session
        .edit_transaction(|edit| {
            edit.set_project_comment("redo target");
            Ok(())
        })
        .unwrap();
    assert!(redo_session.undo().unwrap());
    assert_corrupt_history_restore_fails_atomically(&mut redo_session, false);
}

fn assert_corrupt_history_restore_fails_atomically(session: &mut TasEditorSession, undo: bool) {
    let corrupted_entry = {
        let entries = if undo {
            &mut session.history.undo.entries
        } else {
            &mut session.history.redo.entries
        };
        let entry = entries.back_mut().expect("history entry should exist");
        entry.project_bytes = entry.project_bytes[..8].into();
        entry.project_bytes.clone()
    };
    let package = session.project().encode().unwrap();
    let content_sha256 = session.project_content_sha256();
    let selected_branch_id = session.selected_branch_id().to_owned();
    let cursor = session.cursor();
    let undo_count = session.undo_count();
    let redo_count = session.redo_count();
    let history_revision = session.history_revision;

    let result = if undo { session.undo() } else { session.redo() };
    assert!(result.is_err());
    assert_eq!(session.project().encode().unwrap(), package);
    assert_eq!(session.project_content_sha256(), content_sha256);
    assert_eq!(session.selected_branch_id(), selected_branch_id);
    assert_eq!(session.cursor(), cursor);
    assert_eq!(session.undo_count(), undo_count);
    assert_eq!(session.redo_count(), redo_count);
    assert_eq!(session.history_revision, history_revision);
    let retained_entry = if undo {
        session.history.undo.entries.back()
    } else {
        session.history.redo.entries.back()
    }
    .expect("failed restore should retain its history entry");
    assert_eq!(retained_entry.project_bytes, corrupted_entry);
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
    let root = crate::test_support::test_directory("tas-editor-session-history-witness").unwrap();
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

#[test]
fn recording_checkpoint_restores_history_evicted_by_the_byte_budget() {
    let root = crate::test_support::test_directory("tas-recording-history-byte-budget").unwrap();
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
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    tilt_x_bits: 1,
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let snapshot_bytes =
        session.project().encode().unwrap().len() + session.selected_branch_id().len();
    let byte_budget = snapshot_bytes.saturating_mul(3) / 2;
    assert!(snapshot_bytes <= byte_budget);
    assert!(byte_budget < snapshot_bytes.saturating_mul(2));
    session.history = TasEditorHistory::new(32, byte_budget);
    session
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    tilt_x_bits: 2,
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let before_draft = session.project().encode().unwrap();
    let checkpoint = session
        .begin_recording_draft("main", session.selected_branch().frame_count())
        .unwrap();
    assert_eq!(session.undo_count(), 1);
    session
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                12,
                1,
                TasInputFrame {
                    tilt_x_bits: 3,
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    assert_eq!(session.undo_count(), 1);

    session.discard_recording_draft(&checkpoint).unwrap();
    assert_eq!(session.project().encode().unwrap(), before_draft);
    assert_eq!(session.undo_count(), 1);
    assert!(!session.can_redo());
    assert!(session.undo().unwrap());
    assert_eq!(session.selected_branch().input_at(0).tilt_x_bits, 1);
}

#[test]
fn recording_checkpoint_failure_and_restore_leave_prepared_frames_stale() {
    let root = crate::test_support::test_directory("tas-recording-checkpoint-atomic").unwrap();
    let manual_path = root.path().join("movie.ztas");
    let (autosaves, seek_cache) = stores(root.path(), &manual_path);
    let mut session = TasEditorSession::new(
        crate::tas_project::tests::project(),
        &manual_path,
        autosaves,
        seek_cache,
    )
    .unwrap();
    let prepared = session
        .prepare_live_frame(TasInputFrame::default())
        .unwrap();
    let checkpoint = session
        .begin_recording_draft("main", session.selected_branch().frame_count())
        .unwrap();
    let draft = session.project().encode().unwrap();
    let draft_cursor = session.cursor();
    session.history = TasEditorHistory::new(32, 1);

    assert!(
        session
            .begin_recording_draft("main", session.selected_branch().frame_count())
            .is_err()
    );
    assert_eq!(session.project().encode().unwrap(), draft);
    assert_eq!(session.cursor(), draft_cursor);
    assert_eq!(session.undo_count(), 0);

    session.discard_recording_draft(&checkpoint).unwrap();
    assert!(session.commit_prepared_live_frame(prepared).is_err());
}
