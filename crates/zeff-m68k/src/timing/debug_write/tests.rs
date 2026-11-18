use super::*;
use crate::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::timing::{CycleResult, InstructionByteOrigin, InstructionInput, Pipeline};
use crate::{SUPERVISOR, TRACE};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn literal_data_lanes_waits_preserve_architecture_origins_irq_and_control_metadata() {
    for bank in [0, SUPERVISOR] {
        for wait in [0, 1, 7] {
            for recording in [false, true] {
                let mut cpu = cpu(bank | TRACE | 0x15);
                cpu.registers.ssp |= 1;
                cpu.registers.usp |= 1;
                cpu.set_control_flow_recording(recording);
                assert!(cpu.interrupts.set_level(6));
                assert!(cpu.interrupts.restore_observation(true, 6));
                cpu.pipeline = Some(Pipeline::sampled(
                    cpu.registers.pc,
                    [
                        InstructionInput {
                            value: 0x7003,
                            origin: [
                                InstructionByteOrigin::storage(0x100).unwrap(),
                                InstructionByteOrigin::storage(0x101).unwrap(),
                            ],
                        },
                        InstructionInput {
                            value: 0x4e71,
                            origin: [
                                InstructionByteOrigin::storage(0x102).unwrap(),
                                InstructionByteOrigin::storage(0x103).unwrap(),
                            ],
                        },
                    ],
                ));
                let before = cpu.clone();
                let mut bus = Bus {
                    wait,
                    ..Bus::default()
                };
                let cases = [
                    (
                        DebugDataWrite::Byte {
                            address: 0xa11100,
                            value: 1,
                        },
                        0xa11100,
                        0xa11100,
                        ByteLanes::Upper,
                        0x0100,
                    ),
                    (
                        DebugDataWrite::Byte {
                            address: 0xff0101,
                            value: 0xa5,
                        },
                        0xff0101,
                        0xff0100,
                        ByteLanes::Lower,
                        0x00a5,
                    ),
                    (
                        DebugDataWrite::Word {
                            address: 0xc00004,
                            value: 0x8f02,
                        },
                        0xc00004,
                        0xc00004,
                        ByteLanes::Both,
                        0x8f02,
                    ),
                ];
                for (write, logical_address, bus_address, lanes, value) in cases {
                    let start = cpu.clock.0;
                    assert_eq!(
                        cpu.debug_data_write(&mut bus, write),
                        Ok(CpuClocks(4 + u64::from(wait)))
                    );
                    assert_eq!(
                        bus.events.last(),
                        Some(&Event::Write(
                            BusAccess {
                                logical_address,
                                bus_address,
                                lanes,
                                kind: AccessKind::Data,
                                function_code: if bank == 0 {
                                    FunctionCode::UserData
                                } else {
                                    FunctionCode::SupervisorData
                                },
                            },
                            value,
                            start
                        ))
                    );
                    assert_eq!(cpu.registers, before.registers);
                    assert_eq!(
                        cpu.queued_instruction_inputs(),
                        before.queued_instruction_inputs()
                    );
                    assert_eq!(cpu.interrupts, before.interrupts);
                    assert_eq!(cpu.pending_trace, before.pending_trace);
                    assert_eq!(cpu.control_flow_recording(), recording);
                    assert_eq!(cpu.drain_control_flow_events().events().count(), 0);
                }
                assert_eq!(bus.events.len(), 3);
                assert_eq!(cpu.clock.0, 3 * (4 + u64::from(wait)));
                assert_eq!(bus.clock, cpu.clock.0);
            }
        }
    }
    for mask in [0, 0x0700] {
        for recording in [false, true] {
            let mut edge = cpu(SUPERVISOR | mask);
            edge.set_control_flow_recording(recording);
            assert!(edge.interrupt_input().set_level(7));
            assert!(edge.interrupt_input().set_level(0));
            let saved = *edge.interrupt_input();
            let queue = edge.queued_instruction_inputs();
            let mut bus = Bus::default();
            bus.handler(31);
            edge.debug_data_write(
                &mut bus,
                DebugDataWrite::Word {
                    address: 0xc00004,
                    value: 0x8f02,
                },
            )
            .unwrap();
            assert_eq!(*edge.interrupt_input(), saved);
            assert_eq!(edge.queued_instruction_inputs(), queue);
            assert!(matches!(
                edge.step_boundary(&mut bus).unwrap(),
                crate::timing::TimedOutcome::Interrupt {
                    level: 7,
                    vector: 31,
                    ..
                }
            ));
            assert_eq!(edge.registers.pc, 0x12345000);
            assert_eq!(
                edge.drain_control_flow_events().events().count(),
                usize::from(recording)
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rejected_boundaries_and_overflow_issue_no_transfer() {
    let write = DebugDataWrite::Word {
        address: 0xc00004,
        value: 0x8f02,
    };
    for kind in 0..7 {
        let mut cpu = cpu(SUPERVISOR);
        match kind {
            0 => cpu.frozen = true,
            1 => cpu.stopped = true,
            2 => cpu.pending_trace = true,
            3 => cpu.pipeline = None,
            4 => cpu.registers.pc += 2,
            5 => cpu.physical_inspection = PhysicalInspection::Unavailable,
            _ => {
                cpu.physical_state =
                    PhysicalEntryState::Critical(crate::timing::physical::EntryKind::ImportedGroup0)
            }
        }
        let before = cpu.clone();
        let mut bus = Bus::default();
        assert!(cpu.debug_data_write(&mut bus, write).is_err());
        assert_eq!(cpu, before);
        assert!(bus.events.is_empty());
    }
    let mut odd = cpu(SUPERVISOR);
    let mut expected = odd.clone();
    expected.frozen = true;
    let mut untouched = Bus::default();
    assert_eq!(
        odd.debug_data_write(
            &mut untouched,
            DebugDataWrite::Word {
                address: 0xc00005,
                value: 0x8f02,
            }
        ),
        Err(TimingFault::AddressError { address: 0xc00005 })
    );
    assert_eq!(odd, expected);
    assert!(untouched.events.is_empty());
    assert_eq!(untouched.clock, 0);
    let mut cpu = cpu(SUPERVISOR);
    cpu.clock = CpuClocks(u64::MAX - u64::from(u32::MAX) - 3);
    let before = cpu.clock;
    let mut bus = Bus::default();
    assert_eq!(
        cpu.debug_data_write(&mut bus, write),
        Err(TimingFault::ClockOverflow)
    );
    assert_eq!(cpu.clock, before);
    assert!(cpu.frozen);
    assert!(bus.events.is_empty());
}

struct CommitThenFail {
    bus: Bus,
}

impl ClockedBus for CommitThenFail {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.bus.read_cycle(access, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        let mut result = self.bus.write_cycle(access, value, start);
        result.result = Err(99);
        result
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bus_fault_retains_real_prefix_and_waits_without_architectural_commit() {
    let mut cpu = cpu(SUPERVISOR | TRACE);
    let before = cpu.clone();
    let mut bus = CommitThenFail {
        bus: Bus {
            wait: 3,
            ..Bus::default()
        },
    };
    let fault = cpu.debug_data_write(
        &mut bus,
        DebugDataWrite::Word {
            address: 0xc00004,
            value: 0x8f02,
        },
    );
    assert!(matches!(
        fault,
        Err(TimingFault::Bus {
            direction: Direction::Write,
            source: 99,
            ..
        })
    ));
    assert_eq!(bus.bus.words.get(&0xc00004), Some(&0x8f02));
    assert_eq!(bus.bus.clock, 7);
    assert_eq!(cpu.clock, CpuClocks(7));
    assert!(cpu.frozen);
    assert_eq!(cpu.registers, before.registers);
    assert_eq!(
        cpu.queued_instruction_inputs(),
        before.queued_instruction_inputs()
    );
    assert_eq!(cpu.interrupts, before.interrupts);
}
