use std::path::{Path, PathBuf};

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkState, ReplayJoypadFrame, ReplayMetadata,
    ReplayPlayer, ReplayRecorder, ReplayWonderSwanLinkEvent,
};
use zeff_firmware::sha256_hex;
use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::HeadlessOptions;
#[cfg(not(target_arch = "wasm32"))]
use super::endpoint::run_loaded_replay_for_verification;
use super::endpoint::run_loaded_replay_headless;
use super::endpoint::validate_game_boy_link_replay_result_for_test;
#[cfg(not(target_arch = "wasm32"))]
use super::paired_live::ensure_replay_metadata_has_expected_gb_link_events;
#[cfg(not(target_arch = "wasm32"))]
use super::timeline::{PairedGameBoyReplayTimeline, paired_game_boy_replay_timeline};
use crate::test_support::{build_nes_test_rom, test_directory};

static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

fn build_fds_test_image() -> Vec<u8> {
    let mut side_a = vec![0xA1; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
    let mut side_b = vec![0xB2; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
    side_a[0] = 0x01;
    side_b[0] = 0x02;
    side_a.extend_from_slice(&side_b);
    side_a
}

fn load_fds_test_backend(rom_path: &Path) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        rom_path,
        rom_path,
        Some(build_fds_test_image()),
        BackendLoadConfig {
            fds_bios_override: Some(&TEST_FDS_BIOS),
            ..BackendLoadConfig::default()
        },
    )?
    .backend)
}

fn load_nes_test_backend(rom_path: &Path, rom_data: Vec<u8>) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        rom_path,
        rom_path,
        Some(rom_data),
        BackendLoadConfig::default(),
    )?
    .backend)
}

fn build_pocket_camera_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x134..0x143].copy_from_slice(b"POCKET CAM TEST");
    rom[0x143] = 0x00;
    rom[0x147] = 0xFC;
    rom[0x148] = 0x00;
    rom[0x149] = 0x04;
    rom[0x14A] = 0x01;
    rom[0x14B] = 0x33;
    rom[0x14C] = 0x00;
    let mut checksum = 0u8;
    for byte in &rom[0x134..=0x14C] {
        checksum = checksum.wrapping_sub(*byte).wrapping_sub(1);
    }
    rom[0x14D] = checksum;
    rom
}

fn load_pocket_camera_test_backend(rom_path: &Path) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        rom_path,
        rom_path,
        Some(build_pocket_camera_test_rom()),
        BackendLoadConfig::default(),
    )?
    .backend)
}

fn wonder_swan_test_backend() -> EmuBackend {
    let mut rom = vec![0x90; 0x10000];
    rom[0] = 0x90;
    rom[1] = 0xEB;
    rom[2] = 0xFC;
    let reset_vector = rom.len() - 16;
    rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer] = 0x01;
    rom[footer + 2] = 0x23;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    let ws = zeff_ws_core::emulator::Emulator::from_rom_data(&rom)
        .expect("minimal WonderSwan ROM should initialize");
    EmuBackend::from_ws(ws, PathBuf::from("test.ws"))
}

fn arm_pocket_camera_capture(backend: &mut EmuBackend) {
    let EmuBackend::Gb(gb) = backend else {
        panic!("expected GB backend");
    };
    gb.emu.write_byte(0x0000, 0x0A);
    gb.emu.write_byte(0x4000, 0x10);
    gb.emu.write_byte(0xA002, 0x00);
    gb.emu.write_byte(0xA003, 0x01);
    gb.emu.write_byte(0xA000, 0x01);
}

fn replay_player_with_gb_events(
    dir: &Path,
    name: &str,
    frames: usize,
    events: Vec<ReplayEvent>,
) -> anyhow::Result<ReplayPlayer> {
    let path = dir.join(name);
    let metadata = ReplayMetadata {
        events,
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
    for _ in 0..frames {
        recorder.record_joypad_frame(ReplayJoypadFrame::default());
    }
    recorder.finish()?;
    ReplayPlayer::load(&path)
}

#[test]
fn paired_game_boy_replay_timeline_aligns_common_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline")?;
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        8_121,
        vec![ReplayEvent::GameBoyLink {
            frame: 1_333,
            tick: 2_206_540_680,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 9,
            },
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10_148,
        vec![ReplayEvent::GameBoyLink {
            frame: 273,
            tick: 2_147_632_556,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 9,
                local_reply: None,
            },
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 5);

    assert_eq!(
        timeline,
        PairedGameBoyReplayTimeline {
            left_start_offset: 0,
            right_start_offset: 1_060,
            link_activation_frame: 1_333,
            left_link_activation_frame: 1_333,
            right_link_activation_frame: 1_333,
            left_link_activation_tick: None,
            right_link_activation_tick: None,
            left_target_frames: 8_126,
            right_target_frames: 10_153,
            total_global_frames: 11_213,
        }
    );
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_uses_recorded_link_state_frames() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-link-state")?;
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        8_121,
        vec![
            ReplayEvent::GameBoyLinkState { frame: 100, state },
            ReplayEvent::GameBoyLink {
                frame: 1_333,
                tick: 2_206_540_680,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                },
            },
        ],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10_148,
        vec![
            ReplayEvent::GameBoyLinkState { frame: 20, state },
            ReplayEvent::GameBoyLink {
                frame: 273,
                tick: 2_147_632_556,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                    local_reply: None,
                },
            },
        ],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_link_activation_frame, 1_060);
    assert_eq!(timeline.right_link_activation_frame, 1_080);
    assert_eq!(timeline.left_link_activation_tick, None);
    assert_eq!(timeline.right_link_activation_tick, None);
    assert_eq!(timeline.link_activation_frame, 1_060);
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_uses_recorded_link_state_ticks() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-link-state-ticks")?;
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        10,
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 4,
            tick: 100,
            state,
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 7,
            tick: 200,
            state,
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_link_activation_frame, 4);
    assert_eq!(timeline.right_link_activation_frame, 7);
    assert_eq!(timeline.left_link_activation_tick, Some(100));
    assert_eq!(timeline.right_link_activation_tick, Some(200));
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_defaults_without_common_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-no-common")?;
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        12,
        vec![ReplayEvent::GameBoyLink {
            frame: 4,
            tick: 100,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 1,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 0,
            },
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLink {
            frame: 2,
            tick: 50,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 2,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 0,
                local_reply: None,
            },
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(
        timeline,
        PairedGameBoyReplayTimeline {
            left_start_offset: 0,
            right_start_offset: 0,
            link_activation_frame: 0,
            left_link_activation_frame: 0,
            right_link_activation_frame: 0,
            left_link_activation_tick: None,
            right_link_activation_tick: None,
            left_target_frames: 12,
            right_target_frames: 10,
            total_global_frames: 12,
        }
    );
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_ignores_same_role_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-same-role")?;
    let event = ReplayGameBoyLinkEvent::LocalMasterStart {
        transfer_id: 0x0100_0000_0000_0007,
        clock_period_t_cycles: 4096,
        out_byte: 0x42,
        serial_generation: 3,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        12,
        vec![ReplayEvent::GameBoyLink {
            frame: 4,
            tick: 100,
            event,
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLink {
            frame: 2,
            tick: 50,
            event,
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_start_offset, 0);
    assert_eq!(timeline.right_start_offset, 0);
    assert_eq!(timeline.link_activation_frame, 0);
    Ok(())
}

#[test]
fn headless_replay_route_runs_rom_file_and_checks_final_state_hash() -> anyhow::Result<()> {
    let temp = test_directory("headless-replay-route")?;
    let rom_path = temp.path().join("test.nes");
    let replay_path = temp.path().join("test.zrpl");
    let rom_data = build_nes_test_rom();
    std::fs::write(&rom_path, &rom_data)?;

    let mut expected_backend = load_nes_test_backend(&rom_path, rom_data)?;
    let start_state = expected_backend.encode_state_bytes()?;
    let metadata = expected_backend.replay_metadata();

    let input_frames = [
        ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x02,
            buttons_p2: 0x04,
            dpad_p2: 0x08,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
        ReplayJoypadFrame {
            buttons: 0x03,
            dpad: 0x04,
            buttons_p2: 0x02,
            dpad_p2: 0x01,
            zapper: zeff_emu_common::replay::ReplayZapperFrame {
                enabled: true,
                trigger: true,
                hit: false,
                screen_pos: Some((128, 96)),
            },
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
        ReplayJoypadFrame {
            buttons: 0x08,
            dpad: 0x01,
            buttons_p2: 0x00,
            dpad_p2: 0x04,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
    ];
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
    for frame in &input_frames {
        recorder.record_joypad_frame(frame.clone());
    }
    recorder.finish()?;

    expected_backend.load_state_from_bytes(start_state)?;
    for frame in &input_frames {
        expected_backend.set_input(frame.buttons, frame.dpad);
        expected_backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        expected_backend.set_zapper_state(
            frame.zapper.enabled,
            frame.zapper.trigger,
            frame.zapper.hit,
            frame.zapper.screen_pos,
        );
        expected_backend.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            expected_backend.set_replay_camera_frame(camera_frame);
        }
        expected_backend.step_frame();
    }
    let expected_hash = sha256_hex(&expected_backend.encode_replay_hash_state_bytes()?);

    super::super::run_headless(
        &rom_path,
        HardwareModePreference::Auto,
        Vec::new(),
        &HeadlessOptions {
            replay_path: Some(replay_path),
            expect_replay_final_hash: Some(expected_hash),
            ..HeadlessOptions::default()
        },
    )?;

    Ok(())
}

#[test]
fn loaded_replay_applies_wonder_swan_link_events() -> anyhow::Result<()> {
    let temp = test_directory("wonder-swan-link-replay")?;
    let replay_path = temp.path().join("link.zrpl");
    let mut expected_backend = wonder_swan_test_backend();
    let EmuBackend::Ws(ws) = &mut expected_backend else {
        panic!("expected WonderSwan backend");
    };
    ws.emu.io_write8(0x00B3, 0x80);
    let start_state = expected_backend.encode_state_bytes()?;
    let mut metadata = expected_backend.replay_metadata();
    metadata.wonder_swan_link_start_tick = expected_backend.wonder_swan_cpu_cycles();
    metadata.events.push(ReplayEvent::WonderSwanLink {
        frame: 0,
        session_cycle: 0,
        event: ReplayWonderSwanLinkEvent::RemoteByte {
            generation: 0,
            baud_bps: 9_600,
            byte: 0x5A,
        },
    });
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.finish()?;

    expected_backend.load_state_from_bytes(start_state)?;
    let EmuBackend::Ws(ws) = &mut expected_backend else {
        panic!("expected WonderSwan backend");
    };
    ws.emu.receive_wonder_swan_link_byte(0x5A);
    expected_backend.step_frame();
    let expected_hash = sha256_hex(&expected_backend.encode_replay_hash_state_bytes()?);

    let summary = run_loaded_replay_headless(
        wonder_swan_test_backend(),
        ReplayPlayer::load(&replay_path)?,
        &HeadlessOptions {
            expect_replay_final_hash: Some(expected_hash),
            ..HeadlessOptions::default()
        },
    )?;
    assert_eq!(summary.frames, 1);
    Ok(())
}

#[test]
fn loaded_replay_applies_pocket_camera_frames() -> anyhow::Result<()> {
    let temp = test_directory("camera-replay")?;
    let replay_path = temp.path().join("camera.zrpl");
    let rom_path = temp.path().join("camera.gb");

    let mut expected_backend = load_pocket_camera_test_backend(&rom_path)?;
    let runner_backend = load_pocket_camera_test_backend(&rom_path)?;
    arm_pocket_camera_capture(&mut expected_backend);
    let start_state = expected_backend.encode_state_bytes()?;
    let metadata = expected_backend.replay_metadata();
    let camera_frame = (0..(128 * 112))
        .map(|i| ((i * 17) & 0xFF) as u8)
        .collect::<Vec<_>>();
    let input_frame = ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: Default::default(),
        host_tilt: (0.0, 0.0),
        camera_frame: Some(camera_frame),
        ..ReplayJoypadFrame::default()
    };

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
    recorder.record_joypad_frame(input_frame.clone());
    recorder.finish()?;

    expected_backend.load_state_from_bytes(start_state)?;
    expected_backend.set_replay_camera_frame(
        input_frame
            .camera_frame
            .as_deref()
            .expect("test frame should contain camera data"),
    );
    expected_backend.step_frame();

    let player = ReplayPlayer::load(&replay_path)?;
    let summary = run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

    assert_eq!(summary.frames, 1);
    let raw_final_state = expected_backend.encode_state_bytes()?;
    let replay_hash_state = expected_backend.encode_replay_hash_state_bytes()?;
    assert_eq!(u32::from_le_bytes(raw_final_state[8..12].try_into()?), 13);
    assert_eq!(u32::from_le_bytes(replay_hash_state[8..12].try_into()?), 12);
    assert_eq!(summary.final_state_hash, sha256_hex(&replay_hash_state));

    Ok(())
}

#[test]
fn loaded_replay_restores_start_only_passive_game_boy_completion() -> anyhow::Result<()> {
    let temp = test_directory("gb-passive-start-replay")?;
    let replay_path = temp.path().join("passive-start.zrpl");
    let rom_path = temp.path().join("plain.gb");
    let load_backend = || -> anyhow::Result<EmuBackend> {
        Ok(load_backend_from_rom_source(
            ActiveSystem::GameBoy,
            &rom_path,
            &rom_path,
            Some(vec![0u8; 0x8000]),
            BackendLoadConfig::default(),
        )?
        .backend)
    };
    let mut expected_backend = load_backend()?;
    let runner_backend = load_backend()?;
    let EmuBackend::Gb(gb) = &mut expected_backend else {
        panic!("expected Game Boy backend");
    };
    gb.emu.write_byte(SERIAL_SB, 0x34);
    gb.emu.write_byte(SERIAL_SC, 0x80);
    let start_state = expected_backend.encode_state_bytes()?;
    let link_state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: Some(zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
            peer_byte: 0xAB,
            remaining_t_cycles: 8,
        }),
        serial_generation: 0,
    };
    let mut metadata = expected_backend.replay_metadata();
    metadata.game_boy_link_start_state = Some(link_state);
    metadata.game_boy_link_start_tick = expected_backend.game_boy_cpu_cycles();
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.finish()?;

    expected_backend.load_state_from_bytes(start_state)?;
    assert!(expected_backend.restore_game_boy_link_replay_state(link_state));
    expected_backend.step_frame();
    let expected_hash = sha256_hex(&expected_backend.encode_replay_hash_state_bytes()?);

    let summary = run_loaded_replay_headless(
        runner_backend,
        ReplayPlayer::load(&replay_path)?,
        &HeadlessOptions::default(),
    )?;

    assert_eq!(summary.frames, 1);
    assert_eq!(summary.game_boy_link_events_total, 0);
    assert_eq!(summary.final_state_hash, expected_hash);
    Ok(())
}

#[test]
fn loaded_replay_rejects_pocket_camera_input_for_non_camera_rom() -> anyhow::Result<()> {
    let temp = test_directory("camera-replay-reject")?;
    let replay_path = temp.path().join("camera-on-nes.zrpl");
    let rom_path = temp.path().join("test.nes");
    let rom_data = build_nes_test_rom();

    let backend = load_nes_test_backend(&rom_path, rom_data)?;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: Default::default(),
        host_tilt: (0.0, 0.0),
        camera_frame: Some(vec![0x10, 0x20, 0x30, 0x40]),
        ..ReplayJoypadFrame::default()
    });
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("Pocket Camera replay payload should require Pocket Camera hardware");
    assert!(
        err.to_string().contains("Pocket Camera input"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_rejects_zapper_input_for_non_nes_rom() -> anyhow::Result<()> {
    let temp = test_directory("zapper-replay-reject")?;
    let replay_path = temp.path().join("zapper-on-gb.zrpl");
    let rom_path = temp.path().join("plain.gb");
    let rom_data = vec![0u8; 0x8000];

    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        Some(rom_data),
        BackendLoadConfig::default(),
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: zeff_emu_common::replay::ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((128, 96)),
        },
        host_tilt: (0.0, 0.0),
        camera_frame: None,
        ..ReplayJoypadFrame::default()
    });
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("Zapper replay payload should require NES hardware");
    assert!(
        err.to_string().contains("NES Zapper input"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_rejects_mbc7_tilt_input_for_non_mbc7_rom() -> anyhow::Result<()> {
    let temp = test_directory("tilt-replay-reject")?;
    let replay_path = temp.path().join("tilt-on-plain-gb.zrpl");
    let rom_path = temp.path().join("plain.gb");
    let rom_data = vec![0u8; 0x8000];

    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        Some(rom_data),
        BackendLoadConfig::default(),
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: Default::default(),
        host_tilt: (0.25, -0.5),
        camera_frame: None,
        ..ReplayJoypadFrame::default()
    });
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("MBC7 tilt replay payload should require MBC7 hardware");
    assert!(
        err.to_string().contains("MBC7 tilt input"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_rejects_embedded_final_state_hash_mismatch() -> anyhow::Result<()> {
    let temp = test_directory("replay-hash-mismatch")?;
    let replay_path = temp.path().join("hash-mismatch.zrpl");
    let rom_path = temp.path().join("test.nes");
    let rom_data = build_nes_test_rom();

    let backend = load_nes_test_backend(&rom_path, rom_data)?;
    let start_state = backend.encode_state_bytes()?;
    let mut metadata = backend.replay_metadata();
    metadata.final_state_sha256 = Some([0xA5; 32]);

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_frame(0, 0);
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("embedded hash mismatch should fail playback");
    assert!(
        err.to_string()
            .contains("replay embedded final state hash mismatch"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn game_boy_link_replay_divergence_is_strict_unless_diagnostic_mode_is_explicit() {
    let strict = HeadlessOptions {
        expect_gb_link_events: 1,
        ..HeadlessOptions::default()
    };
    let err = validate_game_boy_link_replay_result_for_test(
        Err(anyhow::anyhow!("link checkpoint mismatch")),
        &strict,
    )
    .expect_err("event-count expectations must not weaken replay validation");
    assert!(err.to_string().contains("link checkpoint mismatch"));

    let diagnostic = HeadlessOptions {
        allow_gb_link_replay_divergence: true,
        ..HeadlessOptions::default()
    };
    validate_game_boy_link_replay_result_for_test(
        Err(anyhow::anyhow!("legacy link checkpoint mismatch")),
        &diagnostic,
    )
    .expect("diagnostic mode should preserve warning-only legacy playback");
}

#[test]
fn loaded_replay_reports_checkpoint_divergence_frame() -> anyhow::Result<()> {
    let temp = test_directory("replay-checkpoint-mismatch")?;
    let replay_path = temp.path().join("checkpoint-mismatch.zrpl");
    let rom_path = temp.path().join("test.nes");
    let rom_data = build_nes_test_rom();

    let backend = load_nes_test_backend(&rom_path, rom_data)?;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_frame(0, 0);
    recorder.record_checkpoint(1, [0xA5; 32]);
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("checkpoint mismatch should fail playback");
    assert!(
        err.to_string()
            .contains("replay diverged at checkpoint frame 1"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_validates_cursor_zero_checkpoint_before_execution() -> anyhow::Result<()> {
    let temp = test_directory("replay-checkpoint-zero")?;
    let replay_path = temp.path().join("checkpoint-zero.zrpl");
    let rom_path = temp.path().join("test.nes");
    let backend = load_nes_test_backend(&rom_path, build_nes_test_rom())?;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_checkpoint(0, [0xA5; 32]);
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("cursor-zero checkpoint mismatch should fail before execution");
    assert!(
        err.to_string()
            .contains("replay diverged at checkpoint frame 0"),
        "unexpected error: {err}"
    );
    Ok(())
}

#[test]
fn loaded_replay_validates_matching_checkpoint() -> anyhow::Result<()> {
    let temp = test_directory("replay-checkpoint")?;
    let replay_path = temp.path().join("checkpoint.zrpl");
    let rom_path = temp.path().join("test.nes");
    let rom_data = build_nes_test_rom();

    let mut expected_backend = load_nes_test_backend(&rom_path, rom_data.clone())?;
    let start_state = expected_backend.encode_state_bytes()?;
    let metadata = expected_backend.replay_metadata();
    expected_backend.step_frame();
    let checkpoint_hash =
        zeff_firmware::sha256_bytes(&expected_backend.encode_replay_hash_state_bytes()?);

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.record_frame(0, 0);
    recorder.record_checkpoint(1, checkpoint_hash);
    recorder.finish()?;

    let summary = run_loaded_replay_headless(
        load_nes_test_backend(&rom_path, rom_data)?,
        ReplayPlayer::load(&replay_path)?,
        &HeadlessOptions::default(),
    )?;
    assert_eq!(summary.frames, 1);
    Ok(())
}

#[test]
fn loaded_replay_rejects_cheat_dependent_metadata() -> anyhow::Result<()> {
    let temp = test_directory("replay-cheat-metadata")?;
    let replay_path = temp.path().join("cheats.zrpl");
    let rom_path = temp.path().join("test.nes");
    let rom_data = build_nes_test_rom();

    let backend = load_nes_test_backend(&rom_path, rom_data)?;
    let start_state = backend.encode_state_bytes()?;
    let mut metadata = backend.replay_metadata();
    metadata.cheat_sha256 = Some([0x5A; 32]);

    let recorder = ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("headless replay should reject cheat-dependent metadata");
    assert!(
        err.to_string().contains("enabled cheat set"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_rejects_declared_core_family_mismatch() -> anyhow::Result<()> {
    let temp = test_directory("replay-core-family-mismatch")?;
    let replay_path = temp.path().join("wrong-core.zrpl");
    let rom_path = temp.path().join("test.nes");
    let backend = load_nes_test_backend(&rom_path, build_nes_test_rom())?;
    let start_state = backend.encode_state_bytes()?;
    let mut metadata = backend.replay_metadata();
    metadata.core_family = Some("different-core".to_owned());
    ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata).finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
        .expect_err("declared replay core-family mismatch should fail preflight");
    assert!(
        err.to_string().contains("replay core family differs"),
        "unexpected error: {err}"
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_link_event_floor_rejects_incomplete_replay_metadata() -> anyhow::Result<()> {
    let temp = test_directory("replay-link-event-floor")?;
    let replay_path = temp.path().join("short-link.zrpl");
    let metadata = ReplayMetadata {
        events: vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 1,
                out_byte: 0x42,
                passive: true,
                serial_generation: 7,
            },
        }],
        ..ReplayMetadata::default()
    };
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), b"state".to_vec(), metadata);
    recorder.record_frame(0, 0);
    recorder.finish()?;

    let player = ReplayPlayer::load(&replay_path)?;
    let err = ensure_replay_metadata_has_expected_gb_link_events(
        "left",
        &player,
        &HeadlessOptions {
            expect_gb_link_events: 2,
            ..HeadlessOptions::default()
        },
    )
    .expect_err("short GB link replay should fail the event-count preflight");
    assert!(
        err.to_string().contains("only 1 GB link events"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn loaded_replay_applies_fds_side_events_before_matching_frame() -> anyhow::Result<()> {
    let temp = test_directory("fds-replay-event")?;
    let replay_path = temp.path().join("side-change.zrpl");
    let rom_path = temp.path().join("test.fds");

    let mut expected_backend = load_fds_test_backend(&rom_path)?;
    let runner_backend = load_fds_test_backend(&rom_path)?;
    let start_state = expected_backend.encode_state_bytes()?;
    let metadata = expected_backend.replay_metadata();

    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
    recorder.record_frame(0x00, 0x00);
    recorder.record_event(ReplayEvent::FdsDiskSide { frame: 1, side: 1 });
    recorder.record_frame(0x00, 0x00);
    recorder.finish()?;

    expected_backend.load_state_from_bytes(start_state)?;
    expected_backend.set_input(0x00, 0x00);
    expected_backend.step_frame();
    expected_backend.set_fds_disk_side(1)?;
    expected_backend.set_input(0x00, 0x00);
    expected_backend.step_frame();

    let expected_hash = sha256_hex(&expected_backend.encode_replay_hash_state_bytes()?);
    let player = ReplayPlayer::load(&replay_path)?;
    let summary = run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

    assert_eq!(summary.frames, 2);
    assert_eq!(summary.events_applied, 1);
    assert_eq!(summary.final_state_hash, expected_hash);
    assert_eq!(expected_backend.fds_disk_side(), Some(1));

    Ok(())
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn verification_hashes_final_cursor_before_boundary_event_and_final_state_after_it()
-> anyhow::Result<()> {
    let temp = test_directory("final-cursor-verification-order")?;
    let replay_path = temp.path().join("final-side-change.zrpl");
    let rom_path = temp.path().join("test.fds");

    let source_backend = load_fds_test_backend(&rom_path)?;
    let runner_backend = load_fds_test_backend(&rom_path)?;
    let start_state = source_backend.encode_state_bytes()?;
    let mut metadata = source_backend.replay_metadata();
    metadata.events = vec![ReplayEvent::FdsDiskSide {
        frame: 300,
        side: 1,
    }];
    let mut recorder =
        ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
    for _ in 0..300 {
        recorder.record_joypad_frame(ReplayJoypadFrame::default());
    }
    recorder.finish()?;
    let player = ReplayPlayer::load(&replay_path)?;

    let run = run_loaded_replay_for_verification(runner_backend, player, true)?;

    assert_eq!(run.frames, 300);
    assert_eq!(run.checkpoints.len(), 1);
    assert_eq!(run.checkpoints[0].frame, 300);
    assert_ne!(run.checkpoints[0].state_sha256, run.final_state_sha256);
    Ok(())
}
