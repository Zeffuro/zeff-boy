use super::*;
use crate::hardware::cartridge::Cartridge;
use zeff_emu_common::cpu::CpuCore;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct NesState {
    pc: u16,
    sp: u8,
    a: u8,
    x: u8,
    y: u8,
    status: u8,
    running: CpuState,
    nmi_pending: bool,
    irq_line: bool,
}

struct NesAdapter {
    cpu: Cpu,
    bus: Bus,
}

impl CpuConformanceAdapter for NesAdapter {
    type State = NesState;

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
                    .expect("NES test address must fit in 16 bits");
                adapter.bus.cpu_write(address, value);
            }
        }
        adapter
    }

    fn snapshot(&self) -> Self::State {
        NesState {
            pc: self.cpu.pc,
            sp: self.cpu.sp,
            a: self.cpu.regs.a,
            x: self.cpu.regs.x,
            y: self.cpu.regs.y,
            status: self.cpu.regs.p.bits(),
            running: self.cpu.state,
            nmi_pending: self.cpu.nmi_pending,
            irq_line: self.cpu.irq_line,
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus
            .cpu_peek(u16::try_from(address).expect("NES test address must fit in 16 bits"))
    }

    fn step(&mut self) -> StepObservation {
        self.bus.debug_trace_enabled = true;
        self.bus.debug_trace_reads = true;
        self.bus.debug_trace_events.clear();
        self.bus.begin_cpu_step_timing(MasterTicks::ZERO);

        let cpu_cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);
        let _ = self.bus.finish_cpu_step_timing(cpu_cycles);
        let bus_events = std::mem::take(&mut self.bus.debug_trace_events);
        self.bus.debug_trace_enabled = false;

        StepObservation {
            kind: match self.cpu.last_step_kind {
                CpuStepKind::Instruction => StepKind::Instruction,
                CpuStepKind::Nmi | CpuStepKind::Irq => StepKind::Interrupt,
                CpuStepKind::Idle => StepKind::Idle,
            },
            cpu_cycles,
            master_ticks: MasterTicks::new(cpu_cycles),
            master_rate: ClockRate::from_ratio(21_477_272, 12),
            bus_events,
        }
    }
}

impl NesAdapter {
    fn apply_state(&mut self, state: &NesState) {
        self.cpu.pc = state.pc;
        self.cpu.sp = state.sp;
        self.cpu.regs.a = state.a;
        self.cpu.regs.x = state.x;
        self.cpu.regs.y = state.y;
        self.cpu.regs.p = StatusFlags::from_bits_truncate(state.status);
        self.cpu.state = state.running;
        self.cpu.cycles = 0;
        self.cpu.last_step_cycles = 0;
        self.cpu.nmi_pending = state.nmi_pending;
        self.cpu.irq_line = state.irq_line;
    }
}

fn test_bus() -> Bus {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg + 0x3FFA] = 0x00;
    rom[prg + 0x3FFB] = 0x90;
    let cartridge = Cartridge::load(&rom).expect("test cartridge");
    Bus::new(cartridge, 44_100.0)
}

#[test]
fn common_adapter_observes_one_access_per_cpu_cycle() {
    let initial_state = NesState {
        pc: 0x0000,
        sp: 0xFD,
        a: 0x7E,
        x: 0,
        y: 0,
        status: 0x24,
        running: CpuState::Running,
        nmi_pending: false,
        irq_line: false,
    };
    let expected_state = NesState {
        pc: 0x0003,
        ..initial_state.clone()
    };
    let case = CpuCase {
        initial_state,
        expected_state,
        initial_memory: vec![MemoryBlock::new(0x0000, [0x8D, 0x00, 0x03])],
        expected_memory: vec![MemoryBlock::new(0x0300, [0x7E])],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 4,
            master_ticks: MasterTicks::new(4),
            master_rate: ClockRate::from_ratio(21_477_272, 12),
            bus_events: vec![
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(0)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0000,
                    value: 0x8D,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(1)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0001,
                    value: 0x00,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(2)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0002,
                    value: 0x03,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(3)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0300,
                    old_value: 0xFF,
                    written_value: 0x7E,
                    new_value: 0x7E,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<NesAdapter>(&case);
}

#[test]
fn common_adapter_observes_nmi_entry_order() {
    let initial_state = NesState {
        pc: 0x0200,
        sp: 0xFD,
        a: 0,
        x: 0,
        y: 0,
        status: 0x24,
        running: CpuState::Running,
        nmi_pending: true,
        irq_line: false,
    };
    let expected_state = NesState {
        pc: 0x9000,
        sp: 0xFA,
        nmi_pending: false,
        ..initial_state.clone()
    };
    let case = CpuCase {
        initial_state,
        expected_state,
        initial_memory: Vec::new(),
        expected_memory: vec![MemoryBlock::new(0x01FB, [0x24, 0x00, 0x02])],
        expected_step: StepObservation {
            kind: StepKind::Interrupt,
            cpu_cycles: 7,
            master_ticks: MasterTicks::new(7),
            master_rate: ClockRate::from_ratio(21_477_272, 12),
            bus_events: vec![
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(0)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0200,
                    value: 0xFF,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(1)),
                    space: TraceWriteKind::Memory,
                    addr: 0x0200,
                    value: 0xFF,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(2)),
                    space: TraceWriteKind::Memory,
                    addr: 0x01FD,
                    old_value: 0xFF,
                    written_value: 0x02,
                    new_value: 0x02,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(3)),
                    space: TraceWriteKind::Memory,
                    addr: 0x01FC,
                    old_value: 0xFF,
                    written_value: 0x00,
                    new_value: 0x00,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: Some(MasterTicks::new(4)),
                    space: TraceWriteKind::Memory,
                    addr: 0x01FB,
                    old_value: 0xFF,
                    written_value: 0x24,
                    new_value: 0x24,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(5)),
                    space: TraceWriteKind::Memory,
                    addr: 0xFFFA,
                    value: 0x00,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: Some(MasterTicks::new(6)),
                    space: TraceWriteKind::Memory,
                    addr: 0xFFFB,
                    value: 0x90,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
            ],
        },
    };

    assert_case::<NesAdapter>(&case);
}

mod conformance_vectors;
