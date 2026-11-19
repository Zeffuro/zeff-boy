use super::md_pc_breakpoint_tests::{compare, fixture_with_timing, machine, machine_mut};
use super::md_rom_cheat_tests::{codes, update};
use super::*;
use crate::debug::DebugUiActions;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, TraceBoundaryKind, TraceExecMode, TraceWriteKind,
    TraceWriteWidth, WatchType,
};
use zeff_md_core::TimingModel;

mod bounds;
mod lifecycle;
mod previous;

fn enabled() -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.trace_enabled = Some(true);
    actions
}

fn run(
    backend: &mut EmuBackend,
    actions: DebugUiActions,
    step: bool,
    fields: usize,
    cursor: Option<u64>,
) -> FrameResult {
    let mut input = md_audio_tests::input(fields, false);
    input.debug_actions = actions;
    input.debug_step = step;
    input.snapshot.want_debug_info = true;
    input.snapshot.show_disassembler = true;
    input.snapshot.show_instruction_trace = true;
    let EmuBackend::Md(md) = &*backend else {
        panic!("expected MD")
    };
    input.snapshot.trace_cursor = crate::ui::TraceCursor {
        target: crate::ui::TraceTarget::Primary,
        stream: Some(md.instruction_trace_stream()),
        after_sequence: cursor,
    };
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 10,
        &mut WorkerRuntimeFault::default(),
    )
}

fn entries(result: &FrameResult) -> &[InstructionTraceRecord] {
    &result.ui_data.instruction_trace.as_ref().unwrap().entries
}

fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes()).collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_reaches_worker_with_consumed_words_registers_and_bus_values() {
    let instructions: [&[u16]; 4] = [
        &[0x303c, 0x1234],
        &[0x23fc, 0x89ab, 0xcdef, 0xff, 0x40],
        &[0x33fc, 0xa55a, 0xff, 0x44],
        &[0x4e72, 0x2700],
    ];
    let program: Vec<_> = instructions
        .iter()
        .flat_map(|words| words.iter().copied())
        .collect();
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(header, &program, timing);
            assert!(
                update(
                    &mut backend,
                    &mut Vec::new(),
                    codes("000200:7001+000202:FFFF")
                )
                .is_none()
            );
            assert_eq!(machine(&backend).peek(0x200), Some(0x70));
            backend.debug_suspend();
            let mut expected = machine(&backend).clone();
            for (index, words) in instructions.into_iter().enumerate() {
                let before = expected.cpu_clock().0;
                let pc = expected.registers().pc;
                expected.step_instruction().unwrap();
                let result = run(&mut backend, enabled(), true, 0, None);
                compare(&backend, &mut expected, &result, 0);
                let caps = result.ui_data.core_features.as_ref().unwrap();
                assert!(caps.supports_instruction_trace && caps.supports_debugger);
                assert_eq!(entries(&result).len(), index + 1);
                let row = entries(&result).last().unwrap();
                assert_eq!(row.mode, TraceExecMode::M68000);
                assert_eq!((row.pc, row.cycle), (pc, before));
                assert_eq!(row.instruction_bytes(), bytes(words));
                assert_eq!(
                    (row.physical_rom_offset, row.bank),
                    (Some(u64::from(pc)), None)
                );
                let boundary = row.boundary.unwrap();
                assert_eq!(boundary.kind, TraceBoundaryKind::Instruction);
                assert_eq!(boundary.cpu_clocks, expected.cpu_clock().0 - before);
                assert!(!boundary.machine_fault);
                assert_eq!(
                    (
                        row.instruction_overflow,
                        row.register_delta_overflow,
                        row.write_overflow
                    ),
                    (0, 0, 0)
                );
                assert!(row.register_deltas().iter().any(|delta| delta.register == 17 && delta.value == expected.registers().pc));
                assert!(
                    row.writes().iter().all(
                        |write| write.old_value == Some(0) && write.kind == TraceWriteKind::Bus
                    )
                );
                if index == 0 {
                    assert!(
                        row.register_deltas()
                            .iter()
                            .any(|delta| delta.register == 0 && delta.value == 0x1234)
                    );
                } else if index == 1 {
                    assert_eq!(
                        row.writes()
                            .iter()
                            .map(|write| (write.address, write.new_value, write.width))
                            .collect::<Vec<_>>(),
                        [
                            (0xff0040, 0x89ab, TraceWriteWidth::Halfword),
                            (0xff0042, 0xcdef, TraceWriteWidth::Halfword)
                        ]
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_fault_retains_driven_write_prefix_and_runtime_latch() {
    let mut backend = fixture_with_timing(
        b'U',
        &[0x23fc, 0x1234, 0x5678, 0x7f, 0xfffe, 0x4e71],
        TimingModel::Standard,
    );
    backend.debug_suspend();
    let mut expected = machine(&backend).clone();
    assert!(expected.step_instruction().is_err());
    let result = run(&mut backend, enabled(), true, 0, None);
    assert!(result.runtime_fault.is_some());
    assert_eq!(machine(&backend), &expected);
    let row = &entries(&result)[0];
    assert_eq!(
        row.instruction_bytes(),
        bytes(&[0x23fc, 0x1234, 0x5678, 0x7f, 0xfffe])
    );
    assert_eq!(row.boundary.unwrap().kind, TraceBoundaryKind::Fault);
    assert!(row.boundary.unwrap().machine_fault);
    assert_eq!(row.writes().len(), 1);
    assert_eq!(
        (
            row.writes()[0].address,
            row.writes()[0].new_value,
            row.writes()[0].old_value
        ),
        (0x7ffffe, 0x1234, None)
    );
    let frozen = machine(&backend).clone();
    let result = run(&mut backend, DebugUiActions::none(), true, 1, None);
    assert_eq!(machine(&backend), &frozen);
    assert_eq!(entries(&result).len(), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_stop_irq_pending_trace_and_traps_keep_distinct_boundaries() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend =
                fixture_with_timing(header, &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
            let result = run(&mut backend, enabled(), false, 1, None);
            assert!(result.runtime_fault.is_none());
            assert_eq!(entries(&result).len(), 1);
            assert_eq!(
                entries(&result)[0].instruction_bytes(),
                bytes(&[0x4e72, 0x2000])
            );
            machine_mut(&mut backend).set_interrupt_level(6);
            backend.debug_suspend();
            let result = run(&mut backend, DebugUiActions::none(), true, 0, None);
            let irq = entries(&result).last().unwrap();
            assert_eq!(irq.boundary.unwrap().kind, TraceBoundaryKind::Interrupt);
            assert_eq!(irq.boundary.unwrap().final_exception, Some(30));
            assert_eq!(irq.event, Some(DebugEvent::Interrupt));
            assert!(irq.instruction_bytes().is_empty());
            assert_eq!(irq.writes().len(), 3);
            assert!(irq.writes().iter().all(|write| write.old_value == Some(0)));

            let mut trace = fixture_with_timing(header, &[0x46fc, 0xa000, 0x4e71, 0x60fe], timing);
            machine_mut(&mut trace).step_instruction().unwrap();
            super::md_pc_breakpoint_tests::trace_state::restore_pending_trace(machine_mut(
                &mut trace,
            ));
            trace.debug_suspend();
            let result = run(&mut trace, enabled(), true, 0, None);
            let row = &entries(&result)[0];
            assert_eq!(row.boundary.unwrap().kind, TraceBoundaryKind::Trace);
            assert_eq!(row.boundary.unwrap().final_exception, Some(9));
            assert_eq!(row.writes().len(), 3);
            assert!(row.writes().iter().all(|write| write.old_value == Some(0)));
            assert!(row.instruction_bytes().is_empty());
            assert_eq!(row.event, Some(DebugEvent::Exception));

            let mut trap = fixture_with_timing(header, &[0x4e40, 0x4e71], timing);
            trap.debug_suspend();
            let result = run(&mut trap, enabled(), true, 0, None);
            let row = &entries(&result)[0];
            assert_eq!(row.instruction_bytes(), bytes(&[0x4e40]));
            assert_eq!(row.boundary.unwrap().final_exception, Some(32));
            assert_eq!(row.event, Some(DebugEvent::Exception));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_fields_match_untraced_state_framebuffer_and_nonzero_pcm() {
    let program = [
        0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239, 0xff,
        0x40, 0x60f8,
    ];
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(header, &program, timing);
            let mut expected = machine(&backend).clone();
            for enabled in [true, false, true] {
                expected.step_field().unwrap();
                let mut actions = DebugUiActions::none();
                actions.trace_enabled = Some(enabled);
                let result = run(&mut backend, actions, false, 1, None);
                assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(machine(&backend).framebuffer(), expected.framebuffer());
                compare(&backend, &mut expected, &result, 1);
                assert_eq!(
                    result.ui_data.instruction_trace.as_ref().unwrap().enabled,
                    enabled
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_coexists_with_pc_watch_irq_and_single_step_policy() {
    let mut backend = fixture_with_timing(
        b'U',
        &[0x33fc, 0xa55a, 0xff, 0x40, 0x4e72, 0x2000, 0x60fe],
        TimingModel::Standard,
    );
    let mut actions = enabled();
    actions.add_breakpoint = Some(0x200);
    actions.add_watchpoint = Some((0xff0041, 0xff0041, WatchType::Write));
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Interrupt, true));
    let result = run(&mut backend, actions, false, 1, None);
    assert!(entries(&result).is_empty());
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    let result = run(&mut backend, DebugUiActions::none(), true, 0, None);
    assert_eq!(entries(&result).len(), 1);
    let cpu = result.ui_data.cpu_debug.unwrap();
    assert_eq!(cpu.hit_watchpoint.unwrap().new_value, Some(0x5a));
    assert!(backend.is_suspended());
    let result = run(&mut backend, DebugUiActions::none(), true, 0, None);
    assert_eq!(entries(&result).len(), 2);
    machine_mut(&mut backend).set_interrupt_level(6);
    let result = run(&mut backend, DebugUiActions::none(), true, 0, None);
    assert_eq!(entries(&result).len(), 3);
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_event,
        Some(DebugEvent::Interrupt)
    );
    assert!(backend.is_suspended());
}
