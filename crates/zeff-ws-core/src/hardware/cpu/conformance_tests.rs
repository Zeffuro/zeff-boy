use super::*;
use crate::hardware::bus::{Bus, DebugTraceMode};
use crate::hardware::cartridge::{Cartridge, compute_footer_checksum};
use crate::hardware::constants::CPU_CLOCK_HZ;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct WonderSwanState {
    registers: [u16; 8],
    segments: [u16; 4],
    ip: u16,
    flags: u16,
    state: CpuState,
    interrupt_shadow: u8,
    brk_shadow: u8,
    last_mul_overflow: bool,
}

struct WonderSwanAdapter {
    cpu: Cpu,
    bus: Bus,
}

impl CpuConformanceAdapter for WonderSwanAdapter {
    type State = WonderSwanState;

    const TRACE_TIMING: TraceTiming = TraceTiming::OrderOnly;

    fn from_case(case: &CpuCase<Self::State>) -> Self {
        let mut adapter = Self {
            cpu: Cpu::new(),
            bus: test_bus(),
        };
        adapter.apply_state(&case.initial_state);
        for block in &case.initial_memory {
            for (offset, &value) in block.bytes.iter().enumerate() {
                adapter
                    .bus
                    .write8(block.start.wrapping_add(offset as u32), value);
            }
        }
        adapter.bus.debug_trace_events.clear();
        adapter
    }

    fn snapshot(&self) -> Self::State {
        WonderSwanState {
            registers: self.cpu.regs,
            segments: self.cpu.segments,
            ip: self.cpu.ip,
            flags: self.cpu.flags,
            state: self.cpu.state,
            interrupt_shadow: self.cpu.interrupt_shadow,
            brk_shadow: self.cpu.brk_shadow,
            last_mul_overflow: self.cpu.last_mul_overflow,
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus.peek8(address)
    }

    fn step(&mut self) -> StepObservation {
        self.bus.debug_trace_mode = DebugTraceMode::MemoryAndIo;
        self.bus.debug_trace_events.clear();
        let before_cycles = self.cpu.cycles;
        let fetched = self.cpu.step(&mut self.bus);
        let cpu_cycles = self.cpu.cycles.wrapping_sub(before_cycles);
        let bus_events = self.bus.take_debug_trace_events();
        self.bus.debug_trace_mode = DebugTraceMode::None;

        StepObservation {
            kind: if self.cpu.last_step_was_interrupt {
                StepKind::Interrupt
            } else if self.cpu.last_trap.is_some() {
                StepKind::Trap
            } else if fetched.is_some() {
                StepKind::Instruction
            } else {
                StepKind::Idle
            },
            cpu_cycles,
            master_ticks: MasterTicks::new(cpu_cycles),
            master_rate: ClockRate::from_hz(u64::from(CPU_CLOCK_HZ)),
            bus_events,
        }
    }
}

impl WonderSwanAdapter {
    fn apply_state(&mut self, state: &WonderSwanState) {
        self.cpu.regs = state.registers;
        self.cpu.segments = state.segments;
        self.cpu.ip = state.ip;
        self.cpu.flags = state.flags;
        self.cpu.state = state.state;
        self.cpu.cycles = 0;
        self.cpu.last_fetch = None;
        self.cpu.last_opcode = 0;
        self.cpu.last_trap = None;
        self.cpu.interrupt_shadow = state.interrupt_shadow;
        self.cpu.brk_shadow = state.brk_shadow;
        self.cpu.last_step_was_interrupt = false;
        self.cpu.last_mul_overflow = state.last_mul_overflow;
        self.cpu.instruction_bytes = [0; MAX_INSTRUCTION_BYTES];
        self.cpu.instruction_len = 0;
    }
}

fn test_bus() -> Bus {
    let mut rom = vec![0xFF; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Bus::new(Cartridge::load(&rom).expect("test cartridge"))
}

#[test]
fn common_adapter_observes_segmented_byte_store() {
    let mut registers = [0; 8];
    registers[REG_AX as usize] = 0x005A;
    let mut segments = [0; 4];
    segments[SegmentRegister::Cs.index()] = 0x0100;
    segments[SegmentRegister::Ds.index()] = 0x0120;
    let initial_state = WonderSwanState {
        registers,
        segments,
        ip: 0,
        flags: FLAG_FIXED,
        state: CpuState::Running,
        interrupt_shadow: 0,
        brk_shadow: 0,
        last_mul_overflow: false,
    };
    let case = CpuCase {
        initial_state: initial_state.clone(),
        expected_state: WonderSwanState {
            ip: 3,
            ..initial_state
        },
        initial_memory: vec![MemoryBlock::new(0x01000, [0xA2, 0x30, 0x00])],
        expected_memory: vec![MemoryBlock::new(0x01230, [0x5A])],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 1,
            master_ticks: MasterTicks::new(1),
            master_rate: ClockRate::from_hz(3_072_000),
            bus_events: vec![
                read(0x01000, 0xA2),
                read(0x01001, 0x30),
                read(0x01002, 0x00),
                BusAccessEvent::Write {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0x01230,
                    old_value: 0,
                    written_value: 0x5A,
                    new_value: 0x5A,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<WonderSwanAdapter>(&case);
}

fn read(addr: u32, value: u32) -> BusAccessEvent {
    BusAccessEvent::Read {
        at: None,
        space: TraceWriteKind::Memory,
        addr,
        value,
        width: TraceWriteWidth::Byte,
        mapped_addr: None,
    }
}
