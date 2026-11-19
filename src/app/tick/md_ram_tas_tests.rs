use super::super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll};
use anyhow::{Context, ensure};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator, Region};

const PCS: [u32; 10] = [
    0x208, 0x210, 0x218, 0x220, 0x226, 0x22c, 0x234, 0x23a, 0x240, 0x246,
];

fn fixture(header: u8) -> Vec<u8> {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1a4..0x1a8].copy_from_slice(&0x3ff_u32.to_be_bytes());
    rom[0x1f0] = header;
    let words = [
        0x13fc_u16, 0x80, 0xc0, 0x11, 0x13fc, 4, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x13fc, 1,
        0xff, 0x8000, 0x4af9, 0xff, 0x8000, 0x1039, 0xff, 0x8000, 0x13fc, 0x80, 0xff, 0x8001,
        0x4af9, 0xff, 0x8001, 0x1239, 0xff, 0x8001, 0x5239, 0xff, 0x40, 0x60d0,
    ];
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

#[test]
fn md_ram_tas_actual_app_tick_native_worker_six_inputs() -> Result<()> {
    ensure!(
        std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1"),
        "qualification requires muted host audio"
    );
    let mut count = 0;
    for (header, region) in [
        (b'U', Region::NtscUs),
        (b'J', Region::NtscJapan),
        (b'E', Region::Pal),
    ] {
        for controller in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
            run_case(header, region, controller)
                .with_context(|| format!("region {region:?}, controller {controller:?}"))?;
            count += 1;
        }
    }
    ensure!(count == 6, "fixture input count differs");
    Ok(())
}

fn run_case(header: u8, region: Region, controller: ControllerKind) -> Result<()> {
    let path = PathBuf::from(format!("ram-tas-{header}-{controller:?}.md"));
    let mut settings = Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 48_000;
    settings.emulation.md_controller = match controller {
        ControllerKind::ThreeButton => crate::settings::MdControllerPreference::ThreeButton,
        ControllerKind::SixButton => crate::settings::MdControllerPreference::SixButton,
    };
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    settings.rewind.enabled = true;
    settings.rewind.seconds = 1;
    let mut app = construct::create(None, settings, None, false);
    let backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        Some(fixture(header)),
        BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(48_000),
            ..app.backend_load_config(ActiveSystem::Md)
        },
    )?
    .backend;
    let EmuBackend::Md(md) = &backend else {
        anyhow::bail!("loader returned another system");
    };
    ensure!(md.core().region() == region, "header region differs");
    for port in [ControllerPort::One, ControllerPort::Two] {
        ensure!(
            md.core().input().controller_kind(port) == controller,
            "loaded controller differs"
        );
    }
    let mut expected = md.core().clone();
    ensure!(
        crate::emu_thread::REWIND_CAPTURE_INTERVAL_FRAMES == 4
            && app.settings.rewind.capture_interval() == 4,
        "rewind cadence differs"
    );
    ensure!(!app.live_control.is_enabled(), "live control enabled");
    app.finalize_rom_load(&backend, ActiveSystem::Md, path.clone(), path);
    app.set_user_paused(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.spawn_emu_thread(backend);
        exercise(&mut app, &mut expected)
    }));
    app.stop_emu_thread();
    match result {
        Ok(result) => result?,
        Err(payload) => std::panic::resume_unwind(payload),
    }
    ensure!(app.emu_thread.is_none(), "worker was not retired");
    println!(
        "PASS_ACTUAL_APP_RAM_TAS region={region:?} controller={controller:?} steps=10 fields=8 rewind=4 future=4"
    );
    Ok(())
}

fn exercise(app: &mut App, expected: &mut Emulator) -> Result<()> {
    let mut pcm = (0_usize, false);
    ensure!(expected.registers().pc == 0x200, "entry PC differs");
    ensure!(expected.cpu_clock().0 == 8, "primed CPU clock differs");
    app.pending_debug_actions.add_one_shot_breakpoint = Some(0x200);
    app.debug_requests.frame_advance = true;
    delivery(app, expected, 0, (true, Some(0x200)), 0xff8000, &mut pcm)?;
    raw_matches(app, expected)?;
    for (index, pc) in PCS.into_iter().enumerate() {
        expected
            .step_instruction()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        ensure!(expected.registers().pc == pc, "literal step PC differs");
        literal_step(expected, index)?;
        let memory = if index == 9 { 0xff0040 } else { 0xff8000 };
        app.debug_requests.step = true;
        delivery(app, expected, 0, (true, None), memory, &mut pcm)?;
        raw_matches(app, expected)?;
    }
    ensure!(expected.registers().d[0] & 255 == 1, "D0 low byte differs");
    ensure!(
        expected.registers().d[1] & 255 == 0x80,
        "D1 low byte differs"
    );
    for (address, value) in [(0xff8000, 1), (0xff8001, 0x80), (0xff0040, 1)] {
        ensure!(expected.peek(address) == Some(value), "literal RAM differs");
    }
    app.debug_requests.continue_ = true;
    delivery(app, expected, 0, (false, None), 0xff8000, &mut pcm)?;
    raw_matches(app, expected)?;
    for _ in 0..4 {
        advance(app, expected, &mut pcm)?;
    }
    let clone_a = expected.clone();
    let raw_a = raw_matches(app, expected)?;
    let frame_a = expected.framebuffer().to_vec();
    for _ in 0..4 {
        advance(app, expected, &mut pcm)?;
    }
    let raw_b = raw_matches(app, expected)?;
    ensure!(raw_a != raw_b, "rewind states did not advance");
    idle(app)?;
    app.rewind.frames_rewound = 0;
    app.rewind.pending = true;
    app.send_emu_command_checked(EmuCommand::Rewind(1))
        .map_err(anyhow::Error::msg)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rewind.pending {
        app.drain_emu_responses();
        ensure!(Instant::now() < deadline, "rewind response timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
    ensure!(app.rewind.frames_rewound == 4, "rewind distance differs");
    idle(app)?;
    ensure!(capture(app)? == raw_a, "rewind raw differs from A");
    ensure!(
        app.latest_frame
            .as_ref()
            .context("rewind frame")?
            .as_slice()
            == frame_a.as_slice(),
        "rewind framebuffer differs from A"
    );
    *expected = clone_a;
    for _ in 0..4 {
        advance(app, expected, &mut pcm)?;
    }
    ensure!(pcm.0 > 0 && pcm.1, "PCM is empty or silent");
    idle(app)
}

fn literal_step(expected: &Emulator, index: usize) -> Result<()> {
    let registers = expected.registers();
    ensure!(
        registers.d[0] == u32::from(index >= 5),
        "literal D0 differs"
    );
    ensure!(
        registers.d[1] == if index >= 8 { 0x80 } else { 0 },
        "literal D1 differs"
    );
    for (address, value) in [
        (0xff8000, u8::from(index >= 3)),
        (0xff8001, if index >= 6 { 0x80 } else { 0 }),
        (0xff0040, u8::from(index == 9)),
    ] {
        ensure!(
            expected.peek(address) == Some(value),
            "literal step RAM differs"
        );
    }
    let sr = [
        0x2708, 0x2700, 0x2708, 0x2700, 0x2700, 0x2700, 0x2708, 0x2708, 0x2708, 0x2700,
    ];
    ensure!(
        registers.status_register() == sr[index],
        "literal step flags differ"
    );
    Ok(())
}

fn advance(app: &mut App, expected: &mut Emulator, pcm: &mut (usize, bool)) -> Result<()> {
    expected
        .step_field()
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    app.debug_requests.frame_advance = true;
    delivery(app, expected, 1, (false, None), 0xff8000, pcm)?;
    raw_matches(app, expected)?;
    Ok(())
}

fn delivery(
    app: &mut App,
    expected: &mut Emulator,
    fields: usize,
    suspension: (bool, Option<u32>),
    memory_start: u32,
    pcm_total: &mut (usize, bool),
) -> Result<()> {
    let (suspended, hit) = suspension;
    idle(app)?;
    app.remote_debug_frames_remaining = 1;
    app.remote_memory_frames_remaining = 1;
    app.remote_memory_view_start = Some(memory_start);
    app.tick();
    ensure!(app.frames_in_flight == 1, "tick request count differs");
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        if let Some(result) = app.emu_thread.as_ref().context("worker")?.try_recv_frame() {
            break result;
        }
        ensure!(Instant::now() < deadline, "worker delivery timeout");
        std::thread::sleep(Duration::from_millis(1));
    };
    ensure!(
        result.runtime_fault.is_none(),
        "fault: {:?}",
        result.runtime_fault
    );
    ensure!(
        result.advanced_frames == fields
            && result.completed_step_requests == 1
            && !result.delivery_merged,
        "delivery accounting differs"
    );
    let cpu = result.ui_data.cpu_debug.as_ref().context("CPU snapshot")?;
    let registers = expected.registers();
    ensure!(
        cpu.pc == registers.pc && cpu.cycles == expected.cpu_clock().0,
        "CPU differs"
    );
    ensure!(
        cpu.cpu_state == if suspended { "Suspended" } else { "Running" },
        "suspension differs"
    );
    ensure!(cpu.hit_breakpoint == hit, "breakpoint hit differs");
    ensure!(
        cpu.breakpoints.is_empty() && cpu.one_shot_breakpoints.is_empty(),
        "breakpoint remained"
    );
    let mut lines = Vec::new();
    for index in (0..8).step_by(2) {
        lines.push(format!(
            "D{index}:{:08X}  D{}:{:08X}",
            registers.d[index],
            index + 1,
            registers.d[index + 1]
        ));
    }
    for index in (0..8).step_by(2) {
        lines.push(format!(
            "A{index}:{:08X}  A{}:{:08X}",
            registers.address_register(index).context("An")?,
            index + 1,
            registers.address_register(index + 1).context("An")?
        ));
    }
    lines.push(format!(
        "USP:{:08X}  SSP:{:08X}",
        registers.usp, registers.ssp
    ));
    lines.push(format!(
        "PC:{:08X}  SR:{:04X}",
        registers.pc,
        registers.status_register()
    ));
    ensure!(cpu.register_lines == lines, "register snapshot differs");
    let flags: Vec<_> = [('X', 16), ('N', 8), ('Z', 4), ('V', 2), ('C', 1)]
        .map(|(flag, mask)| (flag, registers.status_register() & mask != 0))
        .into();
    ensure!(cpu.flags == flags, "flag snapshot differs");
    let features = result
        .ui_data
        .core_features
        .as_ref()
        .context("capabilities")?;
    ensure!(
        features.supports_execution_controls
            && features.supports_pc_breakpoints
            && features.supports_debugger
            && features.supports_replay
            && features.supports_guest_calls
            && features.supports_opcode_history,
        "capability split differs"
    );
    let memory = result
        .ui_data
        .memory_page
        .as_ref()
        .context("RAM snapshot")?;
    for address in [memory_start, memory_start + 1] {
        let value = memory
            .iter()
            .find(|(entry, _)| *entry == address)
            .context("RAM address")?
            .1;
        ensure!(
            Some(value) == expected.peek(address),
            "RAM snapshot differs"
        );
    }
    let frame = app
        .emu_thread
        .as_ref()
        .context("worker")?
        .shared_framebuffer()
        .load_full()
        .context("worker framebuffer")?;
    ensure!(
        frame.as_slice() == expected.framebuffer(),
        "framebuffer differs"
    );
    let (width, height) = expected.framebuffer_dimensions();
    ensure!(
        frame.dimensions() == Some((width as u32, height as u32)),
        "frame dimensions differ"
    );
    let mut pcm = Vec::new();
    expected.drain_audio_samples_into(&mut pcm);
    ensure!(
        pcm.iter()
            .map(|sample| sample.to_bits())
            .eq(result.audio_samples.iter().map(|sample| sample.to_bits())),
        "PCM bits differ"
    );
    pcm_total.0 += pcm.len();
    pcm_total.1 |= pcm.iter().any(|sample| *sample != 0.0);
    app.process_frame_result(result);
    ensure!(
        app.latest_frame
            .as_ref()
            .context("App framebuffer")?
            .as_slice()
            == expected.framebuffer(),
        "App result framebuffer differs"
    );
    idle(app)
}

fn idle(app: &App) -> Result<()> {
    ensure!(
        app.speed.paused
            && app.frames_in_flight == 0
            && !app.rewind.pending
            && !app.rewind.backstep_pending,
        "pacing is not idle"
    );
    let thread = app.emu_thread.as_ref().context("worker")?;
    ensure!(
        thread.try_recv_frame().is_none(),
        "unexpected frame delivery"
    );
    ensure!(
        matches!(thread.poll_response(), EmuResponsePoll::Empty),
        "unexpected response or disconnect"
    );
    Ok(())
}

fn response(app: &mut App) -> Result<EmuResponse> {
    idle(app)?;
    app.send_emu_command_checked(EmuCommand::CaptureStateBytes)
        .map_err(anyhow::Error::msg)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match app.emu_thread.as_ref().context("worker")?.poll_response() {
            EmuResponsePoll::Response(response) => return Ok(*response),
            EmuResponsePoll::Disconnected => anyhow::bail!("worker disconnected"),
            EmuResponsePoll::Empty => {}
        }
        ensure!(Instant::now() < deadline, "state response timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn capture(app: &mut App) -> Result<Vec<u8>> {
    let EmuResponse::StateCaptured(bytes) = response(app)? else {
        anyhow::bail!("state capture failed");
    };
    idle(app)?;
    ensure!(
        bytes.len() >= 12 && &bytes[..8] == b"ZMDRAW01",
        "not raw MD state"
    );
    ensure!(
        u32::from_le_bytes(bytes[8..12].try_into()?) == 1,
        "not current MD state"
    );
    Ok(bytes)
}

fn raw_matches(app: &mut App, expected: &Emulator) -> Result<Vec<u8>> {
    let bytes = capture(app)?;
    ensure!(bytes == expected.encode_state()?, "full raw state differs");
    Ok(bytes)
}
