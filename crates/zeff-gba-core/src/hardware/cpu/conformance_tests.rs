use super::*;
use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct GbaState {
    registers: [u32; 16],
    cpsr: u32,
    state: CpuState,
}

struct GbaAdapter {
    cpu: Cpu,
    bus: Bus,
}

impl CpuConformanceAdapter for GbaAdapter {
    type State = GbaState;

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
        adapter.bus.debug_trace_events.borrow_mut().clear();
        adapter
    }

    fn snapshot(&self) -> Self::State {
        GbaState {
            registers: self.cpu.regs,
            cpsr: self.cpu.cpsr,
            state: self.cpu.state,
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus.peek8(address)
    }

    fn step(&mut self) -> StepObservation {
        self.bus.debug_trace_enabled = true;
        self.bus.debug_trace_reads = true;
        self.bus.debug_trace_writes = true;
        self.bus.debug_trace_events.borrow_mut().clear();

        self.cpu
            .step(&mut self.bus)
            .expect("conformance fixture must execute");
        let bus_events = std::mem::take(&mut *self.bus.debug_trace_events.borrow_mut());
        self.bus.debug_trace_enabled = false;
        self.bus.debug_trace_reads = false;
        self.bus.debug_trace_writes = false;

        StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: self.cpu.cycles,
            master_ticks: MasterTicks::new(self.cpu.cycles),
            master_rate: ClockRate::from_hz(16_777_216),
            bus_events,
        }
    }
}

impl GbaAdapter {
    fn apply_state(&mut self, state: &GbaState) {
        self.cpu.regs = state.registers;
        self.cpu.cpsr = state.cpsr;
        self.cpu.state = state.state;
        self.cpu.cycles = 0;
        self.cpu.last_opcode_pc = state.registers[15];
        self.cpu.break_after_next_stub = false;
        self.cpu.next_fetch_sequential = false;
        self.cpu.last_fetch = None;
        self.cpu.pipeline.clear();
    }
}

fn test_bus() -> Bus {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    Bus::new(Cartridge::load(&rom).expect("test cartridge"), 48_000)
}

#[test]
fn common_adapter_observes_ewram_store_order() {
    let mut registers = [0; 16];
    registers[0] = 0x0200_0010;
    registers[1] = 0x1234_5678;
    registers[15] = 0x0200_0000;
    let initial_state = GbaState {
        registers,
        cpsr: 0x1F,
        state: CpuState::Running,
    };
    let mut expected_registers = registers;
    expected_registers[15] = 0x0200_0004;
    let case = CpuCase {
        initial_state,
        expected_state: GbaState {
            registers: expected_registers,
            cpsr: 0x1F,
            state: CpuState::Running,
        },
        initial_memory: vec![MemoryBlock::new(0x0200_0000, 0xE580_1000_u32.to_le_bytes())],
        expected_memory: vec![MemoryBlock::new(0x0200_0010, 0x1234_5678_u32.to_le_bytes())],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 9,
            master_ticks: MasterTicks::new(9),
            master_rate: ClockRate::from_hz(16_777_216),
            bus_events: vec![
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0x0200_0000,
                    value: 0xE580_1000,
                    width: TraceWriteWidth::Word,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0x0200_0004,
                    value: 0,
                    width: TraceWriteWidth::Word,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0x0200_0008,
                    value: 0,
                    width: TraceWriteWidth::Word,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0x0200_0010,
                    old_value: 0,
                    written_value: 0x1234_5678,
                    new_value: 0x1234_5678,
                    width: TraceWriteWidth::Word,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<GbaAdapter>(&case);
}
