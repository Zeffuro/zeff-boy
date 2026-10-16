use std::collections::BTreeMap;

use super::*;
use crate::{EXTEND, NEGATIVE, ZERO};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Read(BusAccess, CpuClocks),
    Idle(CpuClocks, u32),
}

#[derive(Default)]
struct Bus {
    words: BTreeMap<u32, u16>,
    events: Vec<Event>,
    clock: u64,
    waits: u32,
    fail_read: Option<usize>,
    reads: usize,
}

impl ClockedBus for Bus {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        assert_eq!(start.0, self.clock);
        self.events.push(Event::Read(access, start));
        self.clock += 4 + u64::from(self.waits);
        let index = self.reads;
        self.reads += 1;
        CycleResult {
            result: if self.fail_read == Some(index) {
                Err(index)
            } else {
                Ok(*self.words.get(&access.bus_address).unwrap_or(&0x4e71))
            },
            wait_clocks: self.waits,
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), usize> {
        panic!("unexpected write")
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        self.events.push(Event::Idle(start, clocks));
        self.clock += u64::from(clocks);
    }
}

fn seeded(pc: u32, words: [u16; 2]) -> PrefetchCpu {
    let mut registers = Registers::new();
    registers.pc = pc;
    PrefetchCpu::from_prefetched(registers, words, CpuClocks(0)).unwrap()
}

fn reads(bus: &Bus) -> Vec<(u32, u64)> {
    bus.events
        .iter()
        .filter_map(|event| match event {
            Event::Read(access, clock) => Some((access.logical_address, clock.0)),
            _ => None,
        })
        .collect()
}

#[test]
fn pipeline_retains_overwritten_next_opcode() {
    let mut cpu = seeded(0x100, [0x4e71, 0x7080]);
    let mut bus = Bus::default();
    bus.words.insert(0x102, 0x707f);
    bus.words.insert(0x104, 0x7201);
    assert_eq!(cpu.step(&mut bus).unwrap().clocks, 4);
    assert_eq!(cpu.queued_words(), Some([0x7080, 0x7201]));
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers.d[0], 0xffff_ff80);
    assert_eq!(reads(&bus), [(0x104, 0), (0x106, 4)]);
}

#[test]
fn moveq_full_domain_preserves_x_and_has_one_refill() {
    for register in 0..8 {
        for immediate in 0..256 {
            for extend in [0, EXTEND] {
                let op = 0x7000 | (register << 9) | immediate;
                let mut cpu = seeded(0x100, [op, 0x4e71]);
                cpu.registers.set_status_register(0x271f | extend);
                cpu.registers
                    .set_status_register((cpu.registers.status_register() & !EXTEND) | extend);
                let mut bus = Bus::default();
                assert_eq!(
                    cpu.step(&mut bus).unwrap(),
                    TimedStep {
                        opcode: op,
                        clocks: 4,
                        exception: None
                    }
                );
                assert_eq!(
                    cpu.registers.d[usize::from(register)],
                    i32::from(immediate as u8 as i8) as u32
                );
                assert_eq!(
                    cpu.registers.status_register() & 0x1f,
                    extend
                        | if immediate == 0 {
                            ZERO
                        } else if immediate & 0x80 != 0 {
                            NEGATIVE
                        } else {
                            0
                        }
                );
                assert_eq!(reads(&bus), [(0x104, 0)]);
            }
        }
    }
}

#[test]
fn branch_uses_queued_displacement_and_flushes_sequential_words() {
    for (pc, opcode, extension, target) in [
        (0x100, 0x6006, 0x4e71, 0x108),
        (0x100, 0x60fe, 0x4e71, 0x100),
        (0x100, 0x6000, 0xfffc, 0xfe),
        (0xffff_fffe, 0x6000, 0x0004, 4),
    ] {
        let mut cpu = seeded(pc, [opcode, extension]);
        let mut bus = Bus::default();
        bus.words
            .insert(pc.wrapping_add(2) & BUS_ADDRESS_MASK, 0x0002);
        bus.words.insert(target & BUS_ADDRESS_MASK, 0x7001);
        bus.words
            .insert(target.wrapping_add(2) & BUS_ADDRESS_MASK, 0x7202);
        assert_eq!(cpu.step(&mut bus).unwrap().clocks, 10);
        assert_eq!(cpu.registers.pc, target);
        assert_eq!(cpu.queued_words(), Some([0x7001, 0x7202]));
        assert_eq!(bus.events[0], Event::Idle(CpuClocks(0), 2));
        assert_eq!(reads(&bus), [(target, 2), (target.wrapping_add(2), 6)]);
    }
}

#[test]
fn waits_advance_each_transfer_once_including_failed_refill() {
    let mut cpu = seeded(0x100, [0x6006, 0x4e71]);
    let mut bus = Bus {
        waits: 3,
        fail_read: Some(1),
        ..Bus::default()
    };
    let old = cpu.registers;
    assert!(matches!(
        cpu.step(&mut bus),
        Err(TimingFault::Bus { source: 1, .. })
    ));
    assert_eq!(cpu.clock(), CpuClocks(16));
    assert_eq!(cpu.registers, old);
    assert_eq!(reads(&bus), [(0x108, 2), (0x10a, 9)]);
    let events = bus.events.clone();
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::Frozen));
    assert_eq!(bus.events, events);
}

#[test]
fn logical_wrap_and_program_space_survive_pipeline_shift() {
    let mut cpu = seeded(0xffff_fffc, [0x4e71, 0x4e71]);
    cpu.registers.set_status_register(0);
    let mut bus = Bus::default();
    cpu.step(&mut bus).unwrap();
    let Event::Read(access, _) = bus.events[0] else {
        panic!("missing fetch")
    };
    assert_eq!(access.logical_address, 0);
    assert_eq!(access.bus_address, 0);
    assert_eq!(access.function_code, FunctionCode::UserProgram);
    assert_eq!(access.kind, AccessKind::Prefetch);
    assert_eq!(access.lanes, ByteLanes::Both);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers.pc, 0);
}

#[test]
fn unsupported_timing_and_external_pc_changes_do_not_touch_bus() {
    let mut cpu = seeded(0x100, [0x4e74, 0x4e71]);
    let mut bus = Bus::default();
    assert_eq!(
        cpu.step(&mut bus),
        Err(TimingFault::UnsupportedInstruction {
            pc: 0x100,
            opcode: 0x4e74
        })
    );
    cpu.registers.pc = 0x200;
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::Unprimed));
    assert!(bus.events.is_empty());
}

#[test]
fn prime_is_explicit_and_address_errors_never_transfer() {
    let mut cpu = PrefetchCpu::default();
    cpu.registers.pc = 0x100;
    let mut bus = Bus {
        waits: 2,
        ..Bus::default()
    };
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::Unprimed));
    cpu.prime(&mut bus).unwrap();
    assert_eq!(cpu.clock(), CpuClocks(12));
    assert_eq!(reads(&bus), [(0x100, 0), (0x102, 6)]);
    let mut odd = PrefetchCpu::default();
    odd.registers.pc = 1;
    let mut bus = Bus::default();
    assert_eq!(
        odd.prime(&mut bus),
        Err(TimingFault::AddressError { address: 1 })
    );
    assert!(bus.events.is_empty());
    assert!(PrefetchCpu::from_prefetched(odd.registers, [0, 0], CpuClocks(0)).is_none());
}

#[test]
fn conditional_branches_cover_all_flags_and_retained_words() {
    for condition in 2..16 {
        for flags in 0..32 {
            for word in [false, true] {
                let opcode = 0x6000 | (condition << 8) | if word { 0 } else { 6 };
                let mut cpu = seeded(0x100, [opcode, if word { 6 } else { 0x7201 }]);
                cpu.registers.set_status_register(0x2000 | flags);
                let mut bus = Bus::default();
                let taken = crate::alu::condition(cpu.registers.status_register(), condition);
                let result = cpu.step(&mut bus).unwrap();
                assert_eq!(
                    result.clocks,
                    if taken {
                        10
                    } else if word {
                        12
                    } else {
                        8
                    }
                );
                assert_eq!(
                    cpu.registers.pc,
                    if taken {
                        0x108
                    } else if word {
                        0x104
                    } else {
                        0x102
                    }
                );
                assert_eq!(cpu.registers.status_register(), 0x2000 | flags);
                assert_eq!(
                    reads(&bus),
                    if taken {
                        vec![(0x108, 2), (0x10a, 6)]
                    } else if word {
                        vec![(0x104, 4), (0x106, 8)]
                    } else {
                        vec![(0x104, 4)]
                    }
                );
                if !taken && !word {
                    assert_eq!(cpu.queued_words().unwrap()[0], 0x7201);
                }
            }
        }
    }
}

#[test]
fn clone_preserves_pipeline_and_waited_continuation() {
    let mut original = seeded(0x100, [0x4e71, 0x7080]);
    let mut first_bus = Bus {
        waits: 1,
        ..Bus::default()
    };
    original.step(&mut first_bus).unwrap();
    let mut restored = original.clone();
    let mut second_bus = Bus {
        waits: 1,
        clock: first_bus.clock,
        ..Bus::default()
    };
    let first = original.step(&mut first_bus).unwrap();
    let second = restored.step(&mut second_bus).unwrap();
    assert_eq!(first, second);
    assert_eq!(original, restored);
    assert_eq!(first_bus.events.last(), second_bus.events.last());
    assert_eq!(original.registers.d[0], 0xffff_ff80);
}

#[test]
fn prime_failures_freeze_after_each_completed_transfer() {
    for index in 0..2 {
        let mut cpu = PrefetchCpu::default();
        cpu.registers.pc = 0x100;
        let mut bus = Bus {
            waits: 1,
            fail_read: Some(index),
            ..Bus::default()
        };
        assert!(
            matches!(cpu.prime(&mut bus), Err(TimingFault::Bus { source, .. }) if source == index)
        );
        assert_eq!(cpu.clock().0, 5 * (index as u64 + 1));
        assert_eq!(cpu.queued_words(), None);
        let events = bus.events.clone();
        assert_eq!(cpu.prime(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
    }
}
