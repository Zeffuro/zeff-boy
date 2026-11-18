use super::*;
use crate::debug::DebugUiActions;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use zeff_emu_common::time::Reset;
use zeff_md_core::Emulator;

mod rom_breakpoints;
pub(super) mod trace_state;

pub(super) fn fixture(header: u8, program: &[u16]) -> EmuBackend {
    fixture_with_timing(header, program, zeff_md_core::TimingModel::Standard)
}

pub(super) fn fixture_with_timing(
    header: u8,
    program: &[u16],
    timing: zeff_md_core::TimingModel,
) -> EmuBackend {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    for vector in [9, 30] {
        rom[vector * 4..vector * 4 + 4].copy_from_slice(&0x300_u32.to_be_bytes());
    }
    rom[0x300..0x304].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("pc-breakpoints.md");
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_timing: timing,
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(44_100),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        false,
        &mut WorkerRuntimeFault::default(),
    );
    backend
}

pub(super) fn machine(backend: &EmuBackend) -> &Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core()
}

pub(super) fn machine_mut(backend: &mut EmuBackend) -> &mut Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.test_normal_core_mut().unwrap()
}

pub(super) fn deliver(
    backend: &mut EmuBackend,
    frames: usize,
    actions: DebugUiActions,
    step: bool,
    continue_requested: bool,
    fault: &mut WorkerRuntimeFault,
) -> FrameResult {
    let mut input = md_audio_tests::input(frames, false);
    input.debug_actions = actions;
    input.debug_step = step;
    input.debug_continue = continue_requested;
    input.snapshot.want_debug_info = true;
    input.snapshot.show_disassembler = true;
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

pub(super) fn compare(
    backend: &EmuBackend,
    expected: &mut Emulator,
    result: &FrameResult,
    fields: usize,
) {
    assert!(result.runtime_fault.is_none());
    assert_eq!(result.advanced_frames, fields);
    let mut pcm = Vec::new();
    expected.drain_audio_samples_into(&mut pcm);
    assert!(
        result
            .audio_samples
            .iter()
            .map(|sample| sample.to_bits())
            .eq(pcm.iter().map(|sample| sample.to_bits()))
    );
    assert_eq!(machine(backend), expected);
    assert_eq!(
        machine(backend).encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
}

fn actions(address: u32, one_shot: bool) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    if one_shot {
        actions.add_one_shot_breakpoint = Some(address);
    } else {
        actions.add_breakpoint = Some(address);
    }
    actions
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoints_stop_before_work_resume_once_and_publish_narrow_capability() {
    for header in *b"UJE" {
        let mut backend = fixture(header, &[0x5239, 0xff, 0x40, 0x60f8, 0x4e71]);
        let mut expected = machine(&backend).clone();
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(
            &mut backend,
            8,
            actions(0x200, false),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert!(backend.is_suspended());
        let caps = result.ui_data.core_features.unwrap();
        assert!(caps.supports_pc_breakpoints && caps.supports_execution_controls);
        assert!(
            caps.supports_debugger
                && caps.supports_opcode_history
                && caps.supports_guest_calls
                && caps.supports_replay
                && caps.supports_cheats
        );
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_breakpoint, Some(0x200));
        assert_eq!(cpu.breakpoints, [0x200]);
        let disassembly = result.ui_data.disassembly_view.unwrap();
        assert_eq!(disassembly.breakpoints, [0x200]);
        assert_eq!(expected.peek(0xff0040), Some(0));

        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            true,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert!(!backend.is_suspended());
        for _ in 0..2 {
            expected.step_instruction().unwrap();
        }
        let result = deliver(
            &mut backend,
            8,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(expected.peek(0xff0040), Some(1));
        assert!(backend.is_suspended());
        expected.step_instruction().unwrap();
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert!(backend.is_suspended());
        assert_eq!(result.ui_data.cpu_debug.unwrap().hit_breakpoint, None);
        assert_eq!(expected.peek(0xff0040), Some(2));
    }
    let mut backend = fixture(b'U', &[0x60fe, 0x4e71]);
    machine_mut(&mut backend).step_field().unwrap();
    assert!(machine(&backend).frame_ready());
    let mut expected = machine(&backend).clone();
    let pc = expected.registers().pc & 0xff_ffff;
    let result = deliver(
        &mut backend,
        1,
        actions(pc, false),
        false,
        false,
        &mut WorkerRuntimeFault::default(),
    );
    compare(&backend, &mut expected, &result, 0);
    assert!(machine(&backend).frame_ready());
    assert_eq!(result.ui_data.cpu_debug.unwrap().hit_breakpoint, Some(pc));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoint_self_branch_skips_once_and_failed_restore_preserves_resume() {
    let mut backend = fixture(b'U', &[0x60fe, 0x4e71]);
    let raw = backend.encode_state_bytes().unwrap();
    let mut expected = machine(&backend).clone();
    let mut fault = WorkerRuntimeFault::default();
    let result = deliver(
        &mut backend,
        1,
        actions(0x200, false),
        false,
        false,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 0);
    deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        true,
        &mut fault,
    );
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    expected.step_instruction().unwrap();
    let result = deliver(
        &mut backend,
        4,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 0);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        true,
        &mut fault,
    );
    backend.load_state_from_bytes(raw).unwrap();
    let frozen = machine(&backend).clone();
    let result = deliver(
        &mut backend,
        4,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(machine(&backend), &frozen);
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoint_one_shot_and_unreachable_field_preserve_state_and_pcm() {
    for header in *b"UJE" {
        let program = [
            0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 4, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239,
            0xff, 0x40, 0x60f8,
        ];
        let mut backend = fixture(header, &program);
        let mut expected = machine(&backend).clone();
        let mut fault = WorkerRuntimeFault::default();
        for _ in 0..3 {
            expected.step_instruction().unwrap();
        }
        let mut input_actions = actions(0x218, true);
        input_actions.add_breakpoint = Some(0x3fe);
        let result = deliver(&mut backend, 12, input_actions, false, false, &mut fault);
        compare(&backend, &mut expected, &result, 0);
        let cpu = result.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_breakpoint, Some(0x218));
        assert!(cpu.one_shot_breakpoints.is_empty());
        assert_eq!(cpu.breakpoints, [0x3fe]);
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            true,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        for _ in 0..3 {
            expected.step_field().unwrap();
            let result = deliver(
                &mut backend,
                1,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
            compare(&backend, &mut expected, &result, 1);
        }
        let mut remove = DebugUiActions::none();
        remove.toggle_breakpoints.push(0x3fe);
        let result = deliver(&mut backend, 0, remove, false, false, &mut fault);
        assert!(result.ui_data.cpu_debug.unwrap().breakpoints.is_empty());
        expected.step_field().unwrap();
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoints_reject_invalid_and_unsupported_actions_without_mutation() {
    let mut backend = fixture(b'U', &[0x60fe, 0x4e71]);
    let mut expected = machine(&backend).clone();
    let mut fault = WorkerRuntimeFault::default();
    let mut invalid = actions(0x201, false);
    invalid.add_one_shot_breakpoint = Some(0x1000200);
    invalid.add_breakpoint_after = Some((0x1000200, 1));
    invalid.toggle_breakpoints = vec![0x201, 0x1000200];
    invalid.memory_writes.push((0xff0040, 0x55));
    invalid.add_watchpoint = Some((0, 0x1000000, zeff_emu_common::debug::WatchType::ReadWrite));
    invalid.add_rom_breakpoints.push(u64::MAX);
    invalid.trace_enabled = Some(true);
    let result = deliver(&mut backend, 0, invalid, false, false, &mut fault);
    compare(&backend, &mut expected, &result, 0);
    let cpu = result.ui_data.cpu_debug.unwrap();
    assert!(
        cpu.breakpoints.is_empty() && cpu.watchpoints.is_empty() && cpu.rom_breakpoints.is_empty()
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
    assert!(result.ui_data.instruction_trace.is_none());

    let mut alias = fixture(b'U', &[0x4ef9, 0x0100, 0x0200, 0x4e71]);
    alias.debug_suspend();
    let result = deliver(
        &mut alias,
        0,
        DebugUiActions::none(),
        true,
        false,
        &mut fault,
    );
    assert!(result.runtime_fault.is_none());
    assert_eq!(machine(&alias).registers().pc, 0x01000200);
    deliver(
        &mut alias,
        0,
        actions(0x200, false),
        false,
        true,
        &mut fault,
    );
    let frozen = machine(&alias).clone();
    let result = deliver(
        &mut alias,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(machine(&alias), &frozen);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoint_stop_polls_and_pending_irq_trace_have_explicit_boundary_policy() {
    let mut fault = WorkerRuntimeFault::default();
    let mut stopped = fixture(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71]);
    let mut expected = machine(&stopped).clone();
    expected.step_field().unwrap();
    let result = deliver(
        &mut stopped,
        1,
        actions(0x204, false),
        false,
        false,
        &mut fault,
    );
    compare(&stopped, &mut expected, &result, 1);
    assert!(expected.main_cpu_stopped() && !stopped.is_suspended());
    let opcodes = &result.ui_data.cpu_debug.as_ref().unwrap().recent_opcodes;
    assert_eq!(opcodes.len(), 1);
    assert_eq!(opcodes[0].bytes, [0x4e, 0x72]);
    assert!(machine_mut(&mut stopped).set_interrupt_level(6));
    assert!(expected.set_interrupt_level(6));
    expected.clear_frame_ready();
    expected.step_instruction().unwrap();
    assert_eq!(expected.registers().pc, 0x300);
    let result = deliver(
        &mut stopped,
        8,
        actions(0x300, false),
        false,
        false,
        &mut fault,
    );
    compare(&stopped, &mut expected, &result, 0);
    assert_eq!(
        result
            .ui_data
            .cpu_debug
            .as_ref()
            .unwrap()
            .recent_opcodes
            .len(),
        1
    );
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x300)
    );

    let mut irq = fixture(b'U', &[0x46fc, 0x2000, 0x60fe, 0x4e71]);
    machine_mut(&mut irq).step_instruction().unwrap();
    assert!(machine_mut(&mut irq).set_interrupt_level(6));
    let frozen = machine(&irq).clone();
    let result = deliver(&mut irq, 1, actions(0x204, false), false, false, &mut fault);
    assert_eq!(machine(&irq), &frozen);
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x204)
    );

    let result = deliver(&mut irq, 0, DebugUiActions::none(), true, false, &mut fault);
    assert_eq!(machine(&irq).registers().pc, 0x300);
    assert!(result.ui_data.cpu_debug.unwrap().recent_opcodes.is_empty());

    let mut trace = fixture(b'U', &[0x46fc, 0xa000, 0x4e71, 0x60fe, 0x4e71]);
    machine_mut(&mut trace).step_instruction().unwrap();
    trace_state::restore_pending_trace(machine_mut(&mut trace));
    let frozen = machine(&trace).clone();
    assert_eq!(frozen.registers().pc, 0x204);
    let result = deliver(
        &mut trace,
        1,
        actions(0x204, false),
        false,
        false,
        &mut fault,
    );
    assert_eq!(machine(&trace), &frozen);
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x204)
    );
    let result = deliver(
        &mut trace,
        0,
        DebugUiActions::none(),
        true,
        false,
        &mut fault,
    );
    assert!(result.runtime_fault.is_none());
    assert_eq!(machine(&trace).registers().pc, 0x300);
    assert!(result.ui_data.cpu_debug.unwrap().recent_opcodes.is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pc_breakpoint_restore_reset_and_fault_controls_clear_only_successful_stop_metadata() {
    let mut backend = fixture(
        b'U',
        &[
            0x13fc, 0x5a, 0xff, 0x40, 0x13fc, 0x55, 0xb0, 0, 0x60fe, 0x4e71,
        ],
    );
    let raw = backend.encode_state_bytes().unwrap();
    let frontend = backend.encode_rewind_checkpoint().unwrap();
    let mut fault = WorkerRuntimeFault::default();
    deliver(
        &mut backend,
        1,
        actions(0x200, false),
        false,
        false,
        &mut fault,
    );
    let before = machine(&backend).clone();
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    assert!(backend.restore_rewind_checkpoint(vec![0; 64]).is_err());
    let result = deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(machine(&backend), &before);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    backend.load_state_from_bytes(raw.clone()).unwrap();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    backend.restore_rewind_checkpoint(frontend).unwrap();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    backend.take_rewind_audio_change();
    let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    history.push(&machine(&backend).encode_frontend_state().unwrap(), &[]);
    assert!(matches!(
        EmuThread::handle_rewind(&mut backend, &mut history, &Default::default(), 1),
        EmuResponse::RewindOk { .. }
    ));
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    backend.reset();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    let mut remove = DebugUiActions::none();
    remove.remove_breakpoints.push(0x200);
    let result = deliver(&mut backend, 1, remove, false, true, &mut fault);
    assert!(result.runtime_fault.is_some() && !fault.can_step());
    assert!(backend.is_suspended());
    assert_eq!(machine(&backend).peek(0xff0040), Some(0x5a));
    let frozen = machine(&backend).clone();
    for (step, continue_requested) in [(true, false), (false, true), (true, true)] {
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            step,
            continue_requested,
            &mut fault,
        );
        assert_eq!(result.advanced_frames, 0);
        assert_eq!(machine(&backend), &frozen);
    }
    backend.load_state_from_bytes(raw).unwrap();
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
}
