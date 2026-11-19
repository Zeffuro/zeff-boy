use super::*;
use crate::TRACE;
use crate::timing::DebugCallRejection;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn failed_transfers_publish_no_uncommitted_frame_and_late_trace_keeps_committed_prefix() {
    for (opcode, transfers) in [
        (0x4e90, 4),
        (0x4e75, 4),
        (0x4e73, 5),
        (0x4e77, 5),
        (0x4e40, 7),
        (0x4e71, 8),
        (0x6106, 4),
    ] {
        for fail in 0..transfers {
            let (mut cpu, mut bus) = seed(opcode, 0x4e71, SUPERVISOR);
            cpu.registers.a[0] = TARGET;
            put(
                &mut bus,
                0x1000,
                if opcode == 0x4e75 {
                    &[0xabcd, 0x0400]
                } else {
                    &[0x0015, 0xabcd, 0x0400]
                },
            );
            bus.handler(32);
            if opcode == 0x4e71 {
                cpu.interrupt_input().set_level(4);
                bus.handler(28);
            }
            bus.wait = 2;
            bus.fail = Some(fail);
            cpu.set_control_flow_recording(true);
            assert!(
                matches!(step(&mut cpu, &mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
            );
            assert!(events(&mut cpu).is_empty());
            assert_eq!(bus.transfers, fail + 1);
            match opcode {
                0x4e90 => assert_eq!(cpu.registers.ssp, if fail == 0 { 0x1000 } else { 0xffc }),
                0x6106 => assert_eq!(cpu.registers.ssp, 0xffc),
                0x4e75 => assert_eq!(cpu.registers.ssp, if fail < 2 { 0x1000 } else { 0x1004 }),
                0x4e73 => {
                    assert_eq!(cpu.registers.ssp, if fail < 3 { 0x1000 } else { 0x1006 });
                    assert_eq!(cpu.registers.supervisor(), fail < 3);
                }
                0x4e77 => assert_eq!(cpu.registers.ssp, if fail < 2 { 0x1000 } else { 0x1006 }),
                0x4e40 | 0x4e71 => assert_eq!(cpu.registers.ssp, 0x1000),
                _ => unreachable!(),
            }
            let before = bus.clone();
            assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
            assert_eq!(bus, before);
            assert!(events(&mut cpu).is_empty());
        }
    }
    for opcode in [0x4e90, 0x6101, 0x4e75] {
        let (mut cpu, mut bus) = seed(opcode, 0x4e71, SUPERVISOR);
        cpu.registers.a[0] = TARGET | 1;
        put(&mut bus, 0x1000, &[0xabcd, 0x0401]);
        cpu.set_control_flow_recording(true);
        assert!(matches!(
            step(&mut cpu, &mut bus),
            Err(TimingFault::AddressError { .. })
        ));
        assert!(events(&mut cpu).is_empty());
        assert_eq!(bus.transfers, if opcode == 0x4e90 { 0 } else { 2 });
    }
    for opcode in [0x4e90, 0x4e75] {
        for wait in [0, 3] {
            for fail in 4..11 {
                let (mut cpu, mut bus) = seed(opcode, 0x4e71, SUPERVISOR | TRACE);
                cpu.registers.a[0] = TARGET;
                put(&mut bus, 0x1000, &[0xabcd, 0x0400]);
                bus.handler(9);
                bus.wait = wait;
                bus.fail = Some(fail);
                cpu.set_control_flow_recording(true);
                assert!(
                    matches!(step(&mut cpu, &mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                );
                assert_eq!(
                    events(&mut cpu),
                    if opcode == 0x4e90 {
                        vec![ControlFlowEvent::Entry {
                            kind: ControlFlowEntryKind::Jsr,
                            source_pc: SOURCE,
                            target_pc: TARGET,
                            return_pc: SOURCE + 2,
                            stack: ControlStack {
                                bank: ControlStackBank::Supervisor,
                                before: 0x1000,
                                after: 0xffc,
                                pc_slot: 0xffc,
                            },
                        }]
                    } else {
                        vec![ControlFlowEvent::Return {
                            kind: ControlFlowReturnKind::Rts,
                            source_pc: SOURCE,
                            target_pc: TARGET,
                            supervisor_after: true,
                            stack: ControlStack {
                                bank: ControlStackBank::Supervisor,
                                before: 0x1000,
                                after: 0x1004,
                                pc_slot: 0x1000,
                            },
                        }]
                    }
                );
                assert_eq!(bus.transfers, fail + 1);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn debugger_entry_is_not_a_jsr_instruction_and_only_successful_entry_and_rts_publish() {
    for status in [0, SUPERVISOR] {
        for fail in 0..4 {
            let (mut cpu, mut bus) = seed(0x4e71, 0x4e71, status);
            bus.fail = Some(fail);
            let mut plain = cpu.clone();
            let mut reference = bus.clone();
            cpu.set_control_flow_recording(true);
            assert_eq!(
                cpu.begin_debug_call(&mut bus, TARGET).map(|_| ()),
                plain.begin_debug_call(&mut reference, TARGET).map(|_| ()),
            );
            assert_eq!(cpu, plain);
            assert_eq!(bus, reference);
            assert_eq!(bus.transfers, fail + 1);
            assert!(events(&mut cpu).is_empty());
        }
        let (mut cpu, mut bus) = seed(0x4e71, 0x4e71, status);
        let sp = cpu.registers.stack_pointer();
        cpu.set_control_flow_recording(true);
        let context = cpu.begin_debug_call(&mut bus, TARGET).unwrap();
        let prefix = cpu.clone().drain_control_flow_events();
        let before = bus.clone();
        assert!(matches!(
            cpu.begin_debug_call(&mut bus, TARGET | 1),
            Err(TimingFault::DebugCallRejected(
                DebugCallRejection::OddTarget
            ))
        ));
        assert_eq!(bus, before);
        assert_eq!(cpu.clone().drain_control_flow_events(), prefix);
        assert_eq!(
            events(&mut cpu),
            [ControlFlowEvent::Entry {
                kind: ControlFlowEntryKind::Debugger,
                source_pc: SOURCE,
                target_pc: TARGET,
                return_pc: SOURCE,
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
        step(&mut cpu, &mut bus).unwrap();
        assert!(cpu.debug_call_returned(&context));
        assert!(matches!(
            &events(&mut cpu)[..],
            [ControlFlowEvent::Return {
                kind: ControlFlowReturnKind::Rts,
                source_pc: TARGET,
                target_pc: SOURCE,
                ..
            }]
        ));
        cpu.finish_debug_call(context).unwrap();
        assert_eq!(cpu.registers.stack_pointer(), sp);
        assert_eq!(cpu.registers.status_register(), status);
        assert!(events(&mut cpu).is_empty());
    }
}
