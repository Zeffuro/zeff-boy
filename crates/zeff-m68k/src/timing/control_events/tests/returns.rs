use super::*;
use crate::TRACE;
use crate::timing::InterruptVector;

const HANDLER: u32 = 0x1234_5000;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rtr_and_rte_report_the_popped_bank_slot_and_post_sp_before_user_switch() {
    for (opcode, before_sr, stacked_sr, after_sr, bank) in [
        (0x4e77, 0x0500, 0xffff, 0x051f, ControlStackBank::User),
        (0x4e77, 0x2500, 0xffff, 0x251f, ControlStackBank::Supervisor),
        (0x4e73, 0x2500, 0x8015, 0x8015, ControlStackBank::Supervisor),
        (0x4e73, 0x2500, 0xa315, 0xa315, ControlStackBank::Supervisor),
    ] {
        for sp in [0x8100_3000_u32, 0xffff_fffc] {
            for wait in [0, 3] {
                let (mut cpu, mut bus) = seed(opcode, 0x4e71, before_sr);
                cpu.registers.usp = if bank == ControlStackBank::User {
                    sp
                } else {
                    0x7abc_5000
                };
                cpu.registers.ssp = if bank == ControlStackBank::Supervisor {
                    sp
                } else {
                    0x3456_7000
                };
                put(&mut bus, sp, &[stacked_sr, 0xabcd, 0x0400]);
                cpu.set_control_flow_recording(true);
                bus.wait = wait;
                step(&mut cpu, &mut bus).unwrap();
                assert_eq!(cpu.registers.status_register(), after_sr);
                assert_eq!(cpu.registers.pc, TARGET);
                assert_eq!(
                    events(&mut cpu),
                    [ControlFlowEvent::Return {
                        kind: if opcode == 0x4e73 {
                            ControlFlowReturnKind::Rte
                        } else {
                            ControlFlowReturnKind::Rtr
                        },
                        source_pc: SOURCE,
                        target_pc: TARGET,
                        supervisor_after: after_sr & SUPERVISOR != 0,
                        stack: ControlStack {
                            bank,
                            before: sp,
                            after: sp.wrapping_add(6),
                            pc_slot: sp.wrapping_add(2)
                        },
                    }]
                );
                if opcode == 0x4e73 && after_sr & SUPERVISOR == 0 {
                    assert_eq!(cpu.registers.stack_pointer(), 0x7abc_5000);
                    assert_eq!(cpu.registers.ssp, sp.wrapping_add(6));
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn inline_trace_keeps_two_ordered_real_call_return_and_trap_facts() {
    for supervisor in [false, true] {
        let (mut cpu, mut bus) = seed(0x4e90, 0x4e71, TRACE | (u16::from(supervisor) * SUPERVISOR));
        cpu.registers.a[0] = TARGET;
        cpu.set_control_flow_recording(true);
        bus.handler(9);
        let bank = if supervisor {
            ControlStackBank::Supervisor
        } else {
            ControlStackBank::User
        };
        let sp = if supervisor { 0x1000 } else { 0x2000 };
        let trace_sp = if supervisor { 0xffc } else { 0x1000 };
        let result = step(&mut cpu, &mut bus).unwrap();
        assert!(matches!(result, TimedOutcome::Instruction(step) if step.exception == Some(9)));
        assert_eq!(
            events(&mut cpu),
            [
                ControlFlowEvent::Entry {
                    kind: ControlFlowEntryKind::Jsr,
                    source_pc: SOURCE,
                    target_pc: TARGET,
                    return_pc: SOURCE + 2,
                    stack: ControlStack {
                        bank,
                        before: sp,
                        after: sp - 4,
                        pc_slot: sp - 4
                    },
                },
                ControlFlowEvent::Entry {
                    kind: ControlFlowEntryKind::Exception(9),
                    source_pc: TARGET,
                    target_pc: HANDLER,
                    return_pc: TARGET,
                    stack: ControlStack {
                        bank: ControlStackBank::Supervisor,
                        before: trace_sp,
                        after: trace_sp - 6,
                        pc_slot: trace_sp - 4
                    },
                },
            ]
        );
    }
    let (mut cpu, mut bus) = seed(0x4e75, 0x4e71, TRACE);
    put(&mut bus, 0x2000, &[0xabcd, 0x0400]);
    bus.handler(9);
    cpu.set_control_flow_recording(true);
    step(&mut cpu, &mut bus).unwrap();
    assert_eq!(
        events(&mut cpu),
        [
            ControlFlowEvent::Return {
                kind: ControlFlowReturnKind::Rts,
                source_pc: SOURCE,
                target_pc: TARGET,
                supervisor_after: false,
                stack: ControlStack {
                    bank: ControlStackBank::User,
                    before: 0x2000,
                    after: 0x2004,
                    pc_slot: 0x2000
                },
            },
            ControlFlowEvent::Entry {
                kind: ControlFlowEntryKind::Exception(9),
                source_pc: TARGET,
                target_pc: HANDLER,
                return_pc: TARGET,
                stack: ControlStack {
                    bank: ControlStackBank::Supervisor,
                    before: 0x1000,
                    after: 0xffa,
                    pc_slot: 0xffc
                },
            },
        ]
    );
    let (mut cpu, mut bus) = seed(0x4e40, 0x4e71, SUPERVISOR | TRACE);
    bus.handler(32);
    bus.handler(9);
    cpu.set_control_flow_recording(true);
    step(&mut cpu, &mut bus).unwrap();
    assert_eq!(
        events(&mut cpu),
        [
            ControlFlowEvent::Entry {
                kind: ControlFlowEntryKind::Exception(32),
                source_pc: SOURCE,
                target_pc: HANDLER,
                return_pc: SOURCE + 2,
                stack: ControlStack {
                    bank: ControlStackBank::Supervisor,
                    before: 0x1000,
                    after: 0xffa,
                    pc_slot: 0xffc
                },
            },
            ControlFlowEvent::Entry {
                kind: ControlFlowEntryKind::Exception(9),
                source_pc: HANDLER,
                target_pc: HANDLER,
                return_pc: HANDLER,
                stack: ControlStack {
                    bank: ControlStackBank::Supervisor,
                    before: 0xffa,
                    after: 0xff4,
                    pc_slot: 0xff6
                },
            },
        ]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn irq_trace_priority_privilege_and_stop_record_actual_entered_frames_only() {
    for (level, response, vector) in [
        (4, InterruptVector::Vector(0x45), 0x45),
        (6, InterruptVector::Autovector, 30),
        (7, InterruptVector::Spurious, 24),
    ] {
        let (mut cpu, mut bus) = seed(0x4e71, 0x4e71, if level == 7 { 0x2700 } else { 0 });
        cpu.interrupt_input().set_level(level);
        cpu.set_control_flow_recording(true);
        bus.response = Some(response);
        bus.handler(vector);
        assert!(
            matches!(step(&mut cpu, &mut bus).unwrap(), TimedOutcome::Interrupt { vector: actual, .. } if actual == vector)
        );
        assert_eq!(
            events(&mut cpu),
            [ControlFlowEvent::Entry {
                kind: ControlFlowEntryKind::Interrupt { level, vector },
                source_pc: SOURCE,
                target_pc: HANDLER,
                return_pc: SOURCE,
                stack: ControlStack {
                    bank: ControlStackBank::Supervisor,
                    before: 0x1000,
                    after: 0xffa,
                    pc_slot: 0xffc
                },
            }]
        );
    }
    let (mut cpu, mut bus) = seed(0x4e71, 0x4e71, 0);
    cpu.pending_trace = true;
    cpu.interrupt_input().set_level(4);
    cpu.set_control_flow_recording(true);
    bus.handler(9);
    bus.handler(28);
    assert!(matches!(
        step(&mut cpu, &mut bus).unwrap(),
        TimedOutcome::Trace { .. }
    ));
    assert!(matches!(
        &events(&mut cpu)[..],
        [ControlFlowEvent::Entry {
            kind: ControlFlowEntryKind::Exception(9),
            ..
        }]
    ));
    assert!(matches!(
        step(&mut cpu, &mut bus).unwrap(),
        TimedOutcome::Interrupt { level: 4, .. }
    ));
    assert!(matches!(
        &events(&mut cpu)[..],
        [ControlFlowEvent::Entry {
            kind: ControlFlowEntryKind::Interrupt {
                level: 4,
                vector: 28
            },
            ..
        }]
    ));

    let (mut cpu, mut bus) = seed(0x4e72, 0x2000, SUPERVISOR);
    cpu.set_control_flow_recording(true);
    step(&mut cpu, &mut bus).unwrap();
    assert!(events(&mut cpu).is_empty());
    assert_eq!(step(&mut cpu, &mut bus).unwrap(), TimedOutcome::Stopped);
    assert!(events(&mut cpu).is_empty());
    cpu.interrupt_input().set_level(4);
    bus.handler(28);
    step(&mut cpu, &mut bus).unwrap();
    assert!(matches!(&events(&mut cpu)[..], [ControlFlowEvent::Entry {
        kind: ControlFlowEntryKind::Interrupt { level: 4, vector: 28 }, source_pc, return_pc, ..
    }] if *source_pc == SOURCE + 4 && *return_pc == SOURCE + 4));

    let (mut cpu, mut bus) = seed(0x4e73, 0x4e71, 0);
    cpu.set_control_flow_recording(true);
    bus.handler(8);
    step(&mut cpu, &mut bus).unwrap();
    assert!(matches!(
        &events(&mut cpu)[..],
        [ControlFlowEvent::Entry {
            kind: ControlFlowEntryKind::Exception(8),
            source_pc: SOURCE,
            return_pc: SOURCE,
            ..
        }]
    ));
}
