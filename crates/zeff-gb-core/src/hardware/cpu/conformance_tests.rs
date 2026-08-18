use super::*;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::{CpuState, ImeState};
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct GbState {
    registers: [u8; 8],
    pc: u16,
    sp: u16,
    ime: ImeState,
    running: CpuState,
}

struct GbAdapter {
    cpu: Cpu,
    bus: Bus,
}

impl CpuConformanceAdapter for GbAdapter {
    type State = GbState;

    const TRACE_TIMING: TraceTiming = TraceTiming::Exact;

    fn from_case(case: &CpuCase<Self::State>) -> Self {
        let mut adapter = Self {
            cpu: Cpu::new(),
            bus: test_bus(),
        };
        adapter.apply_state(&case.initial_state);
        for block in &case.initial_memory {
            for (offset, &value) in block.bytes.iter().enumerate() {
                let offset = u32::try_from(offset).expect("test memory block offset");
                let address = u16::try_from(block.start.wrapping_add(offset))
                    .expect("GB test address must fit in 16 bits");
                adapter.bus.write_byte(address, value);
            }
        }
        adapter
    }

    fn snapshot(&self) -> Self::State {
        GbState {
            registers: [
                self.cpu.regs.a,
                self.cpu.regs.f,
                self.cpu.regs.b,
                self.cpu.regs.c,
                self.cpu.regs.d,
                self.cpu.regs.e,
                self.cpu.regs.h,
                self.cpu.regs.l,
            ],
            pc: self.cpu.pc,
            sp: self.cpu.sp,
            ime: self.cpu.ime,
            running: self.cpu.running,
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus
            .read_byte_raw(u16::try_from(address).expect("GB test address must fit in 16 bits"))
    }

    fn step(&mut self) -> StepObservation {
        let interrupt_pending =
            self.cpu.ime == ImeState::Enabled && self.bus.pending_interrupts_for_cpu() != 0;
        self.bus.trace_cpu_accesses = true;
        self.bus.begin_cpu_access_trace_at(MasterTicks::ZERO);
        let cpu_cycles = zeff_emu_common::cpu::CpuCore::step_cpu(&mut self.cpu, &mut self.bus);

        let mut bus_events = Vec::new();
        self.bus
            .drain_cpu_access_trace(|event| bus_events.push(event));
        self.bus.trace_cpu_accesses = false;

        StepObservation {
            kind: if interrupt_pending {
                StepKind::Interrupt
            } else {
                StepKind::Instruction
            },
            cpu_cycles,
            master_ticks: MasterTicks::new(self.cpu.last_step_master_ticks),
            master_rate: ClockRate::from_hz(4_194_304),
            bus_events,
        }
    }
}

impl GbAdapter {
    fn apply_state(&mut self, state: &GbState) {
        let [a, f, b, c, d, e, h, l] = state.registers;
        self.cpu.regs = Registers {
            a,
            f,
            b,
            c,
            d,
            e,
            h,
            l,
        };
        self.cpu.pc = state.pc;
        self.cpu.sp = state.sp;
        self.cpu.ime = state.ime;
        self.cpu.running = state.running;
        self.cpu.cycles = 0;
        self.cpu.last_step_cycles = 0;
        self.cpu.timed_cycles_accounted = 0;
        self.cpu.last_step_master_ticks = 0;
        self.cpu.timed_master_ticks_accounted = 0;
        self.cpu.halt_bug_active = false;
    }
}

fn test_bus() -> Bus {
    let rom = vec![0; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header");
    Bus::new(rom, &header, HardwareMode::DMG).expect("test bus")
}

#[test]
fn common_adapter_observes_exact_load_store_timing() {
    let initial_state = GbState {
        registers: [0x5A, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D],
        pc: 0xC000,
        sp: 0xFFFE,
        ime: ImeState::Disabled,
        running: CpuState::Running,
    };
    let expected_state = GbState {
        pc: 0xC003,
        ..initial_state.clone()
    };
    let case = CpuCase {
        initial_state,
        expected_state,
        initial_memory: vec![MemoryBlock::new(0xC000, [0xEA, 0x00, 0xC1])],
        expected_memory: vec![MemoryBlock::new(0xC100, [0x5A])],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 16,
            master_ticks: MasterTicks::new(16),
            master_rate: ClockRate::from_hz(4_194_304),
            bus_events: vec![
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(4)),
                    space: TraceWriteKind::Memory,
                    addr: 0xC000,
                    value: 0xEA,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(8)),
                    space: TraceWriteKind::Memory,
                    addr: 0xC001,
                    value: 0x00,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(12)),
                    space: TraceWriteKind::Memory,
                    addr: 0xC002,
                    value: 0xC1,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(16)),
                    space: TraceWriteKind::Memory,
                    addr: 0xC100,
                    old_value: 0,
                    written_value: 0x5A,
                    new_value: 0x5A,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<GbAdapter>(&case);
}

#[test]
fn common_adapter_observes_interrupt_stack_order() {
    let initial_state = GbState {
        registers: [0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D],
        pc: 0xC123,
        sp: 0xFFFE,
        ime: ImeState::Enabled,
        running: CpuState::Running,
    };
    let expected_state = GbState {
        pc: 0x0040,
        sp: 0xFFFC,
        ime: ImeState::Disabled,
        ..initial_state.clone()
    };
    let case = CpuCase {
        initial_state,
        expected_state,
        initial_memory: vec![
            MemoryBlock::new(0xFFFF, [0x01]),
            MemoryBlock::new(0xFF0F, [0x01]),
        ],
        expected_memory: vec![MemoryBlock::new(0xFFFC, [0x23, 0xC1])],
        expected_step: StepObservation {
            kind: StepKind::Interrupt,
            cpu_cycles: 20,
            master_ticks: MasterTicks::new(20),
            master_rate: ClockRate::from_hz(4_194_304),
            bus_events: vec![
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(12)),
                    space: TraceWriteKind::Memory,
                    addr: 0xFFFD,
                    old_value: 0,
                    written_value: 0xC1,
                    new_value: 0xC1,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(16)),
                    space: TraceWriteKind::Memory,
                    addr: 0xFFFC,
                    old_value: 0,
                    written_value: 0x23,
                    new_value: 0x23,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<GbAdapter>(&case);
}
