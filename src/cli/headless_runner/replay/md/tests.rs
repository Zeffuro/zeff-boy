use super::*;
use crate::cli::headless_runner::{md::run_md_headless, run_headless};
use crate::emu_backend::EmuBackend;
use zeff_emu_common::replay::{ReplayJoypadFrame, ReplayRecorder};
use zeff_firmware::{sha256_bytes, sha256_hex};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_md_core::{ControllerKind, Region, TimingModel};

fn fixture(directory: &Path) -> anyhow::Result<(std::path::PathBuf, HeadlessOptions)> {
    fixture_with_save(directory, false)
}

fn fixture_with_save(
    directory: &Path,
    with_save: bool,
) -> anyhow::Result<(std::path::PathBuf, HeadlessOptions)> {
    let mut rom = zeff_pgo_corpus::fixtures()
        .into_iter()
        .find(|fixture| fixture.system == "MD")
        .unwrap()
        .bytes;
    if with_save {
        rom[0x1b0..0x1ba].copy_from_slice(&[0x52, 0x41, 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
        rom[0x1ba..0x1bc].copy_from_slice(&1023_u16.to_be_bytes());
    }
    let timing = if with_save {
        TimingModel::Standard
    } else {
        TimingModel::Precise
    };
    let path = directory.join("input.md");
    let replay = directory.join("input.zrpl");
    std::fs::write(&path, &rom)?;
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        Some(rom),
        BackendLoadConfig {
            md_region: Some(Region::Pal),
            md_timing: timing,
            md_controller: ControllerKind::ThreeButton,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )?
    .backend;
    let battery = backend.battery_components();
    if with_save {
        assert_eq!(battery.len(), 1);
        assert_eq!(battery[0].1.len(), 512);
    } else {
        assert!(battery.is_empty());
    }
    for _ in 0..2 {
        backend.step_frame();
    }
    let mut recorder = ReplayRecorder::new_with_metadata(
        replay.clone(),
        backend.encode_state_bytes()?,
        backend.replay_metadata(),
    );
    for (index, (buttons, dpad)) in [(0x41, 4), (0x34, 2), (0, 0)].into_iter().enumerate() {
        let frame = ReplayJoypadFrame {
            buttons,
            dpad,
            buttons_p2: buttons ^ 0xff,
            dpad_p2: dpad ^ 15,
            ..Default::default()
        };
        recorder.record_joypad_frame(frame.clone());
        backend.apply_replay_input(&frame);
        backend.step_frame();
        recorder.record_checkpoint(
            index as u64 + 1,
            sha256_bytes(&backend.encode_replay_hash_state_bytes()?),
        );
    }
    let final_state = backend.encode_replay_hash_state_bytes()?;
    recorder.set_final_state_sha256(sha256_bytes(&final_state));
    recorder.finish()?;
    Ok((
        path,
        HeadlessOptions {
            replay_path: Some(replay),
            md_region: Some(Region::Pal),
            md_timing: Some(timing),
            md_controller: Some(ControllerKind::ThreeButton),
            expect_replay_final_hash: Some(sha256_hex(&final_state)),
            ..Default::default()
        },
    ))
}

fn replay_options(source: &HeadlessOptions) -> HeadlessOptions {
    HeadlessOptions {
        replay_path: source.replay_path.clone(),
        md_region: source.md_region,
        md_timing: source.md_timing,
        md_controller: source.md_controller,
        expect_replay_final_hash: source.expect_replay_final_hash.clone(),
        ..Default::default()
    }
}

fn run(path: &Path, options: &HeadlessOptions) -> anyhow::Result<()> {
    run_headless(path, HardwareModePreference::Auto, Vec::new(), options)
}

#[test]
fn md_replay_headless_direct_zip_and_configuration_identity() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-replay-headless")?;
    let (path, options) = fixture(directory.path())?;
    run(&path, &options)?;
    let zip = directory.path().join("input.zip");
    crate::test_support::write_zip(&zip, &[("inside.md", &std::fs::read(&path)?)])?;
    run(&zip, &options)?;
    let mut no_audio = replay_options(&options);
    no_audio.no_apu = true;
    no_audio.no_sram = true;
    run(&path, &no_audio)?;
    for changed in [
        HeadlessOptions {
            md_region: None,
            ..replay_options(&options)
        },
        HeadlessOptions {
            md_timing: None,
            ..replay_options(&options)
        },
        HeadlessOptions {
            md_controller: None,
            ..replay_options(&options)
        },
    ] {
        assert!(
            run(&path, &changed)
                .unwrap_err()
                .to_string()
                .contains("mismatch")
        );
    }
    let saves = directory.path().join("with-save");
    std::fs::create_dir(&saves)?;
    let (path, options) = fixture_with_save(&saves, true)?;
    let save = path.with_extension("sav");
    let sentinel = vec![0xa7; 511];
    std::fs::write(&save, &sentinel)?;
    assert!(!options.no_sram);
    run(&path, &options)?;
    assert_eq!(std::fs::read(&save)?, sentinel);
    let zip = saves.join("input.zip");
    crate::test_support::write_zip(&zip, &[("inside.md", &std::fs::read(&path)?)])?;
    run(&zip, &options)?;
    assert_eq!(std::fs::read(&save)?, sentinel);
    Ok(())
}

#[test]
fn md_replay_headless_checks_explicit_embedded_and_checkpoint_hashes() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-replay-headless-hashes")?;
    let (path, options) = fixture(directory.path())?;
    let explicit = HeadlessOptions {
        expect_replay_final_hash: Some("00".repeat(32)),
        ..replay_options(&options)
    };
    assert!(
        run(&path, &explicit)
            .unwrap_err()
            .to_string()
            .contains("replay final state hash mismatch")
    );
    for checkpoint in [false, true] {
        let source = ReplayPlayer::load(options.replay_path.as_ref().unwrap())?;
        let mut metadata = source.metadata().clone();
        if checkpoint {
            metadata.checkpoints[0].state_sha256 = [0x55; 32];
        } else {
            metadata.final_state_sha256 = Some([0x55; 32]);
        }
        let bad = directory.path().join(format!("bad-{checkpoint}.zrpl"));
        let mut recorder =
            ReplayRecorder::new_with_metadata(bad.clone(), source.save_state().to_vec(), metadata);
        for frame in source.peek_joypad_frames(0, source.total_frames()) {
            recorder.record_joypad_frame(frame);
        }
        recorder.finish()?;
        let bad_options = HeadlessOptions {
            replay_path: Some(bad),
            ..replay_options(&options)
        };
        let error = run(&path, &bad_options).unwrap_err().to_string();
        assert!(
            error.contains(if checkpoint {
                "replay diverged at checkpoint frame 1"
            } else {
                "embedded final state hash mismatch"
            }),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn md_replay_headless_rejects_conflicting_execution_options_before_loading() {
    let base = HeadlessOptions {
        replay_path: Some("missing.zrpl".into()),
        ..Default::default()
    };
    let options = [
        HeadlessOptions {
            load_state_path: Some("state.mdstate".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            md_save_state_path: Some("out.mdstate".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            audio_trace_path: Some("trace.zmda".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            audio_dump_path: Some("audio.f32".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            screenshot_path: Some("image.png".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            debug_state_path: Some("debug.json".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            replay_peer_path: Some("peer.zrpl".into()),
            ..replay_options(&base)
        },
        HeadlessOptions {
            replay_tail_frames: 1,
            ..replay_options(&base)
        },
        HeadlessOptions {
            trace_opcodes: true,
            ..replay_options(&base)
        },
        HeadlessOptions {
            input_events: crate::cli::parse::parse_input_event_arg("a@1", "--press").unwrap(),
            ..base
        },
    ];
    for options in options {
        let missing = Path::new("missing.md");
        let error = run_md_headless(missing, missing, None, &options)
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("os error") && !error.contains("failed to open replay"),
            "{error}"
        );
    }
}

#[test]
fn md_replay_raw_hash_and_future_pcm_preserve_target_audio_policy() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-replay-audio-policy")?;
    let (path, options) = fixture(directory.path())?;
    let player = ReplayPlayer::load(options.replay_path.as_ref().unwrap())?;
    for rate in [44_100, 48_000, 96_000] {
        let mut results = Vec::new();
        for _ in 0..2 {
            let mut backend = load_backend_from_rom_source(
                ActiveSystem::Md,
                &path,
                &path,
                None,
                BackendLoadConfig {
                    sample_rate: Some(rate),
                    md_region: options.md_region,
                    md_timing: options.md_timing.unwrap(),
                    md_controller: options.md_controller.unwrap(),
                    md_load_save_ram: false,
                    md_host_persistence: false,
                    ..Default::default()
                },
            )?
            .backend;
            backend.set_apu_channel_mutes(&[true; 10]);
            backend.set_apu_sample_generation_enabled(false);
            backend.load_state_from_bytes(player.save_state().to_vec())?;
            assert_eq!(
                backend.encode_replay_hash_state_bytes()?,
                player.save_state()
            );
            let EmuBackend::Md(md) = &backend else {
                unreachable!()
            };
            assert_eq!(md.core().sample_rate(), rate);
            backend.step_frame();
            let mut pcm = Vec::new();
            backend.drain_audio_samples_into(&mut pcm);
            assert!(pcm.is_empty());
            backend.set_apu_sample_generation_enabled(true);
            backend.step_frame();
            backend.drain_audio_samples_into(&mut pcm);
            assert!(!pcm.is_empty() && pcm.iter().all(|sample| *sample == 0.0));
            backend.set_apu_channel_mutes(&[false; 10]);
            backend.step_frame();
            backend.drain_audio_samples_into(&mut pcm);
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            results.push((
                backend.encode_replay_hash_state_bytes()?,
                pcm.iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>(),
            ));
        }
        assert_eq!(results[0], results[1]);
    }
    Ok(())
}

#[test]
fn md_replay_headless_zero_frame_checks_held_origin_checkpoint_and_final_hash() -> anyhow::Result<()>
{
    let directory = crate::test_support::test_directory("md-replay-zero-held-headless")?;
    let (path, options) = fixture(directory.path())?;
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        None,
        BackendLoadConfig {
            md_region: options.md_region,
            md_timing: options.md_timing.unwrap(),
            md_controller: ControllerKind::SixButton,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )?
    .backend;
    backend.apply_replay_input(&ReplayJoypadFrame {
        buttons: 0x75,
        dpad: 6,
        buttons_p2: 0x8a,
        dpad_p2: 9,
        ..Default::default()
    });
    backend.step_frame();
    backend.step_frame();
    let start = backend.encode_replay_start_state_bytes()?;
    let hash = sha256_bytes(&start);
    for failure in [None, Some("checkpoint"), Some("final")] {
        let mut metadata = backend.replay_metadata();
        metadata
            .checkpoints
            .push(zeff_emu_common::replay::ReplayCheckpoint {
                frame: 0,
                state_sha256: if failure == Some("checkpoint") {
                    [0x55; 32]
                } else {
                    hash
                },
            });
        metadata.final_state_sha256 = Some(if failure == Some("final") {
            [0x55; 32]
        } else {
            hash
        });
        let replay = directory
            .path()
            .join(format!("zero-{}.zrpl", failure.unwrap_or("good")));
        ReplayRecorder::new_with_metadata(replay.clone(), start.clone(), metadata).finish()?;
        let mut zero = replay_options(&options);
        zero.replay_path = Some(replay);
        zero.md_controller = Some(ControllerKind::SixButton);
        zero.expect_replay_final_hash = Some(const_hex::encode(hash));
        match failure {
            None => run(&path, &zero)?,
            Some(kind) => {
                let error = run(&path, &zero).unwrap_err().to_string();
                assert!(
                    error.contains(if kind == "checkpoint" {
                        "replay diverged at checkpoint frame 0"
                    } else {
                        "embedded final state hash mismatch"
                    }),
                    "{error}"
                );
            }
        }
    }
    Ok(())
}
