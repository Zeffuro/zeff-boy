use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectFdsTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

static FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xEA; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

#[test]
fn linked_app_records_two_fds_pads_for_five_side_media_without_inventing_a_disk_event() {
    let root = crate::test_support::test_directory("tas-fds-live-record-roundtrip").unwrap();
    let disk_path = root.path().join("game.fds");
    std::fs::write(&disk_path, disk(5)).unwrap();
    let project = DirectFdsTasExecutionLoader::new_with_bios_override(disk_path.clone(), &FDS_BIOS)
        .create_project()
        .unwrap();
    let loader =
        DirectFdsTasExecutionLoader::new_for_project(disk_path.clone(), Vec::new(), &project)
            .unwrap()
            .with_project_bios_override(&FDS_BIOS);
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 93, ActiveSystem::Nes, disk_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(93, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
        LiveCommand::Button {
            player: 2,
            key: HostButton::Right,
            pressed: true,
        },
        LiveCommand::Button {
            player: 2,
            key: HostButton::B,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    let started = live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    assert_eq!(started["live"]["state"], "recording");
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );

    wait_for_recorded_frame(&mut app);
    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(
        input.players,
        [
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x02,
            },
            TasControllerInput {
                buttons: 0x02,
                dpad: 0x01,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ]
    );
    assert_eq!(input.zapper, crate::tas_project::TasZapperInput::default());

    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.set_input_p2(input.players[1].buttons, input.players[1].dpad);
    expected.step_frame();
    let expected_state = TasDigest::from_bytes(&expected.encode_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));
    assert!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .selected_branch()
            .events()
            .is_empty()
    );
}

fn disk(sides: usize) -> Vec<u8> {
    (0..sides)
        .flat_map(|side| {
            (0..zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE)
                .map(move |index| (side as u8).wrapping_mul(0x51).wrapping_add(index as u8))
        })
        .collect()
}
