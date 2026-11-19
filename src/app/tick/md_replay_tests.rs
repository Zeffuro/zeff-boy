use super::super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{EmuCommand, EmuResponse};
use crate::settings::{MdControllerPreference, MdTimingPreference};
use anyhow::{Context, ensure};
use std::path::Path;
use zeff_emu_common::replay::{ReplayJoypadFrame, ReplayPlayer};
use zeff_md_core::Emulator;

mod failures;
mod support;
use support::*;

#[test]
fn md_replay_actual_app_native_all_twelve_configurations() -> Result<()> {
    let mut count = 0;
    for header in *b"UJE" {
        for controller in [
            MdControllerPreference::ThreeButton,
            MdControllerPreference::SixButton,
        ] {
            for timing in [MdTimingPreference::Standard, MdTimingPreference::Precise] {
                run_case(header, controller, timing, false)?;
                count += 1;
            }
        }
    }
    ensure!(count == 12);
    Ok(())
}

#[test]
fn md_replay_actual_app_native_mode2_fields() -> Result<()> {
    run_case(
        b'E',
        MdControllerPreference::SixButton,
        MdTimingPreference::Standard,
        true,
    )
}

fn run_case(
    header: u8,
    controller: MdControllerPreference,
    timing: MdTimingPreference,
    interlace: bool,
) -> Result<()> {
    ensure!(std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1"));
    let directory = crate::test_support::test_directory("md-replay-app")?;
    let path = directory.path().join("input.md");
    let replay = directory.path().join("input.zrpl");
    let mut settings = settings();
    settings.emulation.md_controller = controller;
    settings.emulation.md_timing = timing;
    let backend = load(&path, fixture(header, interlace, false), &settings)?;
    let EmuBackend::Md(md) = &backend else {
        anyhow::bail!("wrong backend");
    };
    let mut oracle = md.core().clone();
    let origin = oracle.pacing_field_count() + 2;
    let height = if header == b'E' { 240 } else { 224 } * if interlace { 2 } else { 1 };
    println!(
        "BEGIN_MD_REPLAY_ACTUAL_APP header={} timing={timing:?} pad={controller:?} mode2={interlace}",
        char::from(header)
    );
    with_app(backend, settings, &path, |app| {
        exercise(app, &mut oracle, &replay, controller, (320, height))
    })
    .with_context(|| {
        format!(
            "header={} timing={timing:?} pad={controller:?} mode2={interlace}",
            char::from(header)
        )
    })?;
    println!(
        "PASS_MD_REPLAY_ACTUAL_APP header={} timing={timing:?} pad={controller:?} mode2={interlace} origin={origin} fields=4",
        char::from(header)
    );
    Ok(())
}

fn exercise(
    app: &mut App,
    oracle: &mut Emulator,
    path: &Path,
    controller: MdControllerPreference,
    dimensions: (u32, u32),
) -> Result<()> {
    let cold_origin = oracle.pacing_field_count();
    let origin = cold_origin + 2;
    host(app, (0x75, 6), (0x8a, 9));
    field(app).context("warm field 1")?;
    field(app).context("warm field 2")?;
    let start = capture(app).context("capture held origin")?;
    oracle.decode_state(&start)?;
    ensure!(oracle.pacing_field_count() - cold_origin == 2);
    ensure!(
        oracle
            .input()
            .controller_raw(zeff_md_core::ControllerPort::One)
            != 0xff
    );
    ensure!(
        oracle
            .input()
            .controller_raw(zeff_md_core::ControllerPort::Two)
            != 0xff
    );
    ensure!(
        oracle
            .input()
            .controller_extra_raw(zeff_md_core::ControllerPort::One)
            == 2
    );
    ensure!(
        oracle
            .input()
            .controller_extra_raw(zeff_md_core::ControllerPort::Two)
            == 13
    );
    app.start_replay_recording_to_path(path.to_owned())?;
    wait(app, |app| !app.recording.is_replay_start_pending())
        .context("acknowledge recording origin")?;
    ensure!(
        app.recording.replay_recorder.is_some(),
        "recording did not start"
    );
    ensure!(app.recording.replay_recording_origin.frame == origin);
    let cases = [
        ((0x41, 4), (0x82, 1), [0x2e, 0x22, 15, 0x17, 0x33, 13]),
        ((0x34, 2), (0x88, 8), [0x3b, 0x33, 2, 0x3d, 0x11, 13]),
        ((0xff, 0), (0, 0), [15, 3, 0, 0x3f, 0x33, 15]),
        ((0, 0), (0, 0), [0x3f, 0x33, 15, 0x3f, 0x33, 15]),
    ];
    let mut states = Vec::new();
    let mut frames = Vec::new();
    let mut transcript = Vec::new();
    for (index, (p1, p2, mut guest)) in cases.into_iter().enumerate() {
        if controller == MdControllerPreference::ThreeButton {
            guest[2] = guest[0] & 15;
            guest[5] = guest[3] & 15;
        }
        host(app, p1, p2);
        ensure!(
            field(app)
                .with_context(|| format!("record field {}", index + 1))?
                .iter()
                .any(|sample| *sample != 0.0),
            "silent guest"
        );
        let state = capture(app)?;
        oracle.decode_state(&state)?;
        ensure!(oracle.pacing_field_count() == origin + 1 + index as u64);
        ensure!(
            oracle.work_ram()[0x40..0x46] == guest,
            "guest pad scan differs at {index}"
        );
        let frame = app.latest_frame.as_ref().context("published frame")?;
        ensure!(frame.dimensions() == Some(dimensions));
        ensure!(frame.as_chunks::<4>().0.iter().any(|pixel| pixel[0] != 0));
        frames.push(frame.as_slice().to_vec());
        states.push(state);
        transcript.push(ReplayJoypadFrame {
            buttons: p1.0,
            dpad: p1.1,
            buttons_p2: p2.0,
            dpad_p2: p2.1,
            ..Default::default()
        });
        app.send_emu_command_checked(EmuCommand::CaptureReplayCheckpoint {
            frame: index as u64 + 1,
        })?;
        let response = app.recv_cold_response().context("checkpoint response")?;
        ensure!(matches!(
            response,
            EmuResponse::ReplayCheckpointCaptured { .. }
        ));
        ensure!(app.consume_replay_checkpoint_response(response).is_none());
    }
    app.stop_replay_recording()?;
    wait(app, |app| !app.recording.is_replay_finalizing()).context("finalize recorded movie")?;
    let player = ReplayPlayer::load(path)?;
    ensure!(player.version() == 2 && player.total_frames() == cases.len());
    ensure!(player.save_state() == start);
    ensure!(player.metadata().system.as_deref() == Some("md"));
    ensure!(player.metadata().checkpoints.len() == cases.len());
    ensure!(player.peek_joypad_frames(0, cases.len()) == transcript);
    ensure!(
        player.metadata().final_state_sha256
            == Some(zeff_firmware::sha256_bytes(states.last().unwrap())),
        "final hash differs"
    );
    field(app).context("advance target before restore")?;
    ensure!(capture(app)? != start, "restore target did not advance");
    let mut cold_pcm = None;
    for cold in 1..=2 {
        app.play_replay(ReplayPlayer::load(path)?)
            .with_context(|| format!("cold restore {cold}"))?;
        ensure!(
            capture(app)? == start,
            "start restore differs at cold restore {cold}"
        );
        let mut pcm = Vec::new();
        for (index, (state, frame)) in states.iter().zip(&frames).enumerate() {
            host(app, (0, 0), (0xff, 15));
            pcm.extend(
                field(app)
                    .with_context(|| format!("cold replay {cold} field {}", index + 1))?
                    .iter()
                    .map(|sample| sample.to_bits()),
            );
            wait(app, |app| {
                app.recording.pending_replay_checkpoint_hashes.is_empty()
            })
            .with_context(|| format!("cold replay {cold} checkpoint {}", index + 1))?;
            ensure!(&capture(app)? == state, "replayed raw state differs");
            ensure!(app.latest_frame.as_ref().unwrap().as_slice() == frame);
        }
        ensure!(
            app.recording.replay_player.is_none(),
            "playback did not finish"
        );
        ensure!(app.recording.pending_replay_batches.is_empty());
        if let Some(expected) = &cold_pcm {
            ensure!(&pcm == expected, "cold replay PCM differs");
        } else {
            ensure!(!pcm.is_empty(), "no replay PCM");
            cold_pcm = Some(pcm);
        }
    }
    Ok(())
}
