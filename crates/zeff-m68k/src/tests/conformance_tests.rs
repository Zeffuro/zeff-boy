use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

use crate::bus::Direction;
use crate::test_bus::TestBus;
use crate::{CARRY, Cpu, EXTEND, NEGATIVE, OVERFLOW, Registers, SUPERVISOR, StepOutcome, ZERO};

struct Adapter {
    cpu: Cpu,
    bus: TestBus,
}

impl CpuConformanceAdapter for Adapter {
    type State = Registers;
    const TRACE_TIMING: TraceTiming = TraceTiming::OrderOnly;

    fn from_case(case: &CpuCase<Registers>) -> Self {
        let mut bus = TestBus::default();
        for block in &case.initial_memory {
            bus.seed(block.start, &block.bytes);
        }
        Self {
            cpu: Cpu {
                registers: case.initial_state,
                ..Cpu::new()
            },
            bus,
        }
    }

    fn snapshot(&self) -> Registers {
        self.cpu.registers
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus.peek(address)
    }

    fn step(&mut self) -> StepObservation {
        let StepOutcome::Executed { nominal_cycles, .. } = self.cpu.step(&mut self.bus) else {
            panic!("conformance instruction failed");
        };
        assert!(
            self.bus
                .transfers
                .iter()
                .all(|transfer| transfer.direction == Direction::Read)
        );
        StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: u64::from(nominal_cycles),
            master_ticks: MasterTicks::new(u64::from(nominal_cycles)),
            master_rate: ClockRate::from_hz(1),
            bus_events: self
                .bus
                .transfers
                .iter()
                .map(|transfer| read_event(transfer.access.logical_address, transfer.value))
                .collect(),
        }
    }
}

pub fn populated_registers(pc: u32) -> Registers {
    let mut registers = Registers::new();
    registers.d = std::array::from_fn(|index| 0x1234_0000 + u32::try_from(index).unwrap());
    registers.a = std::array::from_fn(|index| 0x8765_0000 + u32::try_from(index).unwrap());
    registers.usp = 0xabcd_0123;
    registers.ssp = 0x9876_fedc;
    registers.pc = pc;
    registers
        .set_status_register(SUPERVISOR | 0x0500 | EXTEND | NEGATIVE | ZERO | OVERFLOW | CARRY);
    registers
}

fn read_event(address: u32, value: u16) -> BusAccessEvent {
    BusAccessEvent::Read {
        at: None,
        space: TraceWriteKind::Memory,
        addr: address,
        value: u32::from(value),
        width: TraceWriteWidth::Halfword,
        mapped_addr: Some(address & 0x00ff_fffe),
    }
}

fn case(initial: Registers, expected: Registers, words: &[u16], cycles: u64) {
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
    let block = MemoryBlock::new(initial.pc, bytes);
    let events = words
        .iter()
        .enumerate()
        .map(|(offset, &value)| {
            read_event(
                initial.pc.wrapping_add(u32::try_from(offset).unwrap() * 2),
                value,
            )
        })
        .collect();
    assert_case::<Adapter>(&CpuCase {
        initial_state: initial,
        expected_state: expected,
        initial_memory: vec![block.clone()],
        expected_memory: vec![block],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: cycles,
            master_ticks: MasterTicks::new(cycles),
            master_rate: ClockRate::from_hz(1),
            bus_events: events,
        },
    });
}

#[test]
fn nop_preserves_full_state_except_pc_including_architectural_wrap() {
    for pc in [0x1000, 0xffff_fffe] {
        let initial = populated_registers(pc);
        let mut expected = initial;
        expected.pc = pc.wrapping_add(2);
        case(initial, expected, &[0x4e71], 4);
    }
}

#[test]
fn moveq_all_destinations_and_immediates_have_exact_full_state() {
    for destination in 0..8_u16 {
        for immediate in 0..=255_u16 {
            for initial_ccr in [0, EXTEND | NEGATIVE | ZERO | OVERFLOW | CARRY] {
                let mut initial = populated_registers(0x1234_1000);
                initial.set_status_register(SUPERVISOR | 0x0500 | initial_ccr);
                let mut expected = initial;
                let signed = if immediate < 128 {
                    i32::from(immediate)
                } else {
                    i32::from(immediate) - 256
                };
                expected.d[usize::from(destination)] = signed as u32;
                let flags = match signed.cmp(&0) {
                    std::cmp::Ordering::Less => NEGATIVE,
                    std::cmp::Ordering::Equal => ZERO,
                    std::cmp::Ordering::Greater => 0,
                };
                expected.set_status_register(SUPERVISOR | 0x0500 | (initial_ccr & EXTEND) | flags);
                expected.pc += 2;
                case(
                    initial,
                    expected,
                    &[0x7000 | (destination << 9) | immediate],
                    4,
                );
            }
        }
    }
}

#[test]
fn bra_all_nonzero_byte_encodings_preserve_flags_and_use_opcode_base() {
    for encoded in 1..=255_u16 {
        let initial = populated_registers(0x2000);
        let mut expected = initial;
        let displacement = if encoded < 128 {
            i32::from(encoded)
        } else {
            i32::from(encoded) - 256
        };
        expected.pc = 0x2002_u32.wrapping_add_signed(displacement);
        case(initial, expected, &[0x6000 | encoded], 10);
    }
}

#[test]
fn bra_word_signed_boundaries_use_opcode_plus_two_and_wrap() {
    for pc in [0, 0x00ff_fffe, 0xffff_fffe] {
        for (word, displacement) in [
            (0, 0),
            (1, 1),
            (0x7fff, 32767),
            (0x8000, -32768),
            (0xfffe, -2),
            (0xffff, -1),
        ] {
            let initial = populated_registers(pc);
            let mut expected = initial;
            expected.pc = pc.wrapping_add(2).wrapping_add_signed(displacement);
            case(initial, expected, &[0x6000, word], 10);
        }
    }
}
