use super::*;
use crate::cheats::{CheatCode, CheatPatch, CheatType, CheatValue};
use crate::debug::DebugUiActions;
use crate::emu_thread::{FrameResult, GuestCallRequest, MdTimedPortRequest};
use crate::input::HostButton;
use zeff_md_core::emulator::DebugPortWrite;

#[test]
fn md_timed_ports_actual_app_flushes_older_cheat_before_deferred_guest_call() {
    let mut actual = backend().unwrap();
    let mut control = backend().unwrap();
    let code = [0x10, 0x39, 0, 0, 3, 1, 0x4e, 0x75];
    let writes: Vec<_> = code
        .iter()
        .enumerate()
        .map(|(i, byte)| (0xff0200 + i as u32, *byte))
        .collect();
    for backend in [&mut actual, &mut control] {
        backend.debug_suspend();
        let EmuBackend::Md(md) = backend else {
            unreachable!()
        };
        md.test_normal_core_mut()
            .unwrap()
            .debug_write_memory(&writes)
            .unwrap();
        assert_eq!(md.core().registers().pc, 0x200);
        assert_eq!(md.core().registers().address_register(7), Some(0x00fffffc));
        assert_eq!(md.core().peek(0x301), Some(0));
    }
    let mut settings = Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 44_100;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    settings.rewind.enabled = false;
    let mut app = construct::create(Some(actual), settings, None, false);
    let actual = app.initial_backend.take().unwrap();
    let path = std::path::PathBuf::from("pc-breakpoint-app.md");
    app.finalize_rom_load(&actual, ActiveSystem::Md, path.clone(), path);
    app.spawn_emu_thread(actual);
    app.set_user_paused(true);
    let result = exercise_flush(&mut app, &mut control);
    app.stop_emu_thread();
    result.unwrap();
    app.debug_windows
        .md_timed_ports
        .queue(port_request(703))
        .unwrap();
    app.tick();
    assert!(!app.debug_windows.md_timed_ports.ordering_pending());
    assert_eq!(app.settings.rewind.seconds, 7);
}

fn machine(backend: &mut EmuBackend) -> &mut Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.test_normal_core_mut().unwrap()
}

fn port_request(id: u64) -> MdTimedPortRequest {
    MdTimedPortRequest {
        id,
        writes: vec![DebugPortWrite::Word {
            address: 0xc00004,
            value: 0x8f02,
        }],
    }
}

fn exercise_flush(app: &mut App, control: &mut EmuBackend) -> Result<()> {
    deliver(app, machine(control), 0, true, None)?;
    ensure!(!app.debug_requests.has_pending() && !app.pending_debug_actions.has_pending());
    ensure!(
        app.remote_debug_frames_remaining == 0
            && app.remote_memory_frames_remaining == 0
            && app.remote_graphics_frames_remaining == 0
    );
    let patch = CheatPatch::WideRomWrite {
        address: 0x301,
        value: CheatValue::constant(0xa5),
    };
    app.debug_windows.cheat.user_codes.push(CheatCode {
        name: "older overlay".into(),
        code_text: "000301:A5".into(),
        enabled: true,
        parameter_value: None,
        code_type: CheatType::Raw,
        patches: vec![patch],
    });
    app.debug_windows.cheat.cheats_dirty = true;
    app.settings.rewind.seconds = 7;
    app.host_input.set_remote(HostButton::A, true);
    ensure!(app.current_host_joypad_input() == (1, 0));
    let mut actions = DebugUiActions::none();
    actions.md_timed_ports = Some(port_request(701));
    app.merge_debug_actions(actions);
    let call = GuestCallRequest {
        name: "after flush".into(),
        target: 0xff0200,
        storage_offset: None,
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: 2,
    };
    let mut actions = DebugUiActions::none();
    actions.guest_call = Some(call.clone());
    app.merge_debug_actions(actions);
    ensure!(app.debug_windows.md_timed_ports.has_deferred());
    app.tick();
    ensure!(app.frames_in_flight == 1 && app.debug_windows.cheat.cheats_dirty);
    let outcome = machine(control)
        .debug_write_ports(&port_request(701).writes)
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    ensure!(outcome.instructions == 0);
    let timed_frame = next_frame(app)?;
    check_zero_frame(&timed_frame, machine(control))?;
    app.send_emu_command_checked(EmuCommand::CaptureStateBytes)
        .map_err(anyhow::Error::msg)?;
    let Some(EmuResponse::StateCaptured(before)) = app.recv_cold_response() else {
        anyhow::bail!("cold consumer lost the unrelated capture response");
    };
    ensure!(before == control.encode_state_bytes()?);
    ensure!(!app.debug_windows.md_timed_ports.busy());
    ensure!(app.debug_windows.md_timed_ports.ordinary_flush_required());
    app.tick();
    ensure!(app.frames_in_flight == 1 && app.debug_windows.cheat.cheats_dirty);
    ensure!(app.settings.rewind.seconds == 7 && app.debug_windows.md_timed_ports.has_deferred());
    ensure!(app.current_host_joypad_input() == (1, 0));
    app.process_frame_result(timed_frame);
    app.tick();
    ensure!(
        app.frames_in_flight == 1,
        "paused ordinary flush was not sent"
    );
    ensure!(!app.debug_windows.cheat.cheats_dirty);
    ensure!(app.debug_windows.md_timed_ports.ordinary_delivery_pending());
    let flush = next_frame(app)?;
    machine(control)
        .input_mut()
        .set_controller_raw(zeff_md_core::ControllerPort::One, 0xef);
    check_zero_frame(&flush, machine(control))?;
    app.tick();
    ensure!(
        app.frames_in_flight == 1 && app.debug_windows.md_timed_ports.has_deferred(),
        "guest call overtook a held ordinary delivery"
    );
    ensure!(capture(app)? == control.encode_state_bytes()?);
    app.process_frame_result(flush);
    ensure!(!app.debug_windows.md_timed_ports.ordinary_delivery_pending());
    app.tick();
    let Some(EmuResponse::GuestCallCompleted {
        name,
        instructions,
        undo_state,
    }) = app.recv_cold_response()
    else {
        anyhow::bail!("deferred guest call did not complete");
    };
    ensure!(name == "after flush" && instructions == 2 && undo_state.len() == 16);
    control.try_install_rom_patches(&[patch])?;
    ensure!(control.execute_guest_call(&call)?.0 == 2);
    ensure!(machine(control).registers().d[0] == 0x000000a5);
    ensure!(
        capture(app)? == control.encode_state_bytes()?,
        "guest observed an older overlay"
    );
    app.debug_requests.continue_ = true;
    deliver(app, machine(control), 0, false, None)?;
    for _ in 0..3 {
        machine(control)
            .step_field()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        app.debug_requests.frame_advance = true;
        ensure!(
            deliver(app, machine(control), 1, false, None)?,
            "future PCM was silent"
        );
        ensure!(
            capture(app)? == control.encode_state_bytes()?,
            "future raw state differs"
        );
    }
    app.debug_windows
        .md_timed_ports
        .queue(port_request(702))
        .map_err(anyhow::Error::msg)?;
    let mut actions = DebugUiActions::none();
    actions.memory_writes.push((0xff0041, 0x5a));
    app.merge_debug_actions(actions);
    ensure!(app.debug_windows.md_timed_ports.has_deferred());
    app.send_emu_command_checked(EmuCommand::Reset)
        .map_err(anyhow::Error::msg)?;
    ensure!(!app.debug_windows.md_timed_ports.ordering_pending());
    ensure!(app.settings.rewind.seconds == 7);
    Ok(())
}

fn next_frame(app: &App) -> Result<FrameResult> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(frame) = app.emu_thread.as_ref().unwrap().try_recv_frame() {
            return Ok(frame);
        }
        ensure!(Instant::now() < deadline, "worker delivery timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn check_zero_frame(frame: &FrameResult, expected: &mut Emulator) -> Result<()> {
    ensure!(frame.runtime_fault.is_none());
    ensure!(frame.advanced_frames == 0 && frame.completed_step_requests == 1);
    let cpu = frame.ui_data.cpu_debug.as_ref();
    if let Some(cpu) = cpu {
        ensure!(cpu.pc == expected.registers().pc && cpu.cycles == expected.cpu_clock().0);
    }
    let mut pcm = Vec::new();
    expected.drain_audio_samples_into(&mut pcm);
    ensure!(
        pcm.iter()
            .map(|x| x.to_bits())
            .eq(frame.audio_samples.iter().map(|x| x.to_bits()))
    );
    ensure!(frame.audio_semantic_frames.is_empty());
    Ok(())
}
