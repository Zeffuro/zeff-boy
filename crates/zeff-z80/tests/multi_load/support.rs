use std::cell::{Cell, RefCell};
use zeff_z80::{BusState, Cpu, ResetState, Z80Bus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read(u16, u8),
    Write(u16, u8),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    pub events: [Option<Access>; 16],
    pub count: usize,
}

impl Trace {
    fn push(&mut self, access: Access) {
        self.events[self.count] = Some(access);
        self.count += 1;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bus {
    pub bytes: Vec<u8>,
    pub trace: RefCell<Trace>,
    pub irq: bool,
    pub nmi: bool,
    pub polls: Cell<u32>,
    pub acknowledgements: u32,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            bytes: vec![0; 65536],
            trace: RefCell::default(),
            irq: false,
            nmi: false,
            polls: Cell::new(0),
            acknowledgements: 0,
        }
    }

    pub fn clear_trace(&mut self) {
        *self.trace.get_mut() = Trace::default();
    }
}

impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        let value = self.bytes[usize::from(address)];
        self.trace.borrow_mut().push(Access::Read(address, value));
        value
    }

    fn cpu_write(&mut self, address: u16, value: u8) {
        self.trace.get_mut().push(Access::Write(address, value));
        self.bytes[usize::from(address)] = value;
    }

    fn io_read(&mut self, _: u8) -> u8 {
        0
    }
    fn io_write(&mut self, _: u8, _: u8) {}

    fn maskable_interrupt_pending(&self) -> bool {
        self.polls.set(self.polls.get() + 1);
        self.irq
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.polls.set(self.polls.get() + 1);
        self.nmi
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.acknowledgements += 1;
        std::mem::take(&mut self.nmi)
    }
}

pub fn seeded(value: u8) -> (Cpu, Bus) {
    let mut bus = Bus::new();
    let setup = [
        0x01, 0x12, 0x10, 0x11, 0x23, 0x20, 0x21, 0x56, 0x34, 0xdd, 0x21, 0x67, 0x45, 0xfd, 0x21,
        0x78, 0x56, 0x3e, value, 0x37,
    ];
    bus.bytes[0x100..0x100 + setup.len()].copy_from_slice(&setup);
    let mut cpu = Cpu::new_with_reset(ResetState::new(0x100, 0x9876));
    for _ in 0..7 {
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
    }
    (cpu, bus)
}

pub fn phase(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, clocks: u32, complete: bool) {
    let before = bus.trace.borrow().count;
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(step.instruction.cycles, clocks);
    assert_eq!(step.complete, complete);
    assert_eq!(state.pending(), !complete);
    assert_eq!(bus.trace.borrow().count, before + 1);
}
