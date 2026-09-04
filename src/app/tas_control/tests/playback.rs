use std::path::Path;

use super::*;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasEditorSession, TasInputFrame,
    TasSeekStateCache,
};

fn playback_session(root: &Path) -> TasEditorSession {
    let source_path = root.join("game.nes");
    std::fs::write(&source_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut project = DirectNesTasExecutionLoader::new(source_path, Vec::new())
        .create_project()
        .unwrap();
    project
        .edit_transaction(|edit| {
            edit.insert_frames("main", 1, 2)?;
            edit.set_input_range(
                "main",
                1,
                1,
                TasInputFrame {
                    players: [
                        crate::tas_project::TasControllerInput {
                            buttons: 0x03,
                            dpad: 0x04,
                        },
                        crate::tas_project::TasControllerInput::default(),
                        crate::tas_project::TasControllerInput::default(),
                        crate::tas_project::TasControllerInput::default(),
                        crate::tas_project::TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let manual_path = root.join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
    let mut session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    session.set_cursor(1).unwrap();
    session
}

fn linked_at(session: &TasEditorSession, cursor: u64) -> TasControlCoordinator {
    let project = TasEditorControlSnapshot::capture_at(session, cursor).unwrap();
    let mut coordinator = TasControlCoordinator::new();
    coordinator.state = TasControlState::AwaitingDecision {
        worker_generation: WORKER_GENERATION,
        lease_id: 41,
        run_id: 7,
        next_advance_id: 1,
        proof: TasControlHeldProof {
            frame_count: 11,
            current_state_sha256: TasDigest([0x11; 32]),
        },
        project,
        candidate_segment_id: 1,
        candidate_segment_frame_count: cursor,
        candidate_executed_project_frames: cursor,
        candidate_frame_count: 20,
        candidate_state_sha256: TasDigest([0x31; 32]),
    };
    coordinator
}

fn advanced(executed_project_frames: u64, advance_id: u64) -> EmuResponse {
    EmuResponse::TasFrameAdvanced {
        profile: TasExecutionProfile::DirectNesCartridge,
        lease_id: 41,
        run_id: 7,
        advance_id,
        segment_id: 1,
        segment_frame_count: executed_project_frames,
        executed_project_frames,
        frame_count: 20 + advance_id,
        state_sha256: TasDigest([0x40 + advance_id as u8; 32]),
        rumble: true,
        audio_samples: vec![0.25, -0.25],
        ui_data: None,
    }
}

#[test]
fn stored_input_playback_is_non_mutating_and_pause_settles_once() {
    let directory = crate::test_support::test_directory("tas-linked-playback-pause").unwrap();
    let session = playback_session(directory.path());
    let before = session.project().encode().unwrap();
    let rerecords = session.project().rerecord_count();
    let mut coordinator = linked_at(&session, 1);

    coordinator.start_playback(&session).unwrap();
    let command = coordinator.begin_playback_frame(&session).unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::AdvanceTasControl(request)))
            if request.lease_id == 41
                && request.run_id == 7
                && request.advance_id == 1
                && request.expected_executed_project_frames == 1
                && request.input.p1_buttons == 0x03
                && request.input.p1_dpad == 0x04
    ));
    coordinator.pause_playback();
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Playing {
            cursor: 1,
            pause_pending: true,
        }
    );

    let current = TasEditorControlSnapshot::capture_at(&session, 2).unwrap();
    let response =
        coordinator.consume_response(WORKER_GENERATION, advanced(2, 1), None, Some(&current));
    assert!(matches!(
        response,
        ResponseDisposition::PresentPlaybackFrame {
            rumble: true,
            audio_samples,
            ..
        } if audio_samples == vec![0.25, -0.25]
    ));
    assert_eq!(coordinator.linked_cursor(), Some(2));
    assert!(!coordinator.playback_active());
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project().rerecord_count(), rerecords);
}

#[test]
fn playback_consumes_neutral_row_then_stops_at_end_boundary() {
    let directory = crate::test_support::test_directory("tas-linked-playback-end").unwrap();
    let session = playback_session(directory.path());
    let mut coordinator = linked_at(&session, 2);

    coordinator.start_playback(&session).unwrap();
    let command = coordinator.begin_playback_frame(&session).unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::AdvanceTasControl(request)))
            if request.input == Default::default()
    ));
    let current = TasEditorControlSnapshot::capture_at(&session, 3).unwrap();
    assert!(matches!(
        coordinator.consume_response(WORKER_GENERATION, advanced(3, 1), None, Some(&current),),
        ResponseDisposition::PresentPlaybackFrame { .. }
    ));
    assert_eq!(coordinator.linked_cursor(), Some(3));
    assert!(!coordinator.playback_active());
    assert!(coordinator.start_playback(&session).is_err());
}

#[test]
fn accepted_playback_frame_delivers_its_snapshot_once() {
    let directory = crate::test_support::test_directory("tas-playback-snapshot").unwrap();
    let session = playback_session(directory.path());
    let mut coordinator = linked_at(&session, 1);
    coordinator.start_playback(&session).unwrap();
    coordinator.begin_playback_frame(&session).unwrap();
    let current = TasEditorControlSnapshot::capture_at(&session, 2).unwrap();
    let mut response = advanced(2, 1);
    let EmuResponse::TasFrameAdvanced { ui_data, .. } = &mut response else {
        unreachable!();
    };
    *ui_data = Some(Box::default());

    match coordinator.consume_response(WORKER_GENERATION, response, None, Some(&current)) {
        ResponseDisposition::PresentPlaybackFrame {
            ui_data: Some(_), ..
        } => {}
        _ => panic!("accepted playback must deliver its snapshot"),
    }
}

#[test]
fn stale_project_during_playback_rolls_back_original_lease_checkpoint() {
    let directory = crate::test_support::test_directory("tas-linked-playback-stale").unwrap();
    let session = playback_session(directory.path());
    let mut coordinator = linked_at(&session, 1);
    coordinator.start_playback(&session).unwrap();
    coordinator.begin_playback_frame(&session).unwrap();
    let stale = TasEditorControlSnapshot {
        project_content_sha256: TasDigest([0xEE; 32]),
        ..TasEditorControlSnapshot::capture_at(&session, 2).unwrap()
    };

    let mut advanced = advanced(2, 1);
    let EmuResponse::TasFrameAdvanced { ui_data, .. } = &mut advanced else {
        unreachable!();
    };
    *ui_data = Some(Box::default());
    let response = coordinator.consume_response(WORKER_GENERATION, advanced, None, Some(&stale));
    bound_rollback(response, WORKER_GENERATION, 41);
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending {
            checkpoint_frame_count: 11,
            checkpoint_sha256,
            ..
        } if checkpoint_sha256 == TasDigest([0x11; 32])
    ));
}
