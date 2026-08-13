use std::path::Path;
use std::time::Instant;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use zeff_emu_common::replay::{ReplayEvent, ReplayMetadata, ReplayPlayer};
use zeff_firmware::sha256_hex;

use super::HeadlessOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayHeadlessSummary {
    frames: usize,
    events_applied: usize,
    final_state_hash: String,
    final_framebuffer_hash: String,
}

pub(super) fn run_replay_headless(
    source_path: &Path,
    rom_path: &Path,
    rom_data: Vec<u8>,
    system: ActiveSystem,
    firmware_search_dirs: Vec<std::path::PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let replay_path = opts
        .replay_path
        .as_ref()
        .expect("run_replay_headless requires replay_path");
    let player = ReplayPlayer::load(replay_path)?;
    let loaded = load_backend_from_rom_source(
        system,
        source_path,
        rom_path,
        Some(rom_data),
        BackendLoadConfig {
            firmware_search_dirs,
            ..BackendLoadConfig::default()
        },
    )?;

    let start = Instant::now();
    let summary = run_loaded_replay_headless(loaded.backend, player, opts)?;
    println!(
        "[headless] replay frames={} events={} final_state_sha256={} final_framebuffer_sha256={} elapsed_ms={}",
        summary.frames,
        summary.events_applied,
        summary.final_state_hash,
        summary.final_framebuffer_hash,
        start.elapsed().as_millis()
    );

    Ok(())
}

fn run_loaded_replay_headless(
    mut backend: EmuBackend,
    mut player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<ReplayHeadlessSummary> {
    validate_replay_playback(&player, &backend)?;
    backend.load_state_from_bytes(player.save_state().to_vec())?;
    #[cfg(not(target_arch = "wasm32"))]
    let mut game_boy_replay_link = {
        let link = crate::link::gb::GameBoyReplayLink::new(
            player.metadata().events.clone(),
            backend.frame_count(),
        );
        (!link.is_empty()).then_some(link)
    };

    let mut events_applied = 0usize;
    while !player.is_finished() {
        apply_replay_events_at_cursor(&mut backend, &mut player, &mut events_applied)?;
        let Some(frame) = player.next_joypad_frame() else {
            break;
        };
        backend.set_input(frame.buttons, frame.dpad);
        backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        backend.set_zapper_state(
            frame.zapper.enabled,
            frame.zapper.trigger,
            frame.zapper.hit,
            frame.zapper.screen_pos,
        );
        backend.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            backend.set_replay_camera_frame(camera_frame);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(link) = game_boy_replay_link.as_mut() {
            backend
                .step_game_boy_frame_with_replay_link(link)
                .map_err(|err| anyhow::anyhow!("replay Game Boy link event failed: {err:?}"))?;
        } else {
            backend.step_frame();
        }
        #[cfg(target_arch = "wasm32")]
        backend.step_frame();
    }
    apply_replay_events_at_cursor(&mut backend, &mut player, &mut events_applied)?;

    let final_state = backend.encode_state_bytes()?;
    let final_state_hash = sha256_hex(&final_state);
    let final_framebuffer_hash = sha256_hex(backend.framebuffer());

    if let Some(expected) = player.metadata().final_state_sha256 {
        let expected = digest_hex(&expected);
        if !final_state_hash.eq_ignore_ascii_case(&expected) {
            anyhow::bail!(
                "replay embedded final state hash mismatch: expected {expected}, got {final_state_hash}"
            );
        }
    }

    if let Some(expected) = opts.expect_replay_final_hash.as_deref()
        && !final_state_hash.eq_ignore_ascii_case(expected)
    {
        anyhow::bail!(
            "replay final state hash mismatch: expected {expected}, got {final_state_hash}"
        );
    }

    Ok(ReplayHeadlessSummary {
        frames: player.total_frames(),
        events_applied,
        final_state_hash,
        final_framebuffer_hash,
    })
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn validate_replay_playback(player: &ReplayPlayer, backend: &EmuBackend) -> anyhow::Result<()> {
    validate_replay_metadata(player.metadata(), backend)?;
    validate_replay_input_devices(player, backend)
}

fn validate_replay_metadata(metadata: &ReplayMetadata, backend: &EmuBackend) -> anyhow::Result<()> {
    if metadata.is_empty() {
        return Ok(());
    }
    let current = backend.replay_metadata();

    if metadata.system != current.system {
        anyhow::bail!(
            "replay system differs: replay={}, current={}",
            metadata.system.as_deref().unwrap_or("unknown"),
            current.system.as_deref().unwrap_or("unknown")
        );
    }
    if metadata.rom_sha256 != current.rom_sha256 {
        anyhow::bail!("replay ROM hash differs");
    }
    if metadata.firmware != current.firmware {
        anyhow::bail!("replay firmware differs");
    }
    if metadata.cheat_sha256.is_some() {
        anyhow::bail!(
            "replay requires an enabled cheat set; headless replay cheat restoration is not wired"
        );
    }

    Ok(())
}

fn validate_replay_input_devices(
    player: &ReplayPlayer,
    backend: &EmuBackend,
) -> anyhow::Result<()> {
    if player.uses_zapper_input() && backend.system() != ActiveSystem::Nes {
        anyhow::bail!("replay contains NES Zapper input but current ROM is not a NES game");
    }
    if player.uses_game_boy_link_events() && backend.system() != ActiveSystem::GameBoy {
        anyhow::bail!("replay contains Game Boy link events but current ROM is not a GB/GBC game");
    }
    if player.uses_wonder_swan_link_events() && backend.system() != ActiveSystem::WonderSwan {
        anyhow::bail!(
            "replay contains WonderSwan link events but current ROM is not a WonderSwan game"
        );
    }
    if player.uses_host_tilt_input() && !backend.is_mbc7() {
        anyhow::bail!("replay contains MBC7 tilt input but current ROM is not an MBC7 game");
    }
    if player.uses_host_camera_input() && !backend.is_pocket_camera() {
        anyhow::bail!(
            "replay contains Pocket Camera input but current ROM is not a Pocket Camera game"
        );
    }
    validate_replay_host_input_frame_shapes(player)?;
    Ok(())
}

fn validate_replay_host_input_frame_shapes(player: &ReplayPlayer) -> anyhow::Result<()> {
    for (frame_index, len) in player.host_camera_frame_lengths() {
        if len != zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES {
            anyhow::bail!(
                "replay Pocket Camera frame {frame_index} has {len} bytes, expected {}",
                zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES
            );
        }
    }
    Ok(())
}

fn apply_replay_events_at_cursor(
    backend: &mut EmuBackend,
    player: &mut ReplayPlayer,
    events_applied: &mut usize,
) -> anyhow::Result<()> {
    for event in player.take_events_at_cursor() {
        match event {
            ReplayEvent::FdsDiskSide { side, .. } => {
                backend.set_fds_disk_side(side)?;
                *events_applied += 1;
            }
            ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    use zeff_emu_common::replay::{ReplayEvent, ReplayJoypadFrame, ReplayPlayer, ReplayRecorder};
    use zeff_firmware::sha256_hex;
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;

    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn test_temp_dir(prefix: &str) -> anyhow::Result<TestTempDir> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), suffix));
        std::fs::create_dir(&path)?;
        Ok(TestTempDir { path })
    }

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

    fn build_nes_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg] = 0xA9;
        rom[prg + 1] = 0x42;
        rom[prg + 2] = 0x85;
        rom[prg + 3] = 0x00;
        rom[prg + 4] = 0xEA;
        rom[prg + 5] = 0xEA;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
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

    #[test]
    fn headless_replay_route_runs_rom_file_and_checks_final_state_hash() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_headless_replay_route")?;
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
            },
            ReplayJoypadFrame {
                buttons: 0x08,
                dpad: 0x01,
                buttons_p2: 0x00,
                dpad_p2: 0x04,
                zapper: Default::default(),
                host_tilt: (0.0, 0.0),
                camera_frame: None,
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
        let expected_hash = sha256_hex(&expected_backend.encode_state_bytes()?);

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
    fn loaded_replay_applies_pocket_camera_frames() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_camera_replay")?;
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
        let summary =
            run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

        assert_eq!(summary.frames, 1);
        assert_eq!(
            summary.final_state_hash,
            sha256_hex(&expected_backend.encode_state_bytes()?)
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_pocket_camera_input_for_non_camera_rom() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_camera_replay_reject")?;
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
        let temp = test_temp_dir("zeff_zapper_replay_reject")?;
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
        let temp = test_temp_dir("zeff_tilt_replay_reject")?;
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
        let temp = test_temp_dir("zeff_replay_hash_mismatch")?;
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
    fn loaded_replay_rejects_cheat_dependent_metadata() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_replay_cheat_metadata")?;
        let replay_path = temp.path().join("cheats.zrpl");
        let rom_path = temp.path().join("test.nes");
        let rom_data = build_nes_test_rom();

        let backend = load_nes_test_backend(&rom_path, rom_data)?;
        let start_state = backend.encode_state_bytes()?;
        let mut metadata = backend.replay_metadata();
        metadata.cheat_sha256 = Some([0x5A; 32]);

        let recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
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
    fn loaded_replay_applies_fds_side_events_before_matching_frame() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_fds_replay_event")?;
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

        let expected_hash = sha256_hex(&expected_backend.encode_state_bytes()?);
        let player = ReplayPlayer::load(&replay_path)?;
        let summary =
            run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

        assert_eq!(summary.frames, 2);
        assert_eq!(summary.events_applied, 1);
        assert_eq!(summary.final_state_hash, expected_hash);
        assert_eq!(expected_backend.fds_disk_side(), Some(1));

        Ok(())
    }
}
