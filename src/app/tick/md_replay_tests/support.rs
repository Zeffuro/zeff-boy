use super::*;
use crate::input::HostButton;
use std::time::{Duration, Instant};

pub(super) fn fixture(header: u8, interlace: bool, fault: bool) -> Vec<u8> {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1a4..0x1a8].copy_from_slice(&0xfff_u32.to_be_bytes());
    rom[0x1f0] = header;
    let mut code = vec![0x46fc, 0x2700];
    let mode = if interlace { 0x8c87 } else { 0x8c81 };
    let display = 0x8104 | if header == b'E' { 8 } else { 0 } | if interlace { 0x40 } else { 0 };
    for value in [mode, display, 0xc000, 0] {
        code.extend([0x33fc, value, 0xc0, 4]);
    }
    code.extend([0x33fc, 0x000e, 0xc0, 0]);
    for value in [0x85, 6, 0x90] {
        byte(&mut code, 0xc00011, value);
    }
    for port in [0xa10009, 0xa1000b] {
        byte(&mut code, port, 0x40);
    }
    let start = code.len();
    // Leave enough time between scans to reset the six-button handshake.
    code.extend([0x343c, 2500, 0x51ca, 0xfffe]);
    for (data, destination) in [(0xa10003, 0xff0040), (0xa10005, 0xff0043)] {
        byte(&mut code, data, 0x40);
        sample(&mut code, data, destination, 0x3f);
        byte(&mut code, data, 0);
        sample(&mut code, data, destination + 1, 0x3f);
        for index in 0..3 {
            byte(&mut code, data, 0x40);
            if index != 2 {
                byte(&mut code, data, 0);
            }
        }
        sample(&mut code, data, destination + 2, 0x0f);
    }
    code.extend([0x5239, 0xff, 0x46]);
    if fault {
        code.extend([0x0c39, 20, 0xff, 0x46, 0x6608]);
        byte(&mut code, 0xb00000, 0xaa);
    }
    let displacement = (start as isize - code.len() as isize - 1) * 2;
    code.extend([0x6000, displacement as u16]);
    for (index, value) in code.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&value.to_be_bytes());
    }
    rom
}

fn byte(code: &mut Vec<u16>, address: u32, value: u16) {
    code.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
}

fn sample(code: &mut Vec<u16>, address: u32, destination: u32, mask: u16) {
    code.extend([
        0x1039,
        (address >> 16) as u16,
        address as u16,
        0x0200,
        mask,
        0x13c0,
        (destination >> 16) as u16,
        destination as u16,
    ]);
}

pub(super) fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 48_000;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    settings.rewind.enabled = false;
    settings
}

pub(super) fn load(path: &Path, bytes: Vec<u8>, settings: &Settings) -> Result<EmuBackend> {
    let app = construct::create(None, settings.clone(), None, false);
    Ok(load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(bytes),
        BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(settings.audio.output_sample_rate),
            ..app.backend_load_config(ActiveSystem::Md)
        },
    )?
    .backend)
}

pub(super) fn with_app<T>(
    backend: EmuBackend,
    settings: Settings,
    path: &Path,
    exercise: impl FnOnce(&mut App) -> Result<T>,
) -> Result<T> {
    let mut app = construct::create(None, settings, None, false);
    app.finalize_rom_load(&backend, backend.system(), path.to_owned(), path.to_owned());
    app.set_user_paused(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.spawn_emu_thread(backend);
        exercise(&mut app)
    }));
    app.stop_emu_thread();
    ensure!(app.emu_thread.is_none(), "worker remained live");
    match result {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub(super) fn host(app: &mut App, p1: (u8, u8), p2: (u8, u8)) {
    for button in HostButton::WITH_SIX_BUTTONS {
        let mask = button.host_mask_bit();
        app.host_input.set_remote(
            *button,
            (u16::from(p1.0) << 4 | u16::from(p1.1)) & mask != 0,
        );
        app.host_input.set_remote_p2(
            *button,
            (u16::from(p2.0) << 4 | u16::from(p2.1)) & mask != 0,
        );
    }
}

pub(super) fn field(app: &mut App) -> Result<Vec<f32>> {
    ensure!(app.speed.paused && app.frames_in_flight == 0);
    app.debug_requests.frame_advance = true;
    app.tick();
    ensure!(app.frames_in_flight == 1, "App tick did not send a frame");
    let result = receive(app)?;
    ensure!(
        result.runtime_fault.is_none(),
        "fault: {:?}",
        result.runtime_fault
    );
    ensure!(result.replay_error.is_none(), "replay worker error");
    ensure!(result.advanced_frames == 1 && result.completed_step_requests == 1);
    let pcm = result.audio_samples.clone();
    app.process_frame_result(result);
    ensure!(app.frames_in_flight == 0);
    Ok(pcm)
}

pub(super) fn capture(app: &mut App) -> Result<Vec<u8>> {
    app.send_emu_command_checked(EmuCommand::CaptureStateBytes)?;
    match app.recv_cold_response().context("state capture response")? {
        EmuResponse::StateCaptured(bytes) => Ok(bytes),
        _ => anyhow::bail!("unexpected state capture response"),
    }
}

pub(super) fn wait(app: &mut App, ready: impl Fn(&App) -> bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready(app) {
        app.drain_emu_responses();
        app.poll_replay_save_worker();
        ensure!(Instant::now() < deadline, "replay response timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

pub(super) fn receive(app: &App) -> Result<crate::emu_thread::FrameResult> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = app.emu_thread.as_ref().context("worker")?.try_recv_frame() {
            return Ok(result);
        }
        ensure!(Instant::now() < deadline, "field response timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}
