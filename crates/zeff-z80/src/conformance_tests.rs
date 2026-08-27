use std::cell::RefCell;

use super::*;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_test_support::cpu::{
    CpuCase, CpuConformanceAdapter, MemoryBlock, StepKind, StepObservation, TraceTiming,
    assert_case,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Z80State {
    registers: Registers,
    shadow_registers: [u8; 8],
    state: CpuState,
    interrupt_mode: InterruptMode,
    interrupts_enabled: bool,
    saved_interrupts_enabled: bool,
    enable_interrupts_delay: u8,
}

struct TraceBus {
    memory: Box<[u8]>,
    events: RefCell<Vec<BusAccessEvent>>,
}

impl TraceBus {
    fn new() -> Self {
        Self {
            memory: vec![0; 0x1_0000].into_boxed_slice(),
            events: RefCell::new(Vec::new()),
        }
    }

    fn begin_cpu_access_trace(&self) {
        self.events.borrow_mut().clear();
    }

    fn drain_cpu_access_trace(&self) -> Vec<BusAccessEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    fn cpu_peek(&self, addr: u16) -> u8 {
        self.memory[usize::from(addr)]
    }
}

impl Z80Bus for TraceBus {
    fn cpu_read(&self, addr: u16) -> u8 {
        let value = self.memory[usize::from(addr)];
        self.events.borrow_mut().push(BusAccessEvent::Read {
            at: None,
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        let old_value = self.memory[usize::from(addr)];
        self.memory[usize::from(addr)] = value;
        self.events.get_mut().push(BusAccessEvent::Write {
            at: None,
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            old_value: u32::from(old_value),
            written_value: u32::from(value),
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
    }

    fn io_read(&mut self, _port: u8) -> u8 {
        0xFF
    }

    fn io_write(&mut self, _port: u8, _value: u8) {}

    fn maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        false
    }
}

struct Z80Adapter {
    cpu: Cpu,
    bus: TraceBus,
}

impl CpuConformanceAdapter for Z80Adapter {
    type State = Z80State;

    const TRACE_TIMING: TraceTiming = TraceTiming::OrderOnly;

    fn from_case(case: &CpuCase<Self::State>) -> Self {
        let mut adapter = Self {
            cpu: Cpu::new(),
            bus: TraceBus::new(),
        };
        adapter.apply_state(&case.initial_state);
        for block in &case.initial_memory {
            for (offset, &value) in block.bytes.iter().enumerate() {
                let offset = u32::try_from(offset).expect("test memory block offset");
                let address = u16::try_from(block.start.wrapping_add(offset))
                    .expect("Z80 test address must fit in 16 bits");
                adapter.bus.cpu_write(address, value);
            }
        }
        adapter
    }

    fn snapshot(&self) -> Self::State {
        Z80State {
            registers: self.cpu.regs,
            shadow_registers: [
                self.cpu.shadow.a,
                self.cpu.shadow.f,
                self.cpu.shadow.b,
                self.cpu.shadow.c,
                self.cpu.shadow.d,
                self.cpu.shadow.e,
                self.cpu.shadow.h,
                self.cpu.shadow.l,
            ],
            state: self.cpu.state,
            interrupt_mode: self.cpu.interrupt_mode,
            interrupts_enabled: self.cpu.interrupt_flip_flop_1,
            saved_interrupts_enabled: self.cpu.interrupt_flip_flop_2,
            enable_interrupts_delay: self.cpu.enable_interrupts_delay,
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus
            .cpu_peek(u16::try_from(address).expect("Z80 test address must fit in 16 bits"))
    }

    fn step(&mut self) -> StepObservation {
        let cycles_before = self.cpu.cycles;
        self.bus.begin_cpu_access_trace();
        let was_halted = self.cpu.state == CpuState::Halted;
        let fetched = self.cpu.step(&mut self.bus);
        let bus_events = self.bus.drain_cpu_access_trace();
        let cpu_cycles = self.cpu.cycles - cycles_before;

        StepObservation {
            kind: if self.cpu.last_step_was_interrupt {
                StepKind::Interrupt
            } else if self.cpu.trap.is_some() {
                StepKind::Trap
            } else if was_halted || fetched.is_none() {
                StepKind::Idle
            } else {
                StepKind::Instruction
            },
            cpu_cycles,
            master_ticks: MasterTicks::new(cpu_cycles),
            master_rate: ClockRate::from_hz(3_584_160),
            bus_events,
        }
    }
}

impl Z80Adapter {
    fn apply_state(&mut self, state: &Z80State) {
        self.cpu.regs = state.registers;
        let [a, f, b, c, d, e, h, l] = state.shadow_registers;
        self.cpu.shadow = ShadowRegisters {
            a,
            f,
            b,
            c,
            d,
            e,
            h,
            l,
        };
        self.cpu.state = state.state;
        self.cpu.interrupt_mode = state.interrupt_mode;
        self.cpu.interrupt_flip_flop_1 = state.interrupts_enabled;
        self.cpu.interrupt_flip_flop_2 = state.saved_interrupts_enabled;
        self.cpu.enable_interrupts_delay = state.enable_interrupts_delay;
        self.cpu.cycles = 0;
        self.cpu.last_opcode_pc = state.registers.pc;
        self.cpu.last_opcode = 0;
        self.cpu.last_step_was_interrupt = false;
        self.cpu.instruction_bytes = [0; 4];
        self.cpu.instruction_byte_count = 0;
        self.cpu.trap = None;
    }
}

#[test]
fn common_adapter_observes_load_store_bus_order() {
    let initial_state = Z80State {
        registers: Registers {
            a: 0x5A,
            sp: 0xD000,
            pc: 0xC000,
            ..Registers::default()
        },
        shadow_registers: [0; 8],
        state: CpuState::Running,
        interrupt_mode: InterruptMode::Im1,
        interrupts_enabled: false,
        saved_interrupts_enabled: false,
        enable_interrupts_delay: 0,
    };
    let expected_state = Z80State {
        registers: Registers {
            pc: 0xC003,
            r: 1,
            ..initial_state.registers
        },
        ..initial_state.clone()
    };
    let case = CpuCase {
        initial_state,
        expected_state,
        initial_memory: vec![MemoryBlock::new(0xC000, [0x32, 0x00, 0xC1])],
        expected_memory: vec![MemoryBlock::new(0xC100, [0x5A])],
        expected_step: StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 13,
            master_ticks: MasterTicks::new(13),
            master_rate: ClockRate::from_hz(3_584_160),
            bus_events: vec![
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0xC000,
                    value: 0x32,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0xC001,
                    value: 0x00,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: 0xC002,
                    value: 0xC1,
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                },
                BusAccessEvent::Write {
                    at: None,
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

    assert_case::<Z80Adapter>(&case);
}
