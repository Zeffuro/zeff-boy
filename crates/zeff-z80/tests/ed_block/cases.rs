#[path = "../multi_load/support.rs"]
#[allow(dead_code)]
mod support;

#[path = "flags.rs"]
mod flags;

use support::{Access, Bus};
use zeff_z80::{BusState, Cpu, IoWriteCycle, PrefixState, Registers, Z80Bus};

const OPCODES: [u8; 16] = [
    0xa0, 0xa8, 0xb0, 0xb8, 0xa1, 0xa9, 0xb1, 0xb9, 0xa2, 0xaa, 0xb2, 0xba, 0xa3, 0xab, 0xb3, 0xbb,
];
const PREFIXES: [&[u8]; 3] = [&[], &[0xdd], &[0xfd]];

#[derive(Clone, Copy)]
struct Case {
    opcode: u8,
    af: [u8; 2],
    pairs: [u16; 3],
    value: u8,
    flags: u8,
    repeat: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Io {
    Read(u8, u8),
    Write(IoWriteCycle),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IoBus {
    memory: Bus,
    input: u8,
    io: Vec<Io>,
}

impl Z80Bus for IoBus {
    fn cpu_read(&self, address: u16) -> u8 {
        self.memory.cpu_read(address)
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.memory.cpu_write(address, value);
    }
    fn io_read(&mut self, port: u8) -> u8 {
        self.io.push(Io::Read(port, self.input));
        self.input
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.io.push(Io::Write(IoWriteCycle {
            port,
            value,
            t_states_before: 0,
            t_states: 0,
        }));
    }
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.io.push(Io::Write(cycle));
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.memory.maskable_interrupt_pending()
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.memory.non_maskable_interrupt_pending()
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.memory.acknowledge_non_maskable_interrupt()
    }
}

fn fixture(prefixes: &[u8], opcode: u8, af: [u8; 2], pairs: [u16; 3], value: u8) -> (Cpu, IoBus) {
    let (mut cpu, mut memory) = support::seeded(0x12);
    let mut program = vec![0x21, af[1], af[0], 0xe5, 0xf1];
    for (opcode, value) in [
        (0x01, pairs[0]),
        (0x11, pairs[1]),
        (0x21, pairs[2]),
        (0xc3, 0x4000),
    ] {
        program.push(opcode);
        program.extend(value.to_le_bytes());
    }
    let setup = usize::from(cpu.regs().pc);
    memory.bytes[setup..setup + program.len()].copy_from_slice(&program);
    while cpu.regs().pc != 0x4000 {
        cpu.step_with_bus(&mut memory).unwrap();
        memory.clear_trace();
    }
    memory.bytes[0x4000..0x4000 + prefixes.len()].copy_from_slice(prefixes);
    memory.bytes[0x4000 + prefixes.len()..0x4002 + prefixes.len()].copy_from_slice(&[0xed, opcode]);
    memory.bytes[usize::from(pairs[2])] = value;
    (
        cpu,
        IoBus {
            memory,
            input: value,
            io: Vec::new(),
        },
    )
}

fn phase(cpu: &mut Cpu, bus: &mut IoBus, state: &mut BusState, clocks: u32, complete: bool) {
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(step.instruction.cycles, clocks);
    assert_eq!(step.complete, complete);
    assert_eq!(state.pending(), !complete);
}

fn first_acceptance() {
    let (mut cpu, mut bus) = fixture(&[], 0xa0, [0x12, 1], [2, 0x0200, 0x0100], 0x22);
    let start = cpu.cycles();
    let mut state = BusState::default();
    phase(&mut cpu, &mut bus, &mut state, 4, false);
    phase(&mut cpu, &mut bus, &mut state, 4, false);
    let decoded = cpu.regs();
    phase(&mut cpu, &mut bus, &mut state, 3, false);
    assert_eq!(cpu.regs(), decoded);
    assert_eq!(bus.memory.bytes[0x0200], 0);
    phase(&mut cpu, &mut bus, &mut state, 3, false);
    assert_eq!(cpu.cycles() - start, 14);
    assert_eq!(bus.memory.bytes[0x0200], 0x22);
    assert_eq!(cpu.regs(), decoded);
    let trace = bus.memory.trace.borrow().clone();
    phase(&mut cpu, &mut bus, &mut state, 2, true);
    assert_eq!(cpu.cycles() - start, 16);
    assert_eq!(
        (
            cpu.regs().hl(),
            cpu.regs().de(),
            cpu.regs().bc(),
            cpu.regs().f
        ),
        (0x0101, 0x0201, 1, 5)
    );
    assert_eq!(*bus.memory.trace.borrow(), trace);
    assert!(bus.io.is_empty());
}

fn standard(opcode: u8, count: u16) -> Case {
    let kind = opcode & 3;
    let down = opcode & 8 != 0;
    let flags = match kind {
        0 => {
            if count == 1 {
                1
            } else {
                5
            }
        }
        1 => {
            if count == 1 {
                0x83
            } else {
                0x87
            }
        }
        2 => {
            if count == 1 {
                0x44
            } else if count == 0 {
                0xac
            } else {
                0
            }
        }
        _ => match (down, count) {
            (true, 1) => 0x51,
            (true, 0) => 0xb9,
            (true, _) => 0x15,
            (false, 1) => 0x44,
            (false, 0) => 0xac,
            (false, _) => 0,
        },
    };
    let continuing = opcode & 0x10 != 0 && count != 1;
    let repeat = continuing.then_some(match kind {
        0 => 5,
        1 => 0x87,
        2 => {
            if count == 0 {
                0x80
            } else {
                4
            }
        }
        _ => {
            if down {
                if count == 0 { 0x91 } else { 1 }
            } else if count == 0 {
                0x80
            } else {
                4
            }
        }
    });
    Case {
        opcode,
        af: [0x12, 1],
        pairs: [
            if kind < 2 { count } else { (count << 8) | 0x12 },
            0x0200,
            0x0100,
        ],
        value: 0x22,
        flags,
        repeat,
    }
}

fn clocks(prefixes: &[u8], case: Case) -> Vec<u32> {
    let mut phases = vec![4; prefixes.len()];
    phases.extend_from_slice(match case.opcode & 3 {
        0 => &[4, 4, 3, 3, 2],
        1 => &[4, 4, 3, 5],
        2 => &[4, 4, 1, 4, 3],
        _ => &[4, 4, 1, 3, 4],
    });
    if case.repeat.is_some() {
        phases.push(5);
    }
    phases
}

fn expected(initial: Registers, case: Case, ed: u16, base: bool) -> Registers {
    let mut regs = initial;
    let delta = if case.opcode & 8 == 0 { 1 } else { -1 };
    [regs.h, regs.l] = initial.hl().wrapping_add_signed(delta).to_be_bytes();
    if case.opcode & 3 < 2 {
        [regs.b, regs.c] = initial.bc().wrapping_sub(1).to_be_bytes();
    } else {
        regs.b = initial.b.wrapping_sub(1);
    }
    if case.opcode & 3 == 0 {
        [regs.d, regs.e] = initial.de().wrapping_add_signed(delta).to_be_bytes();
    }
    regs.pc = if !base && case.repeat.is_some() {
        ed
    } else {
        ed.wrapping_add(2)
    };
    regs.f = if base {
        case.flags
    } else {
        case.repeat.unwrap_or(case.flags)
    };
    regs
}

fn expected_io(case: Case, offset: u32) -> Vec<Io> {
    match case.opcode & 3 {
        2 => vec![Io::Read(case.pairs[0] as u8, case.value)],
        3 => vec![Io::Write(IoWriteCycle {
            port: case.pairs[0] as u8,
            value: case.value,
            t_states_before: offset,
            t_states: 4,
        })],
        _ => Vec::new(),
    }
}

fn literal_case(prefixes: &[u8], case: Case, start: u16) {
    let (mut cpu, mut bus) = fixture(prefixes, case.opcode, case.af, case.pairs, case.value);
    if start != 0x4000 {
        let program = bus.memory.bytes[0x4000..0x4002 + prefixes.len()].to_vec();
        for (index, value) in program.into_iter().enumerate() {
            bus.memory.bytes[usize::from(start.wrapping_add(index as u16))] = value;
        }
        bus.memory.bytes[0x4000..0x4003].copy_from_slice(&[0xc3, start as u8, (start >> 8) as u8]);
        cpu.step_with_bus(&mut bus).unwrap();
        bus.memory.clear_trace();
    }
    let initial = cpu.regs();
    let ed = start.wrapping_add(prefixes.len() as u16);
    let mut fetched = initial;
    fetched.pc = ed.wrapping_add(2);
    fetched.r = (initial.r & 0x80) | (initial.r.wrapping_add(prefixes.len() as u8 + 2) & 0x7f);
    let final_regs = expected(fetched, case, ed, false);
    let mut aggregate = (cpu.clone(), bus.clone());
    let phases = clocks(prefixes, case);
    let total = phases.iter().sum::<u32>();
    assert_eq!(
        aggregate.0.step_with_bus(&mut aggregate.1).unwrap().cycles,
        total
    );
    assert_eq!(aggregate.0.regs(), final_regs);
    assert_eq!(
        aggregate.1.io,
        expected_io(case, 12 + 4 * prefixes.len() as u32)
    );
    let mut bounded = (cpu.clone(), bus.clone());
    let mut prefix_state = PrefixState::default();
    let mut consumed = 0;
    loop {
        let step = bounded
            .0
            .step_bounded(&mut bounded.1, &mut prefix_state)
            .unwrap();
        consumed += step.instruction.cycles;
        if step.complete {
            break;
        }
    }
    assert_eq!(consumed, total);
    assert_eq!(bounded.1.io, expected_io(case, 12));
    bounded.1.io = aggregate.1.io.clone();
    assert_eq!(bounded, aggregate);
    let mut state = BusState::default();
    let cycles = cpu.cycles();
    let base_index = phases.len() - 1 - usize::from(case.repeat.is_some());
    let mut accesses = Vec::new();
    for (index, &clock) in phases.iter().enumerate() {
        let before = (bus.memory.trace.borrow().clone(), bus.io.clone());
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            clock,
            index + 1 == phases.len(),
        );
        if index < prefixes.len() + 2 {
            let address = start.wrapping_add(index as u16);
            accesses.push(Some(Access::Read(
                address,
                bus.memory.bytes[usize::from(address)],
            )));
            let mut pending = initial;
            pending.pc = address.wrapping_add(1);
            pending.r = (initial.r & 0x80) | (initial.r.wrapping_add(index as u8 + 1) & 0x7f);
            assert_eq!(cpu.regs(), pending);
        } else {
            let stage = index - prefixes.len() - 2;
            let access = match (case.opcode & 3, stage) {
                (0 | 1, 0) | (3, 1) => Some(Access::Read(initial.hl(), case.value)),
                (0, 1) => Some(Access::Write(initial.de(), case.value)),
                (2, 2) => Some(Access::Write(initial.hl(), case.value)),
                _ => None,
            };
            if let Some(access) = access {
                accesses.push(Some(access));
            }
            if access.is_none() {
                assert_eq!(*bus.memory.trace.borrow(), before.0);
            }
            let io_phase = matches!((case.opcode & 3, stage), (2, 1) | (3, 2));
            if !io_phase {
                assert_eq!(bus.io, before.1);
            }
            assert_eq!(
                cpu.regs(),
                if index < base_index {
                    fetched
                } else {
                    expected(fetched, case, ed, index == base_index)
                }
            );
        }
        assert_eq!(
            &bus.memory.trace.borrow().events[..accesses.len()],
            accesses
        );
    }
    assert_eq!(cpu.cycles() - cycles, u64::from(total));
    assert_eq!(cpu.regs(), final_regs);
    assert_eq!(bus.io, expected_io(case, 0));
    bus.io = aggregate.1.io.clone();
    assert_eq!((cpu, bus), aggregate);
}

fn finish(cpu: &mut Cpu, bus: &mut IoBus, state: &mut BusState, phases: &[u32], from: usize) {
    for (index, &clock) in phases.iter().enumerate().skip(from) {
        phase(cpu, bus, state, clock, index + 1 == phases.len());
    }
}

pub fn all_ed_block_forms_match_literal_effects_and_steppers() {
    first_acceptance();
    let mut forms = 0;
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            forms += 1;
            for count in [2, 1, 0] {
                literal_case(prefixes, standard(opcode, count), 0x4000);
            }
            if opcode & 3 == 1 {
                let mut case = standard(opcode, 2);
                case.af[0] = 0x22;
                case.flags = 0x47;
                case.repeat = None;
                literal_case(prefixes, case, 0x4000);
            }
        }
    }
    assert_eq!(forms, 48);
    flags::base_rows();
}

pub fn ed_block_live_operands_and_latches() {
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            let case = standard(opcode, 2);
            let (mut cpu, mut bus) = fixture(prefixes, 0, case.af, case.pairs, 0);
            let mut state = BusState::default();
            for _ in 0..prefixes.len() + 1 {
                phase(&mut cpu, &mut bus, &mut state, 4, false);
            }
            let terminal = 0x4001 + prefixes.len();
            bus.memory.bytes[terminal] = opcode;
            phase(&mut cpu, &mut bus, &mut state, 4, false);
            bus.memory.bytes[terminal] = 0;
            let phases = clocks(prefixes, case);
            let read = prefixes.len() + 2 + usize::from(opcode & 2 != 0);
            for &clock in &phases[prefixes.len() + 2..read] {
                phase(&mut cpu, &mut bus, &mut state, clock, false);
            }
            bus.memory.bytes[0x0100] = 0x22;
            bus.input = 0x22;
            phase(&mut cpu, &mut bus, &mut state, phases[read], false);
            bus.memory.bytes[0x0100] = 0x99;
            bus.input = 0x99;
            finish(&mut cpu, &mut bus, &mut state, &phases, read + 1);
            assert_eq!(cpu.regs().f, case.repeat.unwrap_or(case.flags));
            assert_eq!(bus.io, expected_io(case, 0));
            if opcode & 3 == 0 {
                assert_eq!(bus.memory.bytes[0x0200], 0x22);
            }
            if opcode & 3 == 2 {
                assert_eq!(bus.memory.bytes[0x0100], 0x22);
            }
            let (mut cpu, mut bus) = fixture(prefixes, opcode, case.af, case.pairs, 0x22);
            let initial = cpu.regs();
            let mut state = BusState::default();
            for _ in 0..prefixes.len() + 1 {
                phase(&mut cpu, &mut bus, &mut state, 4, false);
            }
            bus.memory.bytes[terminal] = 0;
            phase(&mut cpu, &mut bus, &mut state, 4, true);
            assert_eq!(
                (
                    cpu.regs().hl(),
                    cpu.regs().de(),
                    cpu.regs().bc(),
                    cpu.regs().f
                ),
                (initial.hl(), initial.de(), initial.bc(), initial.f)
            );
            assert!(bus.io.is_empty());
        }
    }
}

pub fn ed_block_pending_clone_suspend_reset() {
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            let case = standard(opcode, 2);
            let phases = clocks(prefixes, case);
            for count in 1..phases.len() {
                let (mut cpu, mut bus) = fixture(prefixes, opcode, case.af, case.pairs, case.value);
                let mut state = BusState::default();
                for &clock in &phases[..count] {
                    phase(&mut cpu, &mut bus, &mut state, clock, false);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut cloned = (cpu.clone(), bus.clone(), state);
                finish(&mut cpu, &mut bus, &mut state, &phases, count);
                finish(&mut cloned.0, &mut cloned.1, &mut cloned.2, &phases, count);
                assert_eq!((cpu, bus, state), cloned);
                let (mut cpu, mut bus, _) = saved;
                cpu.reset();
                bus.memory.clear_trace();
                bus.memory.bytes[0x100] = 0;
                let memory = bus.memory.bytes.clone();
                let io = bus.io.clone();
                phase(&mut cpu, &mut bus, &mut BusState::default(), 4, true);
                assert_eq!(bus.memory.bytes, memory);
                assert_eq!(bus.io, io);
                assert_eq!((cpu.regs().pc, cpu.regs().sp), (0x101, 0x9876));
            }
        }
    }
}

pub fn ed_block_interrupt_boundaries() {
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            let case = standard(opcode, 2);
            let phases = clocks(prefixes, case);
            for count in 1..phases.len() {
                for (iff1, iff2) in [(false, false), (false, true), (true, false), (true, true)] {
                    for nmi in [false, true] {
                        let (mut cpu, mut bus) =
                            fixture(prefixes, opcode, case.af, case.pairs, case.value);
                        cpu.finish_guest_call(iff1, iff2, 0);
                        cpu.resume();
                        let mut state = BusState::default();
                        for &clock in &phases[..count] {
                            phase(&mut cpu, &mut bus, &mut state, clock, false);
                        }
                        let polls = bus.memory.polls.get();
                        bus.memory.nmi = nmi;
                        bus.memory.irq = !nmi;
                        finish(&mut cpu, &mut bus, &mut state, &phases, count);
                        assert_eq!(
                            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
                            (iff1, iff2)
                        );
                        assert_eq!(bus.memory.polls.get(), polls);
                        assert_eq!(bus.memory.acknowledgements, 0);
                        let io = bus.io.clone();
                        bus.memory.clear_trace();
                        cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                        assert_eq!(cpu.last_step_was_interrupt(), nmi || iff1);
                        assert_eq!(bus.io, io);
                    }
                }
            }
            let (mut cpu, mut bus) = fixture(prefixes, opcode, case.af, case.pairs, case.value);
            bus.memory.bytes.copy_within(0x4000..0x4008, 0x4001);
            bus.memory.bytes[0x4000] = 0xfb;
            cpu.step_with_bus(&mut bus).unwrap();
            bus.memory.clear_trace();
            bus.memory.irq = true;
            let mut state = BusState::default();
            for (index, &clock) in phases.iter().enumerate() {
                let complete = index + 1 == phases.len();
                phase(&mut cpu, &mut bus, &mut state, clock, complete);
                assert_eq!(cpu.interrupts_enabled(), complete);
                assert!(!cpu.last_step_was_interrupt());
            }
            bus.memory.clear_trace();
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert!(cpu.last_step_was_interrupt());
        }
    }
}

pub fn ed_block_repeat_flags_and_callback_offsets() {
    for opcode in OPCODES {
        literal_case(&[0xdd, 0xfd, 0xdd, 0xfd], standard(opcode, 2), 0x4000);
        for address in [0, 0xffff] {
            let mut case = standard(opcode, 2);
            case.pairs[1] = address;
            case.pairs[2] = address;
            if opcode & 3 == 3 {
                let down = opcode & 8 != 0;
                let (base, repeat) = match (address, down) {
                    (0, false) => (0, 4),
                    (0, true) => (0x15, 1),
                    (_, false) => (4, 0),
                    (_, true) => (0x11, 5),
                };
                (case.flags, case.repeat) = (base, (opcode & 0x10 != 0).then_some(repeat));
            }
            literal_case(&[], case, 0x4000);
        }
    }
    flags::repeat_rows();
}
