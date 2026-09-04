use std::time::Duration;

use super::super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::super::*;
use super::pce_rom;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::{DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::platform::Instant;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

mod arcade_multitap;
mod memory_base_multitap;
mod ppf;

#[test]
fn linked_app_records_pc_engine_multitap_input_and_restores() {
    let root =
        crate::test_support::test_directory("tas-pce-multitap-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.pce");
    std::fs::write(&rom_path, pce_rom()).unwrap();
    let loader = DirectPceTasExecutionLoader::new_multitap(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 91, ActiveSystem::Pce, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(91, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            project: TasEditorControlSnapshot {
                profile: crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard,
                ..
            },
            ..
        }
    ));

    for (player, key) in [
        (1, HostButton::Left),
        (1, HostButton::A),
        (2, HostButton::Right),
        (2, HostButton::B),
        (3, HostButton::Up),
        (3, HostButton::Select),
        (4, HostButton::Down),
        (4, HostButton::Start),
        (5, HostButton::Down),
        (5, HostButton::A),
    ] {
        live_ok(
            &mut app,
            LiveCommand::Button {
                player,
                key,
                pressed: true,
            },
        );
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
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
            TasControllerInput {
                buttons: 0x04,
                dpad: 0x04,
            },
            TasControllerInput {
                buttons: 0x08,
                dpad: 0x08,
            },
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x08,
            },
        ]
    );

    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.set_input_p2(input.players[1].buttons, input.players[1].dpad);
    expected.set_input_p3(input.players[2].buttons, input.players[2].dpad);
    expected.set_input_p4(input.players[3].buttons, input.players[3].dpad);
    expected.set_input_p5(input.players[4].buttons, input.players[4].dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let expected_state = TasDigest::from_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));

    let reply = live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    assert_eq!(reply["live"]["state"], "returning");
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}

#[test]
fn linked_app_records_pc_engine_cd_multitap_input_and_supports_restore_and_keep() {
    for (media, keep) in [
        ("cue", false),
        ("cue", true),
        ("chd", false),
        ("chd", true),
        ("iso", false),
        ("iso", true),
    ] {
        let root = crate::test_support::test_directory(match (media, keep) {
            ("cue", false) => "tas-pce-cd-multitap-live-restore",
            ("cue", true) => "tas-pce-cd-multitap-live-keep",
            ("chd", false) => "tas-pce-cd-chd-multitap-live-restore",
            ("chd", true) => "tas-pce-cd-chd-multitap-live-keep",
            ("iso", false) => "tas-pce-cd-iso-multitap-live-restore",
            ("iso", true) => "tas-pce-cd-iso-multitap-live-keep",
            _ => unreachable!(),
        })
        .unwrap();
        let source_path = match media {
            "chd" => {
                let path = root.path().join("disc.chd");
                crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&path).unwrap();
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[4 * 2_448] ^= if keep { 0x32 } else { 0x31 };
                std::fs::write(&path, bytes).unwrap();
                path
            }
            "iso" => {
                let path = root.path().join("disc.iso");
                std::fs::write(
                    &path,
                    vec![
                        if keep { 0xBE } else { 0xBD };
                        4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES
                    ],
                )
                .unwrap();
                std::fs::write(
                    root.path().join("disc.cue"),
                    b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                )
                .unwrap();
                path
            }
            _ => {
                let path = root.path().join("disc.cue");
                std::fs::write(
                    root.path().join("disc.bin"),
                    vec![
                        if keep { 0xBA } else { 0xB9 };
                        4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES
                    ],
                )
                .unwrap();
                std::fs::write(
                    &path,
                    b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                )
                .unwrap();
                path
            }
        };
        let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
        let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
        let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            source_path.clone(),
            system_card,
            firmware_sha256,
        );
        let disc_sha256 = base
            .load_fresh_backend()
            .unwrap()
            .pce()
            .unwrap()
            .normalized_disc_hash()
            .unwrap();
        let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        );
        let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            source_path.clone(),
            system_card,
            firmware_sha256,
        );
        let project = loader.create_project().unwrap();
        let backend = loader.load_editor_engine(&project).unwrap().into_backend();
        let manual_path = root.path().join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())
                .unwrap();
        let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
        let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
        let worker = EmuThread::spawn(backend, false);
        let mut app = app_with_worker(
            worker,
            if keep { 121 } else { 120 },
            ActiveSystem::Pce,
            source_path,
        );
        app.debug_windows
            .tas_editor
            .install_verified_export_session(session);
        let snapshot = TasEditorControlSnapshot::capture(
            app.debug_windows.tas_editor.active_session().unwrap(),
        )
        .unwrap();
        app.tas_control
            .queue_acquire(
                if keep { 121 } else { 120 },
                snapshot,
                TasControlStartMode::Preview,
            )
            .unwrap();
        wait_for_linked(&mut app);

        for (player, key) in [
            (1, HostButton::A),
            (2, HostButton::B),
            (3, HostButton::Select),
            (4, HostButton::Start),
            (5, HostButton::Left),
        ] {
            live_ok(
                &mut app,
                LiveCommand::Button {
                    player,
                    key,
                    pressed: true,
                },
            );
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
            input.players.map(|player| (player.buttons, player.dpad)),
            [(1, 0), (2, 0), (4, 0), (8, 0), (0, 2)]
        );
        live_ok(&mut app, LiveCommand::TasDisconnect { keep });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
            app.drain_emu_responses();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.tas_control.state, TasControlState::Detached);
    }
}

#[test]
fn direct_pce_cd_multitap_repair_reload_acquires_and_restores() {
    for media in ["cue", "chd", "iso"] {
        let root = crate::test_support::test_directory(match media {
            "cue" => "tas-pce-cd-multitap-repair",
            "chd" => "tas-pce-cd-chd-multitap-repair",
            "iso" => "tas-pce-cd-iso-multitap-repair",
            _ => unreachable!(),
        })
        .unwrap();
        let source_path = match media {
            "chd" => {
                let path = root.path().join("disc.chd");
                crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&path).unwrap();
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[4 * 2_448] ^= 0x33;
                std::fs::write(&path, bytes).unwrap();
                path
            }
            "iso" => {
                let path = root.path().join("disc.iso");
                std::fs::write(
                    &path,
                    vec![0xBF; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
                )
                .unwrap();
                std::fs::write(
                    root.path().join("disc.cue"),
                    b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                )
                .unwrap();
                path
            }
            _ => {
                let path = root.path().join("disc.cue");
                std::fs::write(
                    root.path().join("disc.bin"),
                    vec![0xBB; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
                )
                .unwrap();
                std::fs::write(
                    &path,
                    b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                )
                .unwrap();
                path
            }
        };
        let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
        let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
        let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            source_path.clone(),
            system_card,
            firmware_sha256,
        );
        let disc_sha256 = base
            .load_fresh_backend()
            .unwrap()
            .pce()
            .unwrap()
            .normalized_disc_hash()
            .unwrap();
        let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        );
        let _firmware = crate::emu_backend::loader::register_test_pce_cd_system_card(
            firmware_sha256,
            system_card,
        );
        let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            source_path.clone(),
            system_card,
            firmware_sha256,
        );
        let project = loader.create_project().unwrap();
        let mut original = loader.load_editor_engine(&project).unwrap().into_backend();
        original.set_input(1, 0);
        original.set_input_p2(2, 0);
        original.set_input_p3(4, 0);
        original.set_input_p4(8, 0);
        original.set_input_p5(0, 2);
        original.step_frame();
        let manual_path = root.path().join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())
                .unwrap();
        let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
        let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
        let worker = EmuThread::spawn(original, false);
        let mut app = app_with_worker(worker, 122, ActiveSystem::Pce, source_path);
        app.debug_windows
            .tas_editor
            .install_verified_export_session(session);

        let reply = live_ok(&mut app, LiveCommand::TasReloadGame);
        assert_eq!(reply["repair_activated"], true);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision { .. }
        ) && Instant::now() < deadline
        {
            app.drain_emu_responses();
            app.begin_queued_tas_control_acquire();
            let _ = live_ok(&mut app, LiveCommand::TasStatus);
            std::thread::yield_now();
        }
        assert!(matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision { .. }
        ));
        assert_eq!(app.emu_worker_generation, 123);

        live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
        let deadline = Instant::now() + Duration::from_secs(5);
        while (app.tas_control.state != TasControlState::Detached
            || app.tas_repair_state() != crate::app::tas_control::repair::TasRepairState::Detached)
            && Instant::now() < deadline
        {
            app.drain_emu_responses();
            app.pump_tas_repair_resolution();
            std::thread::yield_now();
        }
        assert_eq!(app.tas_control.state, TasControlState::Detached);
        assert_eq!(
            app.tas_repair_state(),
            crate::app::tas_control::repair::TasRepairState::Detached
        );
        assert_eq!(app.emu_worker_generation, 124);
    }
}
