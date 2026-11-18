use super::super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll};
use anyhow::{Context, ensure};
use std::time::{Duration, Instant};
use zeff_md_core::Emulator;

const STORE: u32 = 0x218;

fn backend() -> Result<EmuBackend> {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    let words = [
        0x13fc_u16, 0x80, 0xc0, 0x11, 0x13fc, 4, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239,
        0xff, 0x40, 0x60f8,
    ];
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("pc-breakpoint-app.md");
    Ok(load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(44_100),
            ..Default::default()
        },
    )?
    .backend)
}

#[test]
fn md_pc_breakpoint_actual_app_tick_native_worker_first_witness() {
    let backend = backend().unwrap();
    let EmuBackend::Md(md) = &backend else {
        unreachable!()
    };
    let mut expected = md.core().clone();
    let mut settings = Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 44_100;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    settings.rewind.enabled = false;
    let mut app = construct::create(Some(backend), settings, None, false);
    let backend = app.initial_backend.take().unwrap();
    let path = std::path::PathBuf::from("pc-breakpoint-app.md");
    app.finalize_rom_load(&backend, ActiveSystem::Md, path.clone(), path);
    app.spawn_emu_thread(backend);
    app.set_user_paused(true);
    let result = exercise(&mut app, &mut expected);
    app.stop_emu_thread();
    result.unwrap();
}

fn exercise(app: &mut App, expected: &mut Emulator) -> Result<()> {
    app.pending_debug_actions.add_breakpoint = Some(0x200);
    app.debug_requests.frame_advance = true;
    deliver(app, expected, 0, true, Some(0x200))?;
    app.pending_debug_actions.remove_breakpoints.push(0x200);
    app.pending_debug_actions.add_breakpoint = Some(STORE);
    app.debug_requests.continue_ = true;
    app.debug_requests.step = true;
    deliver(app, expected, 0, false, None)?;
    for _ in 0..3 {
        step(expected)?;
    }
    ensure!(expected.registers().pc == STORE, "fixture store PC differs");
    app.debug_requests.frame_advance = true;
    deliver(app, expected, 0, true, Some(STORE))?;
    ensure!(
        expected.peek(0xff0040) == Some(0),
        "store executed before hit"
    );
    step(expected)?;
    app.debug_requests.step = true;
    deliver(app, expected, 0, true, None)?;
    ensure!(
        expected.peek(0xff0040) == Some(1),
        "Step did not execute store"
    );
    app.debug_requests.continue_ = true;
    deliver(app, expected, 0, false, None)?;
    step(expected)?;
    app.debug_requests.frame_advance = true;
    deliver(app, expected, 0, true, Some(STORE))?;
    app.pending_debug_actions.remove_breakpoints.push(STORE);
    app.debug_requests.continue_ = true;
    deliver(app, expected, 0, false, None)?;
    expected
        .step_field()
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    app.debug_requests.frame_advance = true;
    deliver(app, expected, 1, false, None)?;
    ensure!(
        capture(app)? == expected.encode_state()?,
        "raw state differs"
    );

    app.pending_debug_actions.add_one_shot_breakpoint = Some(STORE);
    app.debug_requests.continue_ = true;
    app.debug_requests.next_frame = true;
    for _ in 0..2 {
        if expected.registers().pc == STORE {
            break;
        }
        step(expected)?;
    }
    ensure!(
        expected.registers().pc == STORE,
        "run-to-cursor fixture diverged"
    );
    deliver(app, expected, 0, true, Some(STORE))?;
    let cpu = app
        .cached_ui_data
        .as_ref()
        .unwrap()
        .cpu_debug
        .as_ref()
        .unwrap();
    ensure!(
        cpu.breakpoints.is_empty() && cpu.one_shot_breakpoints.is_empty(),
        "one-shot remained armed"
    );
    app.debug_requests.continue_ = true;
    deliver(app, expected, 0, false, None)?;
    for _ in 0..3 {
        expected
            .step_field()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        app.debug_requests.frame_advance = true;
        ensure!(
            deliver(app, expected, 1, false, None)?,
            "missing nonzero PCM"
        );
        ensure!(
            capture(app)? == expected.encode_state()?,
            "future raw state differs"
        );
    }
    Ok(())
}

fn step(expected: &mut Emulator) -> Result<()> {
    expected
        .step_instruction()
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    Ok(())
}

fn deliver(
    app: &mut App,
    expected: &mut Emulator,
    fields: usize,
    suspended: bool,
    hit: Option<u32>,
) -> Result<bool> {
    ensure!(
        app.speed.paused && app.frames_in_flight == 0,
        "pacing is not serialized"
    );
    app.remote_debug_frames_remaining = 1;
    app.tick();
    ensure!(app.frames_in_flight == 1, "App tick sent no request");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = app.emu_thread.as_ref().unwrap().try_recv_frame() {
            ensure!(
                result.runtime_fault.is_none(),
                "worker fault: {:?}",
                result.runtime_fault
            );
            ensure!(
                result.advanced_frames == fields && result.completed_step_requests == 1,
                "field or delivery accounting differs"
            );
            let cpu = result.ui_data.cpu_debug.as_ref().context("CPU snapshot")?;
            let features = result
                .ui_data
                .core_features
                .as_ref()
                .context("capabilities")?;
            ensure!(
                features.supports_pc_breakpoints
                    && features.supports_execution_controls
                    && features.supports_debugger
                    && features.supports_replay
                    && features.supports_guest_calls
                    && features.supports_opcode_history,
                "capability split differs"
            );
            ensure!(
                cpu.pc == expected.registers().pc && cpu.cycles == expected.cpu_clock().0,
                "CPU state differs"
            );
            ensure!(
                cpu.cpu_state == if suspended { "Suspended" } else { "Running" },
                "suspension differs"
            );
            ensure!(cpu.hit_breakpoint == hit, "breakpoint hit differs");
            let frame = app
                .emu_thread
                .as_ref()
                .unwrap()
                .shared_framebuffer()
                .load_full()
                .context("framebuffer")?;
            ensure!(
                frame.as_slice() == expected.framebuffer(),
                "framebuffer differs"
            );
            let mut pcm = Vec::new();
            expected.drain_audio_samples_into(&mut pcm);
            ensure!(
                pcm.iter()
                    .map(|sample| sample.to_bits())
                    .eq(result.audio_samples.iter().map(|sample| sample.to_bits())),
                "PCM differs"
            );
            let nonzero = pcm.iter().any(|sample| *sample != 0.0);
            app.process_frame_result(result);
            ensure!(app.frames_in_flight == 0, "App did not consume delivery");
            return Ok(nonzero);
        }
        ensure!(Instant::now() < deadline, "worker delivery timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn capture(app: &mut App) -> Result<Vec<u8>> {
    app.send_emu_command_checked(EmuCommand::CaptureStateBytes)
        .map_err(anyhow::Error::msg)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match app.emu_thread.as_ref().unwrap().poll_response() {
            EmuResponsePoll::Response(response) => {
                return match *response {
                    EmuResponse::StateCaptured(bytes) => Ok(bytes),
                    _ => anyhow::bail!("unexpected state response"),
                };
            }
            EmuResponsePoll::Disconnected => anyhow::bail!("worker disconnected"),
            EmuResponsePoll::Empty => {}
        }
        ensure!(Instant::now() < deadline, "capture timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

mod timed_ports;
mod uncapped;
