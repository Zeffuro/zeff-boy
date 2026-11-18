use super::*;
use crate::emu_thread::FrameResult;

fn sram_backend() -> Result<EmuBackend> {
    let EmuBackend::Md(md) = backend()? else {
        unreachable!()
    };
    let mut rom = md.core().rom_bytes().to_vec();
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    let words = [0x5239_u16, 0x20, 1, 0x303c, 40_000, 0x51c8, 0xfffe, 0x60f0];
    for (index, word) in words.into_iter().enumerate() {
        let at = STORE as usize + index * 2;
        rom[at..at + 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("pc-breakpoint-uncapped.md");
    let mut backend = load_backend_from_rom_source(
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
    .backend;
    ensure!(
        backend.save_ram_kind().is_persistent(),
        "fixture has no SRAM"
    );
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(STORE);
    backend.apply_runtime_config(crate::emu_backend::BackendRuntimeConfig::new(&actions));
    Ok(backend)
}

#[test]
fn md_pc_breakpoint_actual_app_uncapped_stop_continue_resumes_autonomous_worker() {
    let backend = sram_backend().unwrap();
    let EmuBackend::Md(md) = &backend else {
        unreachable!()
    };
    let mut expected = md.core().clone();
    expected.set_apu_sample_generation_enabled(false);
    let mut settings = Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 44_100;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    settings.rewind.enabled = false;
    settings.emulation.uncapped_frames_per_tick = 64;
    let mut app = construct::create(Some(backend), settings, None, false);
    let backend = app.initial_backend.take().unwrap();
    let path = std::path::PathBuf::from("pc-breakpoint-uncapped.md");
    app.finalize_rom_load(&backend, ActiveSystem::Md, path.clone(), path);
    app.spawn_emu_thread(backend);
    let result = exercise_uncapped(&mut app, &mut expected);
    app.stop_emu_thread();
    result.unwrap();
}

fn exercise_uncapped(app: &mut App, expected: &mut Emulator) -> Result<()> {
    ensure!(
        !app.speed.paused,
        "global pause would mask uncapped execution"
    );
    app.settings.emulation.uncapped_speed = true;
    app.sync_speed_setting();
    ensure!(
        app.timing.uncapped_worker_enabled,
        "uncapped intent was not sent"
    );
    for _ in 0..3 {
        step(expected)?;
    }
    ensure!(expected.registers().pc == STORE, "initial store PC differs");
    let initial_sram = expected.peek(0x200001).context("SRAM peek")?;
    let first = recv(app)?;
    ensure!(
        first.completed_step_requests == 0 && first.advanced_frames == 0,
        "initial stop was not an autonomous zero-field delivery"
    );
    check_delivery(&first)?;
    app.process_frame_result(first);
    snapshot_at_stop(app, expected)?;

    let before_field = expected.pacing_field_count();
    expected.clear_frame_ready();
    let mut field = before_field;
    for index in 0..50_000 {
        step(expected)?;
        if expected.registers().pc == STORE {
            break;
        }
        if expected.pacing_field_count() != field {
            field = expected.pacing_field_count();
            expected.clear_frame_ready();
        }
        ensure!(index < 49_999, "reference did not return to store");
    }
    let fields = usize::try_from(expected.pacing_field_count() - before_field)?;
    ensure!(
        fields > 1 && fields < 64,
        "delay does not require a bounded autonomous batch"
    );
    ensure!(
        expected.peek(0x200001) == Some(initial_sram.wrapping_add(1)),
        "reference executed more than one SRAM store"
    );

    app.debug_requests.continue_ = true;
    app.remote_debug_frames_remaining = 1;
    app.tick();
    ensure!(
        !app.speed.paused && app.frames_in_flight == 1,
        "Continue request was not sent"
    );
    let mut delivered_fields = 0;
    let mut completed_requests = 0;
    // Only receive after Continue; another App tick could hide lost uncapped intent.
    while delivered_fields < fields || completed_requests == 0 {
        let result = recv(app)?;
        check_delivery(&result)?;
        delivered_fields += result.advanced_frames;
        completed_requests += result.completed_step_requests;
        ensure!(
            delivered_fields <= fields && completed_requests <= 1,
            "execution passed the persistent breakpoint"
        );
        app.process_frame_result(result);
    }
    ensure!(
        app.frames_in_flight == 0 && app.timing.uncapped_worker_enabled,
        "uncapped delivery lost foreground accounting or intent"
    );
    snapshot_at_stop(app, expected)?;
    snapshot_at_stop(app, expected)?;

    // Explicit uncapped-off permits raw capture without advancing the paused boundary.
    app.settings.emulation.uncapped_speed = false;
    app.debug_requests.continue_ = true;
    app.remote_debug_frames_remaining = 1;
    app.tick();
    let result = recv(app)?;
    ensure!(
        result.completed_step_requests == 1 && result.advanced_frames == 0,
        "mode transition advanced a foreground field"
    );
    let cpu = result
        .ui_data
        .cpu_debug
        .as_ref()
        .context("readback CPU snapshot")?;
    ensure!(
        cpu.cpu_state == "Running" && cpu.cycles == expected.cpu_clock().0,
        "raw readback resumed guest execution"
    );
    ensure!(
        !app.speed.paused && !app.timing.uncapped_worker_enabled,
        "explicit uncapped-off did not preserve normal host execution"
    );
    app.process_frame_result(result);
    expected.set_apu_sample_generation_enabled(true);
    ensure!(
        capture(app)? == expected.encode_state()?,
        "full raw machine differs after the uncapped stop/resume cycle"
    );
    Ok(())
}

fn snapshot_at_stop(app: &mut App, expected: &Emulator) -> Result<()> {
    ensure!(
        !app.speed.paused && app.frames_in_flight == 0,
        "foreground snapshot was globally paused or overlapped"
    );
    app.remote_debug_frames_remaining = 1;
    app.remote_memory_view_start = Some(0x200001);
    app.remote_memory_frames_remaining = 1;
    app.tick();
    let result = recv(app)?;
    check_delivery(&result)?;
    ensure!(
        result.completed_step_requests == 1 && result.advanced_frames == 0,
        "snapshot advanced the stopped guest"
    );
    let cpu = result
        .ui_data
        .cpu_debug
        .as_ref()
        .context("stopped CPU snapshot")?;
    ensure!(
        cpu.cpu_state == "Suspended"
            && cpu.hit_breakpoint == Some(STORE)
            && cpu.pc == STORE
            && cpu.cycles == expected.cpu_clock().0,
        "instruction executed after the breakpoint hit"
    );
    let memory = result
        .ui_data
        .memory_page
        .as_ref()
        .context("SRAM memory page")?;
    ensure!(
        memory.first() == Some(&(0x200001, expected.peek(0x200001).unwrap())),
        "SRAM store crossed the stopped boundary"
    );
    let frame = app
        .emu_thread
        .as_ref()
        .unwrap()
        .shared_framebuffer()
        .load_full()
        .context("uncapped framebuffer")?;
    ensure!(
        frame.as_slice() == expected.framebuffer(),
        "uncapped framebuffer differs"
    );
    app.process_frame_result(result);
    ensure!(
        app.frames_in_flight == 0 && app.timing.uncapped_worker_enabled && !app.speed.paused,
        "stopped snapshot lost uncapped intent"
    );
    Ok(())
}

fn check_delivery(result: &FrameResult) -> Result<()> {
    ensure!(
        result.runtime_fault.is_none(),
        "uncapped worker fault: {:?}",
        result.runtime_fault
    );
    ensure!(
        result.audio_samples.is_empty(),
        "uncapped delivery unexpectedly generated PCM"
    );
    Ok(())
}

fn recv(app: &App) -> Result<FrameResult> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = app.emu_thread.as_ref().unwrap().try_recv_frame() {
            return Ok(result);
        }
        ensure!(
            Instant::now() < deadline,
            "uncapped worker did not deliver; cached intent may differ from worker state"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
