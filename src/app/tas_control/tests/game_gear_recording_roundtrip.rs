use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectGameGearTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasSeekStateCache,
};
use zeff_sega8_core::hardware::cartridge::{GameGearCartridgeIdentity, GameGearStandardMapperRam};

#[test]
fn linked_app_records_game_gear_pad_and_start() {
    let root = crate::test_support::test_directory("tas-game-gear-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.gg");
    let rom = game_gear_rom();
    std::fs::write(&rom_path, &rom).unwrap();
    let loader = DirectGameGearTasExecutionLoader::new_with_catalog_entry(
        rom_path.clone(),
        GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&rom),
            source_len: rom.len(),
        },
        GameGearStandardMapperRam::Absent,
    );
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 75, ActiveSystem::GameGear, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(75, snapshot, TasControlStartMode::Preview)
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
            player: 1,
            key: HostButton::Start,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
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
                buttons: 0x09,
                dpad: 0x02,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ]
    );
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            ..
        }
    ));
}

fn game_gear_rom() -> Vec<u8> {
    let mut rom = vec![0x00; 16 * 1024];
    let offset = 0x3FF0;
    rom[offset..offset + 8].copy_from_slice(b"TMR SEGA");
    rom[offset + 0x0A..offset + 0x0C].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 0x0C] = 0x42;
    rom[offset + 0x0D] = 0x31;
    rom[offset + 0x0E] = 0xA5;
    rom[offset + 0x0F] = 0x6A;
    rom
}
