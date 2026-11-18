use super::*;
use crate::SUPERVISOR;
use crate::bus::BUS_ADDRESS_MASK;
use crate::timing::test_bus::{Bus, cpu};
use crate::timing::{InstructionByteOrigin, InstructionInput, Pipeline, TimedOutcome, TimingFault};
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod failures;
mod returns;

const SOURCE: u32 = 0x1234_0100;
const TARGET: u32 = 0xabcd_0400;

fn put(bus: &mut Bus, address: u32, words: &[u16]) {
    for (index, &word) in words.iter().enumerate() {
        bus.words.insert(
            address.wrapping_add(index as u32 * 2) & BUS_ADDRESS_MASK,
            word,
        );
    }
}

fn seed(opcode: u16, extension: u16, status: u16) -> (PrefetchCpu, Bus) {
    let mut cpu = cpu(status);
    cpu.pipeline = Some(Pipeline::unknown(SOURCE, [opcode, extension]));
    let mut bus = Bus::default();
    put(&mut bus, SOURCE, &[opcode, extension, 0x4e71, 0x4e71]);
    put(&mut bus, TARGET, &[0x4e75, 0x4e71]);
    (cpu, bus)
}

fn step(cpu: &mut PrefetchCpu, bus: &mut Bus) -> Result<TimedOutcome, TimingFault<usize>> {
    let mut plain = cpu.clone();
    plain.set_control_flow_recording(false);
    let mut reference = bus.clone();
    let actual = cpu.step_boundary(bus);
    assert_eq!(actual, plain.step_boundary(&mut reference));
    assert_eq!(*cpu, plain);
    assert_eq!(
        cpu.queued_instruction_inputs(),
        plain.queued_instruction_inputs()
    );
    assert_eq!(*bus, reference);
    actual
}

fn events(cpu: &mut PrefetchCpu) -> Vec<ControlFlowEvent> {
    let batch = cpu.drain_control_flow_events();
    assert!(!batch.overflowed());
    batch.events().copied().collect()
}

fn encoded(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cpu_debug_keeps_active_metadata_and_all_fields_in_pretty_output() {
    let (mut cpu, _) = seed(0x4e71, 0x4e71, SUPERVISOR);
    let pretty = format!("{cpu:#?}");
    for field in ["origin:", "decision:", "control_events:"] {
        assert!(pretty.contains(field));
    }

    cpu.pipeline = Some(Pipeline::sampled(
        SOURCE,
        [InstructionInput {
            value: 0x4e71,
            origin: [InstructionByteOrigin::NON_STORAGE; 2],
        }; 2],
    ));
    assert!(cpu.interrupts.restore_observation(true, 2));
    cpu.set_control_flow_recording(true);
    let compact = format!("{cpu:?}");
    assert!(compact.contains("origin: [[InstructionByteOrigin(1)"));
    assert!(compact.contains("decision: Observed { pending: true, level: 2 }"));
    assert!(compact.contains("control_events: Mailbox { enabled: true"));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn committed_jsr_and_rts_preserve_full_stack_banks_wrap_waits_and_disabled_hardware() {
    for supervisor in [false, true] {
        for sp in [0, 2, 0x8100_3000_u32] {
            for wait in [0, 3] {
                let (mut cpu, mut bus) = seed(0x4e90, 0x4e71, u16::from(supervisor) * SUPERVISOR);
                assert!(!cpu.control_flow_recording());
                assert!(events(&mut cpu).is_empty());
                cpu.registers.a[0] = TARGET;
                cpu.registers.set_address_register(7, sp);
                cpu.set_control_flow_recording(true);
                bus.wait = wait;
                let bank = if supervisor {
                    ControlStackBank::Supervisor
                } else {
                    ControlStackBank::User
                };
                let outcome = step(&mut cpu, &mut bus).unwrap();
                assert!(
                    matches!(outcome, TimedOutcome::Instruction(step) if step.clocks == 16 + 4 * u64::from(wait))
                );
                assert_eq!(
                    events(&mut cpu),
                    [ControlFlowEvent::Entry {
                        kind: ControlFlowEntryKind::Jsr,
                        source_pc: SOURCE,
                        target_pc: TARGET,
                        return_pc: SOURCE + 2,
                        stack: ControlStack {
                            bank,
                            before: sp,
                            after: sp.wrapping_sub(4),
                            pc_slot: sp.wrapping_sub(4)
                        },
                    }]
                );
                assert_eq!(bus.words[&(sp.wrapping_sub(4) & BUS_ADDRESS_MASK)], 0x1234);
                assert_eq!(bus.words[&(sp.wrapping_sub(2) & BUS_ADDRESS_MASK)], 0x0102);
                step(&mut cpu, &mut bus).unwrap();
                assert_eq!(
                    events(&mut cpu),
                    [ControlFlowEvent::Return {
                        kind: ControlFlowReturnKind::Rts,
                        source_pc: TARGET,
                        target_pc: SOURCE + 2,
                        supervisor_after: supervisor,
                        stack: ControlStack {
                            bank,
                            before: sp.wrapping_sub(4),
                            after: sp,
                            pc_slot: sp.wrapping_sub(4)
                        },
                    }]
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn jsr_ea_forms_and_short_word_bsr_publish_the_actual_computed_return_and_target() {
    for (opcode, extension, target, return_pc) in [
        (0x4e90, 0x4e71, 0xabcd_4000, SOURCE + 2),
        (0x4ea8, 0xfff8, 0xabcd_3ff8, SOURCE + 4),
        (0x4eb0, 0x10fe, 0xabcd_4010, SOURCE + 4),
        (0x4eb8, 0x8030, 0xffff_8030, SOURCE + 4),
        (0x4eb9, 0xabcd, 0xabcd_3010, SOURCE + 6),
        (0x4eba, 0xfffe, SOURCE, SOURCE + 4),
        (0x4ebb, 0x10fe, SOURCE + 0x12, SOURCE + 4),
        (0x6106, 0x4e71, SOURCE + 8, SOURCE + 2),
        (0x6100, 0xffe0, SOURCE - 0x1e, SOURCE + 4),
    ] {
        for status in [0, SUPERVISOR] {
            let (mut cpu, mut bus) = seed(opcode, extension, status);
            cpu.registers.a[0] = 0xabcd_4000;
            cpu.registers.d[1] = 0xaaaa_0012;
            if opcode == 0x4eb9 {
                put(&mut bus, SOURCE + 4, &[0x3010]);
            }
            let sp = cpu.registers.stack_pointer();
            cpu.set_control_flow_recording(true);
            step(&mut cpu, &mut bus).unwrap();
            assert_eq!(cpu.registers.pc, target);
            assert_eq!(
                events(&mut cpu),
                [ControlFlowEvent::Entry {
                    kind: if opcode & 0xff00 == 0x6100 {
                        ControlFlowEntryKind::Bsr
                    } else {
                        ControlFlowEntryKind::Jsr
                    },
                    source_pc: SOURCE,
                    target_pc: target,
                    return_pc,
                    stack: ControlStack {
                        bank: if status == 0 {
                            ControlStackBank::User
                        } else {
                            ControlStackBank::Supervisor
                        },
                        before: sp,
                        after: sp - 4,
                        pc_slot: sp - 4,
                    },
                }]
            );
        }
    }
    for opcode in [0x4ed0, 0x4850, 0x4e50, 0x4e58] {
        let (mut cpu, mut bus) = seed(opcode, 0xfff8, SUPERVISOR);
        cpu.registers.a[0] = TARGET;
        cpu.set_control_flow_recording(true);
        step(&mut cpu, &mut bus).unwrap();
        assert!(events(&mut cpu).is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mailbox_overflow_enable_clear_drain_and_clone_follow_real_nested_calls() {
    let (mut cpu, mut bus) = seed(0x6106, 0x4e71, SUPERVISOR);
    put(
        &mut bus,
        SOURCE + 8,
        &[0x6106, 0x4e71, 0x4e71, 0x4e71, 0x6106, 0x4e71],
    );
    cpu.set_control_flow_recording(true);
    for _ in 0..2 {
        step(&mut cpu, &mut bus).unwrap();
    }
    cpu.set_control_flow_recording(true);
    let mut disabled = cpu.clone();
    disabled.set_control_flow_recording(false);
    assert!(events(&mut disabled).is_empty());
    let mut cloned = cpu.clone();
    let first = events(&mut cloned);
    assert_eq!(first.len(), 2);
    assert!(matches!(
        first[0],
        ControlFlowEvent::Entry {
            source_pc: SOURCE,
            ..
        }
    ));
    assert!(
        matches!(first[1], ControlFlowEvent::Entry { source_pc, .. } if source_pc == SOURCE + 8)
    );
    assert!(events(&mut cloned).is_empty());
    step(&mut cpu, &mut bus).unwrap();
    let batch = cpu.drain_control_flow_events();
    assert!(batch.overflowed());
    assert_eq!(batch.events().copied().collect::<Vec<_>>(), first);
    assert!(events(&mut cpu).is_empty());
    assert!(cpu.control_flow_recording());
    cpu.clear_control_flow_events();
    cpu.set_control_flow_recording(false);
    assert!(!cpu.control_flow_recording());
    step(&mut cpu, &mut bus).unwrap();
    assert!(events(&mut cpu).is_empty());
    cloned.set_control_flow_recording(true);
    assert!(cloned.control_flow_recording());
    cloned.clear_control_flow_events();
    assert!(events(&mut cloned).is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn public_boundary_codec_excludes_events_and_failed_import_preserves_real_metadata() {
    let (mut cpu, mut bus) = seed(0x6106, 0x4e71, SUPERVISOR);
    cpu.set_control_flow_recording(true);
    step(&mut cpu, &mut bus).unwrap();
    let mut plain = cpu.clone();
    plain.set_control_flow_recording(false);
    assert_eq!(cpu, plain);
    let bytes = encoded(&cpu);
    assert_eq!(bytes, encoded(&plain));
    let before = cpu.clone().drain_control_flow_events();
    assert_eq!(before.events().count(), 1);
    assert!(
        cpu.read_boundary_state(&mut StateReader::new(&bytes[..bytes.len() - 1]))
            .is_err()
    );
    assert!(cpu.control_flow_recording());
    assert_eq!(cpu.clone().drain_control_flow_events(), before);
    assert_eq!(encoded(&cpu), bytes);
    cpu.read_boundary_state(&mut StateReader::new(&bytes))
        .unwrap();
    assert_eq!(cpu, plain);
    assert!(!cpu.control_flow_recording());
    assert!(events(&mut cpu).is_empty());
    assert_eq!(encoded(&cpu), bytes);
}
