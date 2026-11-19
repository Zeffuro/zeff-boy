use std::cell::RefCell;
use zeff_z80::{
    BoundedStep, BusState, Cpu, IoWriteCycle, MemptrProfile, PrefixState, ResetState, Z80Bus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Read(u16, u8),
    Write(u16, u8),
    IoRead(u8, u8),
    IoWrite(u8, u8),
    IoCycle(IoWriteCycle),
    Irq(u8),
    Nmi,
    InterruptRead(u16, u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bus {
    pub bytes: Vec<u8>,
    pub events: RefCell<Vec<Event>>,
    pub irq: bool,
    pub nmi: bool,
    pub ack_opcode: u8,
    pub interrupt_bytes: Vec<u8>,
    interrupt_cursor: usize,
}

impl Bus {
    fn new(program: &[u8], pc: u16) -> Self {
        let mut result = Self {
            bytes: vec![0; 65536],
            events: RefCell::default(),
            irq: false,
            nmi: false,
            ack_opcode: 0xff,
            interrupt_bytes: vec![0x34, 0x12],
            interrupt_cursor: 0,
        };
        result.install(program, pc);
        result
    }

    fn install(&mut self, program: &[u8], pc: u16) {
        for (index, value) in program.iter().enumerate() {
            self.bytes[usize::from(pc.wrapping_add(index as u16))] = *value;
        }
    }
}

impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        let value = self.bytes[usize::from(address)];
        self.events.borrow_mut().push(Event::Read(address, value));
        value
    }

    fn cpu_write(&mut self, address: u16, value: u8) {
        self.events.get_mut().push(Event::Write(address, value));
        self.bytes[usize::from(address)] = value;
    }

    fn io_read(&mut self, port: u8) -> u8 {
        let value = port ^ 0xa5;
        self.events.get_mut().push(Event::IoRead(port, value));
        value
    }

    fn io_write(&mut self, port: u8, value: u8) {
        self.events.get_mut().push(Event::IoWrite(port, value));
    }

    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.events.get_mut().push(Event::IoCycle(cycle));
        self.io_write(cycle.port, cycle.value);
    }

    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi
    }

    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.events.get_mut().push(Event::Irq(self.ack_opcode));
        self.ack_opcode
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.events.get_mut().push(Event::Nmi);
        std::mem::take(&mut self.nmi)
    }

    fn read_interrupt_instruction(&mut self, address: u16) -> u8 {
        let value = self.interrupt_bytes[self.interrupt_cursor];
        self.interrupt_cursor += 1;
        self.events
            .get_mut()
            .push(Event::InterruptRead(address, value));
        value
    }
}

pub struct Observed {
    pub bus: Bus,
    pub instruction: RefCell<Vec<(u16, u8)>>,
}

impl Z80Bus for Observed {
    fn cpu_read(&self, address: u16) -> u8 {
        self.bus.cpu_read(address)
    }

    fn cpu_read_instruction(&self, address: u16) -> u8 {
        let value = self.bus.cpu_read_instruction(address);
        self.instruction.borrow_mut().push((address, value));
        value
    }

    fn cpu_write(&mut self, address: u16, value: u8) {
        self.bus.cpu_write(address, value);
    }

    fn io_read(&mut self, port: u8) -> u8 {
        self.bus.io_read(port)
    }

    fn io_write(&mut self, port: u8, value: u8) {
        self.bus.io_write(port, value);
    }

    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.bus.io_write_cycle(cycle);
    }

    fn maskable_interrupt_pending(&self) -> bool {
        self.bus.maskable_interrupt_pending()
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.bus.non_maskable_interrupt_pending()
    }

    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.bus.acknowledge_maskable_interrupt()
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.bus.acknowledge_non_maskable_interrupt()
    }

    fn read_interrupt_instruction(&mut self, address: u16) -> u8 {
        self.bus.read_interrupt_instruction(address)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Api {
    Whole,
    Bounded,
    Segmented,
}

pub const APIS: [Api; 3] = [Api::Whole, Api::Bounded, Api::Segmented];
pub const PROFILES: [MemptrProfile; 2] = [MemptrProfile::Legacy, MemptrProfile::Nmos];

pub struct Pair {
    pub cpu: Cpu,
    reference: Cpu,
    pub observed: Observed,
    plain: Bus,
    api: Api,
    prefix: PrefixState,
    reference_prefix: PrefixState,
    segment: BusState,
    reference_segment: BusState,
}

impl Pair {
    pub fn new(program: &[u8], pc: u16, api: Api, profile: MemptrProfile) -> Self {
        let bus = Bus::new(program, pc);
        let cpu = Cpu::new_with_memptr_profile(ResetState::new(pc, 0x9000), profile);
        Self {
            reference: cpu.clone(),
            cpu,
            plain: bus.clone(),
            observed: Observed {
                bus,
                instruction: RefCell::default(),
            },
            api,
            prefix: PrefixState::default(),
            reference_prefix: PrefixState::default(),
            segment: BusState::default(),
            reference_segment: BusState::default(),
        }
    }

    pub fn buses(&mut self, edit: impl Fn(&mut Bus)) {
        edit(&mut self.observed.bus);
        edit(&mut self.plain);
    }

    pub fn install(&mut self, program: &[u8]) -> u16 {
        let pc = self.cpu.regs().pc;
        self.buses(|bus| bus.install(program, pc));
        self.clear();
        pc
    }

    pub fn clear(&mut self) {
        self.buses(|bus| bus.events.borrow_mut().clear());
        self.observed.instruction.get_mut().clear();
    }

    pub fn cycle(&mut self) -> Option<BoundedStep> {
        let actual = step(
            &mut self.cpu,
            &mut self.observed,
            self.api,
            &mut self.prefix,
            &mut self.segment,
        );
        let expected = step(
            &mut self.reference,
            &mut self.plain,
            self.api,
            &mut self.reference_prefix,
            &mut self.reference_segment,
        );
        assert_eq!(actual, expected);
        self.same();
        actual
    }

    pub fn finish(&mut self) {
        for _ in 0..256 {
            if self.cycle().unwrap().complete {
                return;
            }
        }
        panic!("finite instruction did not complete");
    }

    pub fn same(&self) {
        assert_eq!(self.cpu, self.reference);
        assert_eq!(self.observed.bus, self.plain);
        assert_eq!(self.prefix, self.reference_prefix);
        assert_eq!(self.segment, self.reference_segment);
    }

    pub fn bytes(&self, pc: u16, values: &[u8]) {
        let expected: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(index, value)| (pc.wrapping_add(index as u16), *value))
            .collect();
        assert_eq!(*self.observed.instruction.borrow(), expected);
    }

    pub fn waits(&mut self, step: &mut BoundedStep, waits: u32, accepted: bool) {
        let before = self.cpu.clone();
        let original = *step;
        let bytes = self.observed.instruction.borrow().clone();
        let bus = self.observed.bus.clone();
        let mut expected = *step;
        assert_eq!(
            self.cpu.extend_bus_segment(&mut step.instruction, waits),
            accepted
        );
        assert_eq!(
            self.reference
                .extend_bus_segment(&mut expected.instruction, waits),
            accepted
        );
        assert_eq!(*step, expected);
        self.same();
        assert_eq!(*self.observed.instruction.borrow(), bytes);
        assert_eq!(self.observed.bus, bus);
        if !accepted {
            assert_eq!(self.cpu, before);
            assert_eq!(*step, original);
        }
    }

    pub fn suspend(&mut self) {
        self.cpu.suspend();
        self.reference.suspend();
    }

    pub fn resume(&mut self) {
        self.cpu.resume();
        self.reference.resume();
    }
}

fn step<B: Z80Bus>(
    cpu: &mut Cpu,
    bus: &mut B,
    api: Api,
    prefix: &mut PrefixState,
    segment: &mut BusState,
) -> Option<BoundedStep> {
    match api {
        Api::Whole => cpu.step_with_bus(bus).map(|instruction| BoundedStep {
            instruction,
            complete: true,
        }),
        Api::Bounded => cpu.step_bounded(bus, prefix),
        Api::Segmented => cpu.step_bus_segment(bus, segment),
    }
}

pub fn seeded(api: Api, profile: MemptrProfile) -> Pair {
    let mut pair = Pair::new(
        &[
            0x31, 0, 0x90, 0x01, 0, 0x40, 0x11, 0, 0x41, 0x21, 0, 0x42, 0xdd, 0x21, 0, 0x43, 0xfd,
            0x21, 0, 0x44, 0x3e, 0x5a,
        ],
        0x1000,
        api,
        profile,
    );
    for _ in 0..7 {
        pair.finish();
    }
    pair.buses(|bus| {
        for address in [0x4000, 0x4100, 0x4200, 0x4300, 0x4400] {
            bus.bytes[address..address + 2].copy_from_slice(&[0x5a, 0xa5]);
        }
        bus.bytes[0x9000..0x9002].copy_from_slice(&[0x34, 0x12]);
    });
    pair.clear();
    pair
}
