use super::*;
use crate::settings::MdRegionPreference;
use zeff_emu_common::replay::{
    ReplayEvent, ReplayFirmwareManifest, ReplayMetadata, ReplayRecorder,
};

fn player(
    path: &Path,
    state: Vec<u8>,
    metadata: ReplayMetadata,
    frames: &[ReplayJoypadFrame],
) -> Result<ReplayPlayer> {
    let mut recorder = ReplayRecorder::new_with_metadata(path.to_owned(), state, metadata);
    for frame in frames {
        recorder.record_joypad_frame(frame.clone());
    }
    recorder.finish()?;
    ReplayPlayer::load(path)
}

#[test]
fn md_replay_actual_app_rejects_identity_and_late_payload_transactionally() -> Result<()> {
    let directory = crate::test_support::test_directory("md-replay-reject")?;
    let path = directory.path().join("input.md");
    let replay = directory.path().join("bad.zrpl");
    let settings = settings();
    let bytes = fixture(b'U', false, false);
    let backend = load(&path, bytes.clone(), &settings)?;
    let metadata = backend.replay_metadata();
    let EmuBackend::Md(md) = &backend else {
        unreachable!()
    };
    let mut oracle = md.core().clone();
    let mut wrong_states = Vec::new();
    let mut changed_rom = bytes.clone();
    changed_rom[0x180] ^= 1;
    wrong_states.push((
        "ROM digest",
        load(&path, changed_rom, &settings)?.encode_state_bytes()?,
    ));
    for (label, region, controller, timing) in [
        (
            "region",
            MdRegionPreference::Pal,
            MdControllerPreference::SixButton,
            MdTimingPreference::Standard,
        ),
        (
            "controller",
            MdRegionPreference::Auto,
            MdControllerPreference::ThreeButton,
            MdTimingPreference::Standard,
        ),
        (
            "timing model",
            MdRegionPreference::Auto,
            MdControllerPreference::SixButton,
            MdTimingPreference::Precise,
        ),
    ] {
        let mut changed = settings.clone();
        changed.emulation.md_region = region;
        changed.emulation.md_controller = controller;
        changed.emulation.md_timing = timing;
        wrong_states.push((
            label,
            load(&path, bytes.clone(), &changed)?.encode_state_bytes()?,
        ));
    }
    with_app(backend, settings, &path, |app| {
        for _ in 0..2 {
            oracle
                .step_field()
                .map_err(|error| anyhow::anyhow!("{error:?}"))?;
            ensure!(field(app)? == oracle.drain_audio_samples());
        }
        let before = capture(app)?;
        let frame = app.latest_frame.clone().context("framebuffer")?;
        ensure!(
            before.ends_with(&[1, 0, 0, 0, 0]),
            "plain download-owner tail changed"
        );
        let mut late = before.clone();
        *late.last_mut().unwrap() = 2;
        let mut trailing = before.clone();
        trailing.push(0);
        wrong_states.extend([
            ("short header", before[..12].to_vec()),
            ("truncated tail", before[..before.len() - 1].to_vec()),
            ("invalid late boolean", late),
            ("trailing byte", trailing),
        ]);
        for (label, state) in wrong_states {
            ensure!(
                app.play_replay(player(
                    &replay,
                    state,
                    metadata.clone(),
                    &[Default::default()]
                )?)
                .is_err(),
                "raw replay case {label} was accepted"
            );
            ensure!(capture(app)? == before && app.latest_frame.as_ref() == Some(&frame));
            ensure!(app.recording.replay_player.is_none());
        }
        let mut wrong_metadata = Vec::new();
        let mut system = metadata.clone();
        system.system = Some("nes".into());
        wrong_metadata.push(system);
        let mut family = metadata.clone();
        family.core_family = Some("wrong".into());
        wrong_metadata.push(family);
        let mut rom = metadata.clone();
        rom.rom_sha256 = Some([0x55; 32]);
        wrong_metadata.push(rom);
        let mut firmware = metadata.clone();
        firmware.firmware.push(ReplayFirmwareManifest::Skipped {
            firmware_id: "wrong".into(),
            compatibility_version: 1,
        });
        wrong_metadata.push(firmware);
        let mut media = metadata.clone();
        media
            .events
            .push(ReplayEvent::FdsDiskSide { frame: 0, side: 0 });
        wrong_metadata.push(media);
        for metadata in wrong_metadata {
            ensure!(
                app.play_replay(player(
                    &replay,
                    before.clone(),
                    metadata,
                    &[Default::default()]
                )?)
                .is_err()
            );
            ensure!(capture(app)? == before && app.latest_frame.as_ref() == Some(&frame));
        }
        for input in [
            ReplayJoypadFrame {
                buttons_p3: 1,
                ..Default::default()
            },
            ReplayJoypadFrame {
                dpad_p2: 0x80,
                ..Default::default()
            },
            ReplayJoypadFrame {
                host_tilt: (1.0, 0.0),
                ..Default::default()
            },
            ReplayJoypadFrame {
                zapper: zeff_emu_common::replay::ReplayZapperFrame {
                    enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        ] {
            ensure!(
                app.play_replay(player(&replay, before.clone(), metadata.clone(), &[input])?)
                    .is_err()
            );
            ensure!(capture(app)? == before);
        }
        oracle
            .step_field()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        ensure!(
            field(app)? == oracle.drain_audio_samples(),
            "failed replay changed future PCM"
        );
        ensure!(capture(app)? == oracle.encode_state()?);
        Ok(())
    })
}

#[test]
fn md_replay_actual_app_guards_pending_active_and_preserves_paused_recording_origin() -> Result<()>
{
    let directory = crate::test_support::test_directory("md-replay-start-guards")?;
    let path = directory.path().join("input.md");
    let replay = directory.path().join("play.zrpl");
    let recording = directory.path().join("record.zrpl");
    let settings = settings();
    let backend = load(&path, fixture(b'U', false, false), &settings)?;
    let metadata = backend.replay_metadata();
    let EmuBackend::Md(md) = &backend else {
        unreachable!()
    };
    let mut oracle = md.core().clone();
    with_app(backend, settings, &path, |app| {
        field(app)?;
        let start = capture(app)?;
        player(
            &replay,
            start.clone(),
            metadata.clone(),
            &[Default::default()],
        )?;
        app.start_replay_recording_to_path(recording.clone())?;
        ensure!(app.recording.is_replay_start_pending());
        ensure!(
            app.play_replay(ReplayPlayer::load(&replay)?)
                .unwrap_err()
                .to_string()
                .contains("stop replay recording")
        );
        wait(app, |app| !app.recording.is_replay_start_pending())?;
        ensure!(app.recording.replay_recorder.is_some());
        ensure!(app.play_replay(ReplayPlayer::load(&replay)?).is_err());
        app.stop_replay_recording()?;
        wait(app, |app| !app.recording.is_replay_finalizing())?;
        ensure!(capture(app)? == start);

        oracle.decode_state(&start)?;
        let pc = oracle.registers().pc;
        app.pending_debug_actions.add_breakpoint = Some(pc);
        app.debug_requests.frame_advance = true;
        app.tick();
        let result = receive(app)?;
        ensure!(result.advanced_frames == 0 && result.runtime_fault.is_none());
        app.process_frame_result(result);
        oracle
            .step_instruction()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        app.debug_requests.step = true;
        app.tick();
        let stepped = receive(app)?;
        ensure!(stepped.advanced_frames == 0 && stepped.runtime_fault.is_none());
        app.process_frame_result(stepped);
        let paused_start = capture(app)?;
        ensure!(paused_start == oracle.encode_state()? && paused_start != start);
        let paused = directory.path().join("paused.zrpl");
        app.start_replay_recording_to_path(paused.clone())?;
        wait(app, |app| !app.recording.is_replay_start_pending())?;
        ensure!(app.recording.replay_recorder.is_some());
        ensure!(
            capture(app)? == paused_start,
            "paused capture mutated the machine"
        );
        ensure!(app.recording.replay_recording_origin.frame == oracle.pacing_field_count());
        app.pending_debug_actions.remove_breakpoints.push(pc);
        app.debug_requests.continue_ = true;
        app.tick();
        let result = receive(app)?;
        ensure!(result.advanced_frames == 0 && result.runtime_fault.is_none());
        app.process_frame_result(result);
        ensure!(
            capture(app)? == paused_start,
            "resuming changed the captured origin"
        );
        ensure!(field(app)?.iter().any(|sample| *sample != 0.0));
        let recorded = capture(app)?;
        let frame = app.latest_frame.clone().context("recorded framebuffer")?;
        app.stop_replay_recording()?;
        wait(app, |app| !app.recording.is_replay_finalizing())?;
        let movie = ReplayPlayer::load(&paused)?;
        ensure!(movie.total_frames() == 1 && movie.save_state() == paused_start);
        ensure!(
            movie.metadata().final_state_sha256 == Some(zeff_firmware::sha256_bytes(&recorded))
        );
        let mut previous_pcm = None;
        for _ in 0..2 {
            app.play_replay(ReplayPlayer::load(&paused)?)?;
            ensure!(capture(app)? == paused_start);
            let pcm = field(app)?
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>();
            ensure!(
                pcm.iter().any(|&bits| f32::from_bits(bits) != 0.0),
                "cold replay PCM is silent"
            );
            wait(app, |app| app.recording.replay_player.is_none())?;
            ensure!(capture(app)? == recorded);
            ensure!(app.latest_frame.as_ref() == Some(&frame));
            if let Some(expected) = &previous_pcm {
                ensure!(&pcm == expected, "cold replay PCM changed");
            }
            previous_pcm = Some(pcm);
        }
        Ok(())
    })
}

#[test]
fn md_replay_actual_app_fault_commits_only_executed_input_prefix() -> Result<()> {
    let directory = crate::test_support::test_directory("md-replay-fault-prefix")?;
    let path = directory.path().join("fault.md");
    let replay = directory.path().join("fault.zrpl");
    let settings = settings();
    let backend = load(&path, fixture(b'U', false, true), &settings)?;
    let long_replay = directory.path().join("long-fault.zrpl");
    let inputs = vec![
        ReplayJoypadFrame {
            buttons: 0x41,
            dpad: 4,
            buttons_p2: 0x82,
            dpad_p2: 1,
            ..Default::default()
        };
        8
    ];
    player(
        &long_replay,
        backend.encode_state_bytes()?,
        backend.replay_metadata(),
        &inputs,
    )?;
    let prefix = with_app(backend, settings.clone(), &path, |app| {
        app.start_replay_recording_to_path(replay.clone())?;
        wait(app, |app| !app.recording.is_replay_start_pending())?;
        host(app, (0x41, 4), (0x82, 1));
        app.settings.emulation.uncapped_speed = true;
        app.settings.emulation.uncapped_frames_per_tick = 8;
        app.set_user_paused(false);
        app.tick();
        let result = receive(app)?;
        let prefix = result.advanced_frames;
        ensure!(prefix > 0 && prefix < 8 && result.staged_input_frames == prefix);
        ensure!(result.runtime_fault.is_some() && result.completed_step_requests == 1);
        app.process_frame_result(result);
        ensure!(app.speed.paused && app.recording.pending_replay_batches.is_empty());
        wait(app, |app| !app.recording.is_replay_finalizing())?;
        let movie = ReplayPlayer::load(&replay)?;
        ensure!(movie.total_frames() == prefix && movie.metadata().final_state_sha256.is_none());
        ensure!(movie.peek_joypad_frames(0, prefix).iter().all(|frame| {
            (frame.buttons, frame.dpad, frame.buttons_p2, frame.dpad_p2) == (0x41, 4, 0x82, 1)
        }));
        let rejected = directory.path().join("faulted-start.zrpl");
        app.start_replay_recording_to_path(rejected.clone())?;
        wait(app, |app| !app.recording.is_replay_start_pending())?;
        ensure!(app.recording.replay_recorder.is_none() && !rejected.exists());
        app.settings.emulation.uncapped_speed = false;
        app.set_user_paused(true);
        app.play_replay(movie)?;
        for _ in 0..prefix {
            field(app)?;
        }
        ensure!(app.recording.replay_player.is_none());
        Ok(prefix)
    })?;
    let backend = load(&path, fixture(b'U', false, true), &settings)?;
    with_app(backend, settings, &path, |app| {
        app.play_replay(ReplayPlayer::load(&long_replay)?)?;
        app.settings.emulation.uncapped_speed = true;
        app.settings.emulation.uncapped_frames_per_tick = 8;
        app.set_user_paused(false);
        app.tick();
        ensure!(app.recording.queued_replay_playback_frames == 8);
        ensure!(app.recording.pending_replay_batches.front().unwrap().frames == inputs);
        let result = receive(app)?;
        ensure!(result.runtime_fault.is_some() && result.completed_step_requests == 1);
        ensure!(result.advanced_frames == prefix && result.staged_input_frames == prefix);
        ensure!(app.recording.replay_player.as_ref().unwrap().cursor() == 0);
        app.process_frame_result(result);
        ensure!(app.frames_in_flight == 0 && app.speed.paused);
        ensure!(app.recording.replay_player.is_none());
        ensure!(app.recording.pending_replay_batches.is_empty());
        ensure!(app.recording.queued_replay_playback_frames == 0);
        Ok(())
    })
}

#[test]
fn replay_playback_helper_preserves_nes_success_path() -> Result<()> {
    let directory = crate::test_support::test_directory("nes-replay-app-helper")?;
    let path = directory.path().join("input.nes");
    let replay = directory.path().join("input.zrpl");
    let settings = settings();
    let config = BackendLoadConfig {
        sample_rate: Some(48_000),
        nes_load_battery_sram: false,
        ..Default::default()
    };
    let mut expected = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        Some(crate::test_support::build_nes_test_rom()),
        config.clone(),
    )?
    .backend;
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        Some(crate::test_support::build_nes_test_rom()),
        config,
    )?
    .backend;
    let metadata = backend.replay_metadata();
    with_app(backend, settings, &path, |app| {
        field(app)?;
        let start = capture(app)?;
        expected.load_state_from_bytes(start.clone())?;
        let inputs = [
            ReplayJoypadFrame {
                buttons: 1,
                dpad: 2,
                buttons_p2: 2,
                dpad_p2: 4,
                ..Default::default()
            },
            ReplayJoypadFrame::default(),
        ];
        let movie = player(&replay, start.clone(), metadata, &inputs)?;
        field(app)?;
        app.play_replay(movie)?;
        ensure!(capture(app)? == start);
        for input in inputs {
            expected.apply_replay_input(&input);
            expected.step_frame();
            field(app)?;
            ensure!(capture(app)? == expected.encode_state_bytes()?);
            ensure!(app.latest_frame.as_ref().unwrap().as_slice() == expected.framebuffer());
        }
        ensure!(app.recording.replay_player.is_none());
        Ok(())
    })
}

#[test]
fn md_replay_actual_app_zero_frame_preserves_held_origin_exactly() -> Result<()> {
    let directory = crate::test_support::test_directory("md-replay-zero-held-origin")?;
    let path = directory.path().join("held.md");
    let replay = directory.path().join("zero.zrpl");
    let settings = settings();
    let backend = load(&path, fixture(b'U', false, false), &settings)?;
    let mut metadata = backend.replay_metadata();
    with_app(backend, settings, &path, |app| {
        host(app, (0x75, 6), (0x8a, 9));
        field(app)?;
        field(app)?;
        let start = capture(app)?;
        let hash = zeff_firmware::sha256_bytes(&start);
        metadata.final_state_sha256 = Some(hash);
        metadata
            .checkpoints
            .push(zeff_emu_common::replay::ReplayCheckpoint {
                frame: 0,
                state_sha256: hash,
            });
        let movie = player(&replay, start.clone(), metadata, &[])?;
        ensure!(movie.total_frames() == 0 && movie.cursor() == 0);
        ensure!(movie.metadata().final_state_sha256 == Some(hash));
        app.play_replay(movie)?;
        let restored = capture(app)?;
        ensure!(
            restored == start,
            "zero-frame replay changed held origin pads"
        );
        ensure!(zeff_firmware::sha256_bytes(&restored) == hash);
        ensure!(app.recording.replay_player.as_ref().unwrap().is_finished());
        ensure!(app.recording.queued_replay_playback_frames == 0);
        Ok(())
    })
}
