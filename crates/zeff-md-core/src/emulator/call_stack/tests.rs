use super::*;
use crate::emulator::{LoadConfig, Region, TimingModel};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::timing::ControlStackBank;

mod state;

const PROFILES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];

fn words(bytes: &mut [u8], address: usize, values: &[u16]) {
    for (index, word) in values.iter().enumerate() {
        bytes[address + index * 2..address + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
}

fn image(sp: u32, pc: u32, inner: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x2000];
    bytes[..4].copy_from_slice(&sp.to_be_bytes());
    bytes[4..8].copy_from_slice(&pc.to_be_bytes());
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    words(&mut bytes, 0x24, &[0, 0x600]);
    words(
        &mut bytes,
        0x200,
        &[
            0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 0x10, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x4eb9,
            0, 0x400, 0x60fe, 0x4e71,
        ],
    );
    words(&mut bytes, 0x400, &[0x6106, 0x4e75, 0x4e71, 0x4e71]);
    words(&mut bytes, 0x408, inner);
    words(&mut bytes, 0x600, &[0x4e73, 0x4e71]);
    bytes
}

fn with_program(
    timing: TimingModel,
    concurrent: bool,
    sp: u32,
    pc: u32,
    inner: &[u16],
) -> Emulator {
    Emulator::new_with_timing(
        image(sp, pc, inner),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
        timing,
    )
    .unwrap()
}

fn fixture(timing: TimingModel, concurrent: bool) -> Emulator {
    with_program(
        timing,
        concurrent,
        0xfffffc,
        0x200,
        &[0x4e71, 0x4e75, 0x4e71],
    )
}

fn frames(machine: &Emulator) -> Vec<CallStackFrame> {
    machine.call_stack().copied().collect()
}

fn enter(machine: &mut Emulator) {
    machine.set_call_stack_recording(true);
    for _ in 0..5 {
        machine.step_instruction().unwrap();
    }
    assert_eq!(machine.registers().pc, 0x408);
    assert_eq!(frames(machine).len(), 2);
}

fn patch(value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address: 0x1800,
        value: CheatValue::Constant(value),
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn normal_kernels_capture_nested_calls_full_coordinates_and_real_returns_without_changing_hardware()
{
    for (timing, concurrent) in PROFILES {
        for sp in [0xfffffc_u32, 0x01fffffc, 0] {
            let mut subject = with_program(
                timing,
                concurrent,
                sp,
                0x80000200,
                &[0x4e71, 0x4e75, 0x4e71],
            );
            let mut control = with_program(
                timing,
                concurrent,
                sp,
                0x80000200,
                &[0x4e71, 0x4e75, 0x4e71],
            );
            assert!(control.call_stack.recording.is_none());
            subject.set_call_stack_recording(true);
            for _ in 0..5 {
                assert_eq!(subject.step_instruction(), control.step_instruction());
                assert_eq!(
                    subject.encode_frontend_state().unwrap(),
                    control.encode_frontend_state().unwrap()
                );
                assert_eq!(
                    subject.cpu.queued_instruction_inputs(),
                    control.cpu.queued_instruction_inputs()
                );
                assert!(
                    subject
                        .cpu
                        .clone()
                        .drain_control_flow_events()
                        .events()
                        .next()
                        .is_none()
                );
            }
            assert_eq!(
                frames(&subject),
                [
                    CallStackFrame {
                        kind: ControlFlowEntryKind::Jsr,
                        source_pc: 0x80000218,
                        target_pc: 0x400,
                        return_pc: 0x8000021e,
                        stack: ControlStack {
                            bank: ControlStackBank::Supervisor,
                            before: sp,
                            after: sp.wrapping_sub(4),
                            pc_slot: sp.wrapping_sub(4)
                        }
                    },
                    CallStackFrame {
                        kind: ControlFlowEntryKind::Bsr,
                        source_pc: 0x400,
                        target_pc: 0x408,
                        return_pc: 0x402,
                        stack: ControlStack {
                            bank: ControlStackBank::Supervisor,
                            before: sp.wrapping_sub(4),
                            after: sp.wrapping_sub(8),
                            pc_slot: sp.wrapping_sub(8)
                        }
                    },
                ]
            );
            let saved = subject.encode_debugger_checkpoint().unwrap();
            assert!(subject.begin_debug_call(0x1001).is_err());
            assert_eq!(subject.encode_debugger_checkpoint().unwrap(), saved);
            assert_eq!(frames(&subject).len(), 2);
            for count in [2, 1, 0] {
                assert_eq!(subject.step_instruction(), control.step_instruction());
                assert_eq!(frames(&subject).len(), count);
                assert_eq!(
                    subject.encode_state().unwrap(),
                    control.encode_state().unwrap()
                );
            }
            assert_eq!(subject.registers().pc, 0x8000021e);
            assert_eq!(
                subject.call_stack_status(),
                CallStackStatus::Observed { truncated: false }
            );
            assert_eq!(control.call_stack_status(), CallStackStatus::Disabled);
        }
        let mut subject = fixture(timing, concurrent);
        let mut control = fixture(timing, concurrent);
        subject.set_call_stack_recording(true);
        for _ in 0..2 {
            subject.step_field().unwrap();
            control.step_field().unwrap();
            assert_eq!(
                subject.encode_state().unwrap(),
                control.encode_state().unwrap()
            );
            assert_eq!(
                subject.encode_frontend_state().unwrap(),
                control.encode_frontend_state().unwrap()
            );
            assert_eq!(subject.framebuffer(), control.framebuffer());
            assert!(!subject.framebuffer().is_empty());
            let audio = subject.drain_audio_samples();
            assert!(audio.iter().any(|sample| *sample != 0.0));
            assert!(
                audio.iter().map(|sample| sample.to_bits()).eq(control
                    .drain_audio_samples()
                    .iter()
                    .map(|sample| sample.to_bits()))
            );
        }
        assert!(frames(&subject).is_empty());
        assert_eq!(
            subject.call_stack_status(),
            CallStackStatus::Observed { truncated: false }
        );
        assert!(
            subject
                .cpu
                .clone()
                .drain_control_flow_events()
                .events()
                .next()
                .is_none()
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn core_applies_ordered_return_trace_and_bank_mismatch_as_observed_facts() {
    for (timing, concurrent) in PROFILES {
        vertical_irq(timing, concurrent);
        let mut machine = with_program(
            timing,
            concurrent,
            0xfffffc,
            0x200,
            &[0x46fc, 0xa700, 0x4e71, 0x4e75, 0x4e71],
        );
        enter(&mut machine);
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(
            frames(&machine)
                .iter()
                .map(|frame| frame.kind)
                .collect::<Vec<_>>(),
            [
                ControlFlowEntryKind::Jsr,
                ControlFlowEntryKind::Bsr,
                ControlFlowEntryKind::Exception(9)
            ]
        );
        assert_eq!(frames(&machine)[2].return_pc, 0x40e);
        machine.step_instruction().unwrap();
        assert_eq!(frames(&machine).len(), 2);
        machine.step_instruction().unwrap();
        let captured = frames(&machine);
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].kind, ControlFlowEntryKind::Jsr);
        assert_eq!(captured[1].kind, ControlFlowEntryKind::Exception(9));
        assert_eq!(captured[1].return_pc, 0x402);

        let mut mismatch = with_program(
            timing,
            concurrent,
            0xfffffc,
            0x200,
            &[0x207c, 0x01ff, 0xfff4, 0x4e60, 0x46fc, 0, 0x4e75, 0x4e71],
        );
        enter(&mut mismatch);
        for _ in 0..4 {
            mismatch.step_instruction().unwrap();
        }
        assert_eq!(mismatch.registers().pc, 0x402);
        assert_eq!(mismatch.registers().stack_pointer(), 0x01fffff8);
        assert!(frames(&mismatch).is_empty());
        assert_eq!(mismatch.call_stack_status(), CallStackStatus::Unknown);
        let mut rtr = with_program(
            timing,
            concurrent,
            0xfffffc,
            0x200,
            &[0x3f3c, 0x001f, 0x4e77, 0x4e71],
        );
        enter(&mut rtr);
        rtr.step_instruction().unwrap();
        assert_eq!(frames(&rtr).len(), 2);
        rtr.step_instruction().unwrap();
        assert_eq!(frames(&rtr).len(), 1);
        assert_eq!(rtr.registers().pc, 0x402);
        assert_eq!(rtr.registers().status_register() & 0x1f, 0x1f);
        if timing == TimingModel::Standard {
            let mut bytes = image(0xfffffc, 0x200, &[0x46fc, 0x2000, 0x4e71, 0x4e75]);
            words(&mut bytes, 28 * 4, &[0, 0x600]);
            let mut irq = Emulator::new(
                bytes,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: concurrent,
                },
            )
            .unwrap();
            enter(&mut irq);
            irq.step_instruction().unwrap();
            assert!(irq.set_interrupt_level(4));
            let before = irq.registers().stack_pointer();
            let outcome = irq.step_instruction().unwrap();
            assert!(matches!(
                outcome.cpu,
                super::super::TimedOutcome::Interrupt {
                    level: 4,
                    vector: 28,
                    ..
                }
            ));
            let captured = frames(&irq);
            assert_eq!(captured.len(), 3);
            assert_eq!(
                captured[2],
                CallStackFrame {
                    kind: ControlFlowEntryKind::Interrupt {
                        level: 4,
                        vector: 28
                    },
                    source_pc: 0x40c,
                    target_pc: 0x600,
                    return_pc: 0x40c,
                    stack: ControlStack {
                        bank: ControlStackBank::Supervisor,
                        before,
                        after: before.wrapping_sub(6),
                        pc_slot: before.wrapping_sub(4)
                    },
                }
            );
            assert!(irq.set_interrupt_level(0));
            irq.step_instruction().unwrap();
            assert_eq!(frames(&irq), captured[..2]);
            assert_eq!(irq.registers().pc, 0x40c);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bounded_real_recursion_toggle_and_reset_lifecycles_rearm_without_default_history() {
    for (timing, concurrent) in PROFILES {
        let mut bytes = image(0xfffffc, 0x200, &[0x4e71]);
        words(&mut bytes, 0x400, &[0x61fe, 0x4e71]);
        let mut machine = Emulator::new_with_timing(
            bytes,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: concurrent,
            },
            timing,
        )
        .unwrap();
        machine.set_call_stack_recording(true);
        for _ in 0..261 {
            machine.step_instruction().unwrap();
        }
        let captured = frames(&machine);
        assert_eq!(captured.len(), 256);
        assert!(
            captured
                .iter()
                .all(|frame| frame.kind == ControlFlowEntryKind::Bsr && frame.return_pc == 0x402)
        );
        assert_eq!(
            machine.call_stack_status(),
            CallStackStatus::Observed { truncated: true }
        );
        machine.set_call_stack_recording(true);
        assert_eq!(frames(&machine), captured);
        let saved = machine.encode_debugger_checkpoint().unwrap();
        if timing == TimingModel::Precise {
            assert!(machine.reset_cpu().is_err());
            assert!(machine.warm_reset().is_err());
            assert_eq!(machine.encode_debugger_checkpoint().unwrap(), saved);
        } else {
            machine.reset_cpu().unwrap();
            assert_eq!(machine.call_stack_status(), CallStackStatus::Unknown);
            assert!(frames(&machine).is_empty());
            for _ in 0..4 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(frames(&machine).len(), 1);
            machine.warm_reset().unwrap();
            assert_eq!(machine.call_stack_status(), CallStackStatus::Unknown);
            assert!(frames(&machine).is_empty());
        }
        machine.reset();
        assert_eq!(machine.call_stack_status(), CallStackStatus::Unknown);
        for _ in 0..4 {
            machine.step_instruction().unwrap();
        }
        assert_eq!(frames(&machine).len(), 1);
        machine.set_call_stack_recording(false);
        assert_eq!(machine.call_stack_status(), CallStackStatus::Disabled);
        assert!(machine.call_stack.recording.is_none());
        machine.step_instruction().unwrap();
        assert!(frames(&machine).is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn actual_main_fault_and_late_sound_entry_failure_invalidate_committed_prefixes() {
    for (timing, concurrent) in PROFILES {
        let mut machine = with_program(
            timing,
            concurrent,
            0xfffffc,
            0x200,
            &[0x3039, 0x00b0, 0, 0x4e75],
        );
        enter(&mut machine);
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.call_stack_status(), CallStackStatus::Unknown);
        assert!(frames(&machine).is_empty());
        assert!(machine.cpu.control_flow_recording());
        assert!(
            machine
                .cpu
                .clone()
                .drain_control_flow_events()
                .events()
                .next()
                .is_none()
        );
    }
    let mut found = false;
    'phase: for padding in 0..9 {
        for lead in 0..17 {
            let mut program = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
            let mut sound = vec![0_u8; padding];
            sound.extend([0x3a, 0, 0x7f, 0x76]);
            for (index, value) in sound.iter().enumerate() {
                program.extend([0x13fc, u16::from(*value), 0xa0, index as u16]);
            }
            program.extend([0x33fc, 0, 0xa1, 0x1100]);
            let setup = 4 + sound.len() + lead;
            program.extend(std::iter::repeat_n(0x4e71, lead));
            program.extend([0x60fe, 0x4e71]);
            let mut bytes = image(0xfffffc, 0x200, &[0x4e71]);
            words(&mut bytes, 0x200, &[0x4eb9, 0, 0x400, 0x60fe, 0x4e71]);
            words(&mut bytes, 0x400, &program);
            words(&mut bytes, 0x1000, &[0x7007, 0x4e75, 0x4e71]);
            let mut machine = Emulator::new(
                bytes,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            machine.set_call_stack_recording(true);
            if (0..setup).any(|_| machine.step_instruction().is_err())
                || machine.check_debug_call(0x1000).is_err()
            {
                continue;
            }
            assert_eq!(frames(&machine).len(), 1);
            let before = machine.registers();
            if !matches!(
                machine.begin_debug_call(0x1000),
                Err(super::super::GuestCallError::Execution(
                    super::super::MachineFault::Sound(_)
                ))
            ) {
                continue;
            }
            assert_eq!(machine.registers().pc, 0x1000);
            assert_eq!(
                machine.registers().stack_pointer(),
                before.stack_pointer() - 4
            );
            assert!(frames(&machine).is_empty());
            assert_eq!(machine.call_stack_status(), CallStackStatus::Unknown);
            assert!(
                machine
                    .cpu
                    .clone()
                    .drain_control_flow_events()
                    .events()
                    .next()
                    .is_none()
            );
            found = true;
            break 'phase;
        }
    }
    assert!(found, "no actual admitted late SOUND entry fault reached");
}

fn vertical_irq(timing: TimingModel, concurrent: bool) {
    let mut bytes = image(0x01fffffc, 0x80000200, &[]);
    words(&mut bytes, 30 * 4, &[0, 0x600]);
    words(
        &mut bytes,
        0x200,
        &[
            0x46fc, 0x2000, 0x33fc, 0x8000, 0x00c0, 4, 0x33fc, 0x8124, 0x00c0, 4, 0x60fe, 0x4e71,
        ],
    );
    let create = || {
        Emulator::new_with_timing(
            bytes.clone(),
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: concurrent,
            },
            timing,
        )
        .unwrap()
    };
    let mut subject = create();
    let mut control = create();
    subject.set_call_stack_recording(true);
    let deadline = subject.timing_snapshot().now().get() + subject.region().frame_ticks() * 2;
    loop {
        assert!(
            subject.timing_snapshot().now().get() < deadline,
            "no real VDP IRQ6 within two fields"
        );
        let before = subject.registers();
        let actual = subject.step_instruction().unwrap();
        assert_eq!(actual, control.step_instruction().unwrap());
        assert_eq!(
            subject.cpu.queued_instruction_inputs(),
            control.cpu.queued_instruction_inputs()
        );
        if matches!(
            actual.cpu,
            super::super::TimedOutcome::Interrupt {
                level: 6,
                vector: 30,
                ..
            }
        ) {
            assert!(subject.timing_snapshot().now().get() <= deadline);
            assert_eq!(before.pc, 0x80000214);
            assert_eq!(before.stack_pointer(), 0x01fffffc);
            assert_eq!(subject.registers().pc, 0x600);
            assert_eq!(
                frames(&subject),
                [CallStackFrame {
                    kind: ControlFlowEntryKind::Interrupt {
                        level: 6,
                        vector: 30
                    },
                    source_pc: 0x80000214,
                    target_pc: 0x600,
                    return_pc: 0x80000214,
                    stack: ControlStack {
                        bank: ControlStackBank::Supervisor,
                        before: 0x01fffffc,
                        after: 0x01fffff6,
                        pc_slot: 0x01fffff8,
                    },
                }]
            );
            for (index, byte) in [0x20, 8, 0x80, 0, 2, 0x14].into_iter().enumerate() {
                assert_eq!(subject.peek(0xfffff6 + index as u32), Some(byte));
            }
            break;
        }
    }
    assert_eq!(subject.vdp_registers()[0], Some(0));
    assert_eq!(subject.vdp_registers()[1], Some(0x24));
    assert_eq!(
        subject.encode_frontend_state().unwrap(),
        control.encode_frontend_state().unwrap()
    );
    assert_eq!(subject.step_instruction(), control.step_instruction());
    assert_eq!(subject.registers().pc, 0x80000214);
    assert_eq!(subject.registers().stack_pointer(), 0x01fffffc);
    assert!(frames(&subject).is_empty());
    assert_eq!(
        subject.call_stack_status(),
        CallStackStatus::Observed { truncated: false }
    );
    assert_eq!(control.call_stack_status(), CallStackStatus::Disabled);
    assert_eq!(
        subject.encode_frontend_state().unwrap(),
        control.encode_frontend_state().unwrap()
    );
    assert!(
        subject
            .drain_audio_samples()
            .iter()
            .map(|sample| sample.to_bits())
            .eq(control
                .drain_audio_samples()
                .iter()
                .map(|sample| sample.to_bits()))
    );
}

mod input_edits;
