use std::cell::RefCell;
use zeff_z80::{BusState, Cpu, PrefixState, ResetState, Z80Bus};

const COMMON: [u8; 19] = [
    0x02, 0x0a, 0x12, 0x1a, 0x06, 0x0e, 0x16, 0x1e, 0x26, 0x2e, 0x3e, 0xc6, 0xce, 0xd6, 0xde, 0xe6,
    0xee, 0xf6, 0xfe,
];
const MEMORY: [u8; 22] = [
    0x46, 0x4e, 0x56, 0x5e, 0x66, 0x6e, 0x7e, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x77, 0x86, 0x8e,
    0x96, 0x9e, 0xa6, 0xae, 0xb6, 0xbe,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u16, u8),
    Write(u16, u8),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Trace {
    events: [Option<Access>; 16],
    count: usize,
}

impl Trace {
    fn push(&mut self, access: Access) {
        self.events[self.count] = Some(access);
        self.count += 1;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bus {
    bytes: Vec<u8>,
    trace: RefCell<Trace>,
    irq: bool,
    nmi: bool,
    acknowledgements: u32,
}

impl Bus {
    fn new() -> Self {
        Self {
            bytes: vec![0; 65536],
            trace: RefCell::default(),
            irq: false,
            nmi: false,
            acknowledgements: 0,
        }
    }

    fn clear_trace(&mut self) {
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
        0x55
    }

    fn io_write(&mut self, _: u8, _: u8) {}

    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.acknowledgements += 1;
        std::mem::take(&mut self.nmi)
    }
}

fn seeded(value: u8) -> (Cpu, Bus) {
    let mut bus = Bus::new();
    let setup = [
        0x01, 0, 0x10, 0x11, 0, 0x20, 0x21, 0, 0x30, 0xdd, 0x21, 0, 0x40, 0xfd, 0x21, 0, 0x50,
        0x3e, value, 0x37,
    ];
    bus.bytes[..setup.len()].copy_from_slice(&setup);
    let mut cpu = Cpu::new();
    for _ in 0..7 {
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
    }
    for address in [0x1000, 0x2000, 0x3000, 0x4000, 0x5000] {
        bus.bytes[address] = value;
    }
    (cpu, bus)
}

pub fn complete_single_transfer_family_matches_legacy() {
    let mut count = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in COMMON.into_iter().chain(MEMORY) {
            if prefix.is_some() && MEMORY.contains(&opcode) {
                continue;
            }
            count += 1;
            for value in [0, 1, 0x7f, 0x80, 0xff] {
                let (mut cpu, mut bus) = seeded(value);
                let pc = cpu.regs().pc;
                let start = usize::from(pc);
                let mut cursor = start;
                if let Some(prefix) = prefix {
                    bus.bytes[cursor] = prefix;
                    cursor += 1;
                }
                bus.bytes[cursor] = opcode;
                bus.bytes[cursor + 1] = value;
                let mut reference = cpu.clone();
                let mut reference_bus = bus.clone();
                let expected = reference.step_with_bus(&mut reference_bus).unwrap();
                let mut state = BusState::default();
                let mut clocks = 0;
                let m1_count = 1 + usize::from(prefix.is_some());
                for fetched in 1..=m1_count {
                    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                    assert!(!step.complete);
                    assert_eq!(step.instruction.cycles, 4);
                    clocks += 4;
                    assert_eq!(bus.trace.borrow().count, fetched);
                    assert_eq!(cpu.regs().pc, pc + fetched as u16);
                    assert!(state.pending());
                    assert_eq!(state.prefix_pending(), prefix.is_some());
                }
                let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                assert!(step.complete);
                assert_eq!(step.instruction.cycles, 3);
                assert_eq!(step.instruction.pc, expected.pc);
                assert_eq!(step.instruction.opcode, expected.opcode);
                assert_eq!(clocks + step.instruction.cycles, expected.cycles);
                assert_eq!(bus.trace.borrow().count, m1_count + 1);
                assert!(!state.pending());
                assert_eq!(cpu, reference, "opcode {opcode:02x}, prefix {prefix:?}");
                assert_eq!(bus, reference_bus);
            }
        }
    }
    assert_eq!(count, 79);
}

pub fn latched_opcode_and_live_transfer_are_distinct() {
    for (opcode, address, expected) in
        [(0x3e, 21, 0xa5), (0x7e, 0x3000, 0xa5), (0x77, 0x3000, 0x81)]
    {
        let (mut cpu, mut bus) = seeded(0x81);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc] = opcode;
        let mut state = BusState::default();
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        assert_eq!(cpu.regs().a, 0x81);
        assert_eq!(bus.trace.borrow().count, 1);
        bus.bytes[pc] = 0x76;
        bus.bytes[address] = 0xa5;
        let mut copied = cpu.clone();
        let mut copied_state = state;
        let mut copied_bus = bus.clone();
        let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert_eq!(
            step,
            copied
                .step_bus_segment(&mut copied_bus, &mut copied_state)
                .unwrap()
        );
        assert_eq!(cpu, copied);
        assert_eq!(state, copied_state);
        assert_eq!(bus, copied_bus);
        assert_eq!(bus.trace.borrow().count, 2);
        if opcode == 0x77 {
            assert_eq!(bus.bytes[address], expected);
        } else {
            assert_eq!(cpu.regs().a, expected);
        }
        assert!(!cpu.is_halted());
    }
}

pub fn interrupts_suspend_clone_and_reset_preserve_boundaries() {
    for nmi in [false, true] {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new();
        bus.bytes[..4].copy_from_slice(&[0xfb, 0x3e, 0x55, 0]);
        let mut state = BusState::default();
        assert!(cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        if !nmi {
            bus.irq = true;
        }
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        bus.nmi = nmi;
        assert!(!cpu.interrupts_enabled());
        cpu.suspend();
        let saved = (cpu.clone(), state, bus.clone());
        assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
        assert_eq!((cpu.clone(), state, bus.clone()), saved);
        cpu.resume();
        assert!(cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        assert!(cpu.interrupts_enabled());
        assert_eq!(cpu.regs().a, 0x55);
        assert_eq!(bus.acknowledgements, 0);
        for (index, clocks) in [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate() {
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, clocks);
            assert_eq!(step.complete, index == 2);
        }
        assert!(cpu.last_step_was_interrupt());
        assert_eq!(cpu.regs().pc, if nmi { 0x66 } else { 0x38 });
        assert_eq!(bus.acknowledgements, u32::from(nmi));
    }
    let mut cpu = Cpu::new();
    let mut bus = Bus::new();
    bus.bytes[0] = 0x3e;
    let mut state = BusState::default();
    assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    cpu.reset();
    state = BusState::default();
    assert_eq!(cpu, Cpu::new());
    assert!(!state.pending());
}

pub fn wrapping_fetch_and_long_prefixes_resume_once() {
    let mut bus = Bus::new();
    bus.bytes[0xffff] = 0x3e;
    bus.bytes[0] = 0x55;
    let mut cpu = Cpu::new_with_reset(ResetState::new(0xffff, 0));
    let mut state = BusState::default();
    assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    assert_eq!(cpu.regs().pc, 0);
    assert!(cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    assert_eq!(cpu.regs().pc, 1);
    assert_eq!(cpu.regs().a, 0x55);
    assert_eq!(cpu.regs().r, 1);
    bus.bytes.fill(0xdd);
    cpu.reset();
    for _ in 0..65537 {
        bus.clear_trace();
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        assert_eq!(bus.trace.borrow().count, 1);
    }
    assert_eq!(cpu.regs().pc, 0);
    bus.bytes[0] = 0x26;
    bus.bytes[1] = 0x75;
    bus.clear_trace();
    assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    assert!(state.prefix_pending());
    let refresh = cpu.regs().r;
    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
    assert!(step.complete);
    assert_eq!(step.instruction.cycles, 3);
    assert_eq!(cpu.regs().ix, 0x7500);
    assert_eq!(cpu.regs().r, refresh);
    assert_eq!(cpu.last_m1_fetch_count(), 255);
    assert_eq!(cpu.cycles(), 65538 * 4 + 3);
    assert_eq!(bus.trace.borrow().count, 2);
}

pub fn aggregate_fallback_and_old_bounded_api_stay_exact() {
    for program in [
        &[0x34][..],
        &[0x20, 2],
        &[0x09],
        &[0xcd, 0x34, 0x12],
        &[0xcb, 0x06],
        &[0xed, 0xa0],
        &[0xdd, 0x7e, 0],
        &[0xfd, 0x77, 0],
        &[0xdd, 0xd3, 0x7f],
        &[0xfd, 0xed, 0xb0],
        &[0x76],
        &[0x00],
    ] {
        let (mut cpu, mut bus) = seeded(0x81);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + program.len()].copy_from_slice(program);
        let mut reference = cpu.clone();
        let mut reference_bus = bus.clone();
        let mut prefix = PrefixState::default();
        let mut state = BusState::default();
        if matches!(program[0], 0x34 | 0xcb | 0xcd | 0xed)
            || program == [0xdd, 0x7e, 0]
            || program == [0xfd, 0x77, 0]
            || program == [0xfd, 0xed, 0xb0]
        {
            loop {
                if reference
                    .step_bounded(&mut reference_bus, &mut prefix)
                    .unwrap()
                    .complete
                {
                    break;
                }
            }
            loop {
                if cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete {
                    break;
                }
            }
            assert_eq!(cpu, reference);
            assert_eq!(bus, reference_bus);
            continue;
        }
        loop {
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            let expected = reference
                .step_bounded(&mut reference_bus, &mut prefix)
                .unwrap();
            assert_eq!(step, expected);
            assert_eq!(cpu, reference);
            assert_eq!(bus, reference_bus);
            assert_eq!(state.pending(), prefix.pending());
            if step.complete {
                break;
            }
        }
    }
    for opcode in COMMON.into_iter().chain(MEMORY) {
        let (mut cpu, mut bus) = seeded(0x81);
        bus.bytes[usize::from(cpu.regs().pc)] = opcode;
        let mut prefix = PrefixState::default();
        let step = cpu.step_bounded(&mut bus, &mut prefix).unwrap();
        assert!(step.complete);
        assert_eq!(step.instruction.cycles, 7);
        assert!(!prefix.pending());
    }
}
