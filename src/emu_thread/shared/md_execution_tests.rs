use super::*;
use crate::debug::DebugUiActions;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::Emulator;

fn backend(header: u8, fault: bool) -> EmuBackend {
    let mut rom = vec![0; 32 * 1024];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    let mut words = Vec::<u16>::new();
    let mut write = |address: u32, value: u8| {
        words.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ]);
    };
    if fault {
        write(0xff0020, 0x5a);
        write(0xb00000, 0xaa);
    } else {
        write(0xa11100, 1);
        write(0xa11200, 1);
        for (offset, value) in [0x21, 0x00, 0x01, 0x34, 0x18, 0xfd].into_iter().enumerate() {
            write(0xa00000 + offset as u32, value);
        }
        write(0xa11200, 1);
        write(0xa11100, 0);
        for value in [0x80, 0x04, 0x90] {
            write(0xc00011, value);
        }
    }
    words.extend([0x5239, 0x00ff, 0x0021, 0x60f8]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("execution-controls.md");
    load_backend_from_rom_source(
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
    )
    .unwrap()
    .backend
}

fn machine(backend: &EmuBackend) -> &Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core()
}

fn deliver(
    backend: &mut EmuBackend,
    frames: usize,
    step: bool,
    continue_requested: bool,
    suspend_after: bool,
    fault: &mut WorkerRuntimeFault,
) -> FrameResult {
    let mut input = md_audio_tests::input(frames, false);
    input.debug_step = step;
    input.debug_continue = continue_requested;
    input.debug_suspend_after_frame = suspend_after;
    input.snapshot.want_debug_info = true;
    input.snapshot.show_instruction_trace = true;
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 10,
        fault,
    )
}

fn assert_machine_and_pcm(backend: &EmuBackend, expected: &mut Emulator, result: &FrameResult) {
    let mut pcm = Vec::new();
    expected.drain_audio_samples_into(&mut pcm);
    assert_eq!(result.audio_samples, pcm);
    assert_eq!(machine(backend), expected);
    assert!(result.runtime_fault.is_none());
}

fn assert_snapshot(result: &FrameResult, suspended: bool) {
    let features = result.ui_data.core_features.as_ref().unwrap();
    assert!(features.supports_execution_controls);
    assert!(features.supports_debugger && features.supports_opcode_history);
    assert!(features.supports_instruction_trace);
    assert!(features.supports_guest_calls && features.supports_replay && features.supports_cheats);
    let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(
        cpu.cpu_state,
        if suspended { "Suspended" } else { "Running" }
    );
    assert!(cpu.breakpoints.is_empty() && cpu.one_shot_breakpoints.is_empty());
    assert!(cpu.watchpoints.is_empty());
    assert!(cpu.recent_opcodes.len() <= 32);
    assert!(
        cpu.recent_opcodes
            .iter()
            .all(|entry| entry.bytes.len() == 2)
    );
    assert_eq!(
        cpu.supported_events,
        [
            zeff_emu_common::debug::DebugEvent::Interrupt,
            zeff_emu_common::debug::DebugEvent::Dma,
            zeff_emu_common::debug::DebugEvent::Exception
        ]
    );
    assert!(cpu.event_breakpoints.is_empty() && cpu.hit_event.is_none());
    assert!(result.ui_data.instruction_trace.is_some());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_execution_step_routes_the_whole_machine_and_preserves_pause() {
    for header in *b"UJE" {
        let mut backend = backend(header, false);
        let mut fault = WorkerRuntimeFault::default();
        let running = deliver(&mut backend, 0, false, false, false, &mut fault);
        assert_snapshot(&running, false);
        let before = machine(&backend).clone();
        let ignored = deliver(&mut backend, 0, true, false, false, &mut fault);
        assert_eq!(machine(&backend), &before);
        assert_snapshot(&ignored, false);
        backend.debug_suspend();
        assert!(backend.is_suspended());
        let mut expected = machine(&backend).clone();
        let sound_before = expected.sound_cpu().clone();
        let mut actions = DebugUiActions::none();
        actions.memory_writes.push((0xff0020, 0xa5));
        actions.add_breakpoint = Some(0x201);
        actions.trace_enabled = Some(true);
        backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
        expected.debug_write_ram(&actions.memory_writes).unwrap();
        assert_eq!(machine(&backend), &expected);
        for _ in 0..256 {
            let now = expected.timing_snapshot().now();
            expected.step_instruction().unwrap();
            assert!(expected.timing_snapshot().now() > now);
            let stepped = deliver(&mut backend, 1, true, false, false, &mut fault);
            assert_eq!(stepped.advanced_frames, 0);
            assert!(backend.is_suspended());
            assert_machine_and_pcm(&backend, &mut expected, &stepped);
            assert_snapshot(&stepped, true);
            let frozen = machine(&backend).clone();
            let paused = deliver(&mut backend, 2, false, false, false, &mut fault);
            assert_eq!(paused.advanced_frames, 0);
            assert_eq!(machine(&backend), &frozen);
            assert!(paused.audio_samples.is_empty());
        }
        assert_ne!(machine(&backend).sound_cpu(), &sound_before);
        for (offset, byte) in [0x21, 0x00, 0x01, 0x34, 0x18, 0xfd].into_iter().enumerate() {
            assert_eq!(machine(&backend).peek(0xa00000 + offset as u32), Some(byte));
        }
        assert_ne!(machine(&backend).peek(0xa00100), Some(0));
        assert_ne!(machine(&backend).peek(0xff0021), Some(0));
        assert_eq!(machine(&backend).peek(0xff0020), Some(0xa5));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_execution_crosses_a_field_boundary_then_continues_exact_pcm_and_state() {
    let mut backend = backend(b'U', false);
    let mut fault = WorkerRuntimeFault::default();
    deliver(&mut backend, 0, false, false, false, &mut fault);
    let mut expected = machine(&backend).clone();
    let threshold = expected.region().frame_ticks() - 300;
    for _ in 0..40_000 {
        if expected.timing_snapshot().now().get() >= threshold {
            break;
        }
        expected.step_instruction().unwrap();
    }
    assert!(expected.timing_snapshot().now().get() >= threshold);
    assert_eq!(expected.pacing_field_count(), 0);
    let mut discarded = Vec::new();
    expected.drain_audio_samples_into(&mut discarded);
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    *md.test_normal_core_mut().unwrap() = expected.clone();
    backend.debug_suspend();
    for _ in 0..128 {
        expected.step_instruction().unwrap();
        let result = deliver(&mut backend, 1, true, false, false, &mut fault);
        assert_machine_and_pcm(&backend, &mut expected, &result);
        assert!(backend.is_suspended());
        if expected.pacing_field_count() == 1 {
            break;
        }
    }
    assert_eq!(expected.pacing_field_count(), 1);
    let frozen = machine(&backend).clone();
    let paused_saved = backend.encode_state_bytes().unwrap();
    assert!(backend.is_suspended());
    assert_eq!(machine(&backend), &frozen);
    let continued = deliver(&mut backend, 0, true, true, false, &mut fault);
    assert_snapshot(&continued, false);
    assert_eq!(machine(&backend), &frozen);
    let saved = backend.encode_state_bytes().unwrap();
    assert_eq!(saved, paused_saved);
    for _ in 0..2 {
        expected.step_field().unwrap();
        let result = deliver(&mut backend, 1, false, false, false, &mut fault);
        assert_eq!(result.advanced_frames, 1);
        assert!(!result.audio_samples.is_empty());
        assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_machine_and_pcm(&backend, &mut expected, &result);
    }
    backend.debug_suspend();
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    assert!(backend.is_suspended());
    backend.load_state_from_bytes(saved.clone()).unwrap();
    assert!(!backend.is_suspended());
    assert_eq!(backend.encode_state_bytes().unwrap(), saved);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_execution_fault_is_delivered_once_and_controls_wait_for_reset_or_restore() {
    let mut backend = backend(b'U', true);
    let mut worker_fault = WorkerRuntimeFault::default();
    deliver(&mut backend, 0, false, false, false, &mut worker_fault);
    let recover = backend.encode_state_bytes().unwrap();
    backend.debug_suspend();
    deliver(&mut backend, 0, true, false, false, &mut worker_fault);
    assert_eq!(machine(&backend).peek(0xff0020), Some(0x5a));
    let mut expected = machine(&backend).clone();
    let expected_fault = expected.step_instruction().unwrap_err();
    let result = deliver(&mut backend, 0, true, false, false, &mut worker_fault);
    assert_eq!(
        result.runtime_fault.unwrap(),
        format!("Mega Drive machine fault: {expected_fault:?}")
    );
    assert_eq!(machine(&backend), &expected);
    assert!(backend.is_suspended());
    assert!(!worker_fault.can_step());
    assert!(backend.take_runtime_fault().is_none());
    for (continue_requested, step) in [(false, true), (true, false), (true, true)] {
        let actions = DebugUiActions::none();
        let mut config = BackendRuntimeConfig::new(&actions);
        config.debug_continue = continue_requested;
        config.debug_step = step;
        backend.apply_runtime_config(config);
        assert!(backend.is_suspended());
        assert_eq!(machine(&backend), &expected);
        let result = deliver(
            &mut backend,
            1,
            step,
            continue_requested,
            false,
            &mut worker_fault,
        );
        assert!(result.runtime_fault.is_none());
        assert_eq!(result.advanced_frames, 0);
    }
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    assert!(backend.is_suspended());
    backend.load_state_from_bytes(recover.clone()).unwrap();
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
    assert_eq!(backend.encode_state_bytes().unwrap(), recover);
    backend.debug_suspend();
    let mut worker_fault = WorkerRuntimeFault::default();
    deliver(&mut backend, 0, true, false, false, &mut worker_fault);
    deliver(&mut backend, 0, true, false, false, &mut worker_fault);
    backend.reset();
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
    assert_eq!(backend.encode_state_bytes().unwrap(), recover);
    backend.debug_suspend();
    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    assert!(!backend.is_suspended());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_execution_existing_next_frame_route_resumes_exactly_one_field_and_pauses() {
    let mut backend = backend(b'U', false);
    let mut fault = WorkerRuntimeFault::default();
    deliver(&mut backend, 0, false, false, false, &mut fault);
    backend.debug_suspend();
    let mut expected = machine(&backend).clone();
    expected.step_field().unwrap();
    let result = deliver(&mut backend, 1, false, true, true, &mut fault);
    assert_eq!(result.advanced_frames, 1);
    assert!(backend.is_suspended());
    assert_machine_and_pcm(&backend, &mut expected, &result);
    assert_snapshot(&result, true);
    let paused = deliver(&mut backend, 1, false, false, false, &mut fault);
    assert_eq!(paused.advanced_frames, 0);
    assert_eq!(machine(&backend), &expected);
}
