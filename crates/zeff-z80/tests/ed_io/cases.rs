#[path = "../multi_load/support.rs"]
#[allow(dead_code)]
mod support;

use support::Bus;
use zeff_z80::{BusState, Cpu, IoWriteCycle, PrefixState, Registers, Z80Bus};

const OPCODES: [u8; 16] = [
    0x40, 0x41, 0x48, 0x49, 0x50, 0x51, 0x58, 0x59, 0x60, 0x61, 0x68, 0x69, 0x70, 0x71, 0x78, 0x79,
];
const PREFIXES: [&[u8]; 3] = [&[], &[0xdd], &[0xfd]];
const INPUTS: [(u8, u8); 6] = [
    (0, 0x44),
    (1, 0),
    (3, 4),
    (0x28, 0x2c),
    (0x80, 0x80),
    (0xff, 0xac),
];

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

fn fixture(prefixes: &[u8], opcode: u8, port: u8, flags: u8, input: u8) -> (Cpu, IoBus) {
    let (mut cpu, mut memory) = support::seeded(0xa8);
    let program = [
        0x21, flags, 0xa8, 0xe5, 0xf1, 0x21, 0x56, 0x34, 0x0e, port, 0xc3, 0, 0x40,
    ];
    let setup = usize::from(cpu.regs().pc);
    memory.bytes[setup..setup + program.len()].copy_from_slice(&program);
    while cpu.regs().pc != 0x4000 {
        cpu.step_with_bus(&mut memory).unwrap();
        memory.clear_trace();
    }
    memory.bytes[0x4000..0x4000 + prefixes.len()].copy_from_slice(prefixes);
    memory.bytes[0x4000 + prefixes.len()..0x4002 + prefixes.len()].copy_from_slice(&[0xed, opcode]);
    (
        cpu,
        IoBus {
            memory,
            input,
            io: Vec::new(),
        },
    )
}

fn phase(cpu: &mut Cpu, bus: &mut IoBus, state: &mut BusState, complete: bool) {
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(step.instruction.cycles, 4);
    assert_eq!(step.complete, complete);
    assert_eq!(state.pending(), !complete);
}

fn first_acceptance() {
    let (mut cpu, mut bus) = fixture(&[], 0x48, 0x12, 0xff, 0xa8);
    let initial = cpu.regs();
    let memory = bus.memory.bytes.clone();
    let mut state = BusState::default();
    for _ in 0..2 {
        phase(&mut cpu, &mut bus, &mut state, false);
        assert_eq!((cpu.regs().c, cpu.regs().f), (0x12, 0xff));
        assert!(bus.io.is_empty());
    }
    let trace = bus.memory.trace.borrow().clone();
    let refresh = cpu.regs().r;
    phase(&mut cpu, &mut bus, &mut state, true);
    assert_eq!((cpu.regs().c, cpu.regs().f), (0xa8, 0xa9));
    assert_eq!(cpu.regs().pc, initial.pc + 2);
    assert_eq!(bus.io, [Io::Read(0x12, 0xa8)]);
    assert_eq!(*bus.memory.trace.borrow(), trace);
    assert_eq!(cpu.regs().r, refresh);
    assert_eq!(bus.memory.bytes, memory);
}

fn expected_registers(
    mut regs: Registers,
    prefixes: &[u8],
    opcode: u8,
    input: u8,
    flags: u8,
) -> Registers {
    if opcode & 1 == 0 {
        match (opcode >> 3) & 7 {
            0 => regs.b = input,
            1 => regs.c = input,
            2 => regs.d = input,
            3 => regs.e = input,
            4 => regs.h = input,
            5 => regs.l = input,
            6 => {}
            _ => regs.a = input,
        }
        regs.f = flags;
    }
    let fetched = prefixes.len() as u8 + 2;
    regs.pc += u16::from(fetched);
    regs.r = (regs.r & 0x80) | (regs.r.wrapping_add(fetched) & 0x7f);
    regs
}

fn expected_io(opcode: u8, port: u8, input: u8, offset: u32) -> Io {
    if opcode & 1 == 0 {
        Io::Read(port, input)
    } else {
        Io::Write(IoWriteCycle {
            port,
            value: [0x10, port, 0x20, 0x23, 0x34, 0x56, 0, 0xa8][usize::from((opcode >> 3) & 7)],
            t_states_before: offset,
            t_states: 4,
        })
    }
}

fn finish(cpu: &mut Cpu, bus: &mut IoBus, state: &mut BusState, prefixes: &[u8], from: usize) {
    for index in from..prefixes.len() + 3 {
        phase(cpu, bus, state, index == prefixes.len() + 2);
    }
}

fn literal_case(
    prefixes: &[u8],
    opcode: u8,
    port: u8,
    flags: u8,
    (input, result_flags): (u8, u8),
    bounded: bool,
) {
    let (mut cpu, mut bus) = fixture(prefixes, opcode, port, flags, input);
    let initial = cpu.regs();
    let expected = expected_registers(initial, prefixes, opcode, input, result_flags);
    let mut aggregate = (cpu.clone(), bus.clone());
    let total = aggregate.0.step_with_bus(&mut aggregate.1).unwrap().cycles;
    assert_eq!(total, 12 + 4 * prefixes.len() as u32);
    assert_eq!(aggregate.0.regs(), expected);
    assert_eq!(
        aggregate.1.io,
        [expected_io(
            opcode,
            port,
            input,
            8 + 4 * prefixes.len() as u32
        )]
    );
    if bounded {
        let mut pair = (cpu.clone(), bus.clone());
        let mut state = PrefixState::default();
        let mut clocks = 0;
        loop {
            let step = pair.0.step_bounded(&mut pair.1, &mut state).unwrap();
            clocks += step.instruction.cycles;
            if step.complete {
                break;
            }
        }
        assert_eq!(clocks, total);
        assert_eq!(pair.1.io, [expected_io(opcode, port, input, 8)]);
        pair.1.io = aggregate.1.io.clone();
        assert_eq!(pair, aggregate);
    }
    let start = cpu.cycles();
    let memory = bus.memory.bytes.clone();
    let mut state = BusState::default();
    for index in 0..prefixes.len() + 3 {
        let complete = index == prefixes.len() + 2;
        let trace = bus.memory.trace.borrow().clone();
        let refresh = cpu.regs().r;
        phase(&mut cpu, &mut bus, &mut state, complete);
        if complete {
            assert_eq!(*bus.memory.trace.borrow(), trace);
            assert_eq!(cpu.regs().r, refresh);
        } else {
            assert!(bus.io.is_empty());
            assert_eq!(bus.memory.trace.borrow().count, trace.count + 1);
            assert_eq!(
                bus.memory.trace.borrow().events[trace.count],
                Some(support::Access::Read(
                    initial.pc + index as u16,
                    memory[usize::from(initial.pc) + index]
                ))
            );
            let mut pending = initial;
            pending.pc += index as u16 + 1;
            pending.r = (initial.r & 0x80) | (initial.r.wrapping_add(index as u8 + 1) & 0x7f);
            assert_eq!(cpu.regs(), pending);
        }
    }
    assert_eq!(cpu.regs(), expected);
    assert_eq!(cpu.cycles() - start, u64::from(total));
    assert_eq!(bus.io, [expected_io(opcode, port, input, 0)]);
    assert_eq!(bus.memory.bytes, memory);
    bus.io = aggregate.1.io.clone();
    assert_eq!((cpu, bus), aggregate);
}

pub fn all_ed_io_forms_match_literal_effects_and_steppers() {
    first_acceptance();
    let mut forms = 0;
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            forms += 1;
            for carry in [0, 1] {
                for (input, flags) in INPUTS {
                    literal_case(
                        prefixes,
                        opcode,
                        0x12,
                        0xfe | carry,
                        (input, flags | carry),
                        true,
                    );
                }
            }
        }
    }
    assert_eq!(forms, 48);
    for port in 0..=255 {
        for opcode in [0x48, 0x49] {
            literal_case(&[], opcode, port, 0xff, (0xa8, 0xa9), false);
        }
    }
}

pub fn ed_io_live_terminal_and_port_reads() {
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            let terminal = 0x4001 + prefixes.len();
            let (mut cpu, mut bus) = fixture(prefixes, 0, 0x12, 0xff, 0);
            let initial = cpu.regs();
            let mut state = BusState::default();
            for _ in 0..prefixes.len() + 1 {
                phase(&mut cpu, &mut bus, &mut state, false);
            }
            bus.memory.bytes[terminal] = opcode;
            phase(&mut cpu, &mut bus, &mut state, false);
            assert!(bus.io.is_empty());
            bus.memory.bytes[terminal] = 0;
            bus.input = 0xa8;
            phase(&mut cpu, &mut bus, &mut state, true);
            assert_eq!(
                cpu.regs(),
                expected_registers(initial, prefixes, opcode, 0xa8, 0xa9)
            );
            assert_eq!(bus.io, [expected_io(opcode, 0x12, 0xa8, 0)]);

            let (mut cpu, mut bus) = fixture(prefixes, opcode, 0x12, 0xff, 0xa8);
            let initial = cpu.regs();
            let mut state = BusState::default();
            for _ in 0..prefixes.len() + 1 {
                phase(&mut cpu, &mut bus, &mut state, false);
            }
            bus.memory.bytes[terminal] = 0;
            phase(&mut cpu, &mut bus, &mut state, true);
            assert!(bus.io.is_empty());
            assert_eq!(
                cpu.regs(),
                expected_registers(initial, prefixes, 0x41, 0, 0)
            );
        }
    }
}

pub fn ed_io_pending_clone_suspend_reset() {
    for prefixes in PREFIXES.into_iter().chain([&[0xdd, 0xfd, 0xdd, 0xfd][..]]) {
        for opcode in OPCODES {
            for count in 1..prefixes.len() + 3 {
                let (mut cpu, mut bus) = fixture(prefixes, opcode, 0x12, 0xff, 0);
                let mut state = BusState::default();
                for _ in 0..count {
                    phase(&mut cpu, &mut bus, &mut state, false);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut cloned = (cpu.clone(), bus.clone(), state);
                bus.input = 0xa8;
                cloned.1.input = 0xa8;
                finish(&mut cpu, &mut bus, &mut state, prefixes, count);
                finish(&mut cloned.0, &mut cloned.1, &mut cloned.2, prefixes, count);
                assert_eq!((cpu, bus, state), cloned);
                assert_eq!(cloned.1.io, [expected_io(opcode, 0x12, 0xa8, 0)]);

                let (mut cpu, mut bus, _) = saved;
                cpu.reset();
                bus.memory.clear_trace();
                bus.memory.bytes[0x100] = 0;
                let memory = bus.memory.bytes.clone();
                phase(&mut cpu, &mut bus, &mut BusState::default(), true);
                assert!(bus.io.is_empty());
                assert_eq!(bus.memory.bytes, memory);
                assert_eq!((cpu.regs().pc, cpu.regs().sp), (0x101, 0x9876));
            }
        }
    }
}

pub fn ed_io_interrupt_boundaries() {
    for prefixes in PREFIXES {
        for opcode in OPCODES {
            for count in 1..prefixes.len() + 3 {
                for (iff1, iff2) in [(false, false), (false, true), (true, false), (true, true)] {
                    for nmi in [false, true] {
                        let (mut cpu, mut bus) = fixture(prefixes, opcode, 0x12, 0xff, 0xa8);
                        cpu.finish_guest_call(iff1, iff2, 0);
                        cpu.resume();
                        let mut state = BusState::default();
                        for _ in 0..count {
                            phase(&mut cpu, &mut bus, &mut state, false);
                        }
                        let polls = bus.memory.polls.get();
                        bus.memory.nmi = nmi;
                        bus.memory.irq = !nmi;
                        finish(&mut cpu, &mut bus, &mut state, prefixes, count);
                        assert_eq!(bus.io, [expected_io(opcode, 0x12, 0xa8, 0)]);
                        assert_eq!(
                            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
                            (iff1, iff2)
                        );
                        assert_eq!(bus.memory.polls.get(), polls);
                        bus.memory.clear_trace();
                        cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                        assert_eq!(cpu.last_step_was_interrupt(), nmi || iff1);
                        assert_eq!(bus.io.len(), 1);
                    }
                }
            }
            let (mut cpu, mut bus) = fixture(prefixes, opcode, 0x12, 0xff, 0xa8);
            bus.memory.bytes.copy_within(0x4000..0x4008, 0x4001);
            bus.memory.bytes[0x4000] = 0xfb;
            cpu.step_with_bus(&mut bus).unwrap();
            bus.memory.clear_trace();
            bus.memory.irq = true;
            let mut state = BusState::default();
            for index in 0..prefixes.len() + 3 {
                let complete = index == prefixes.len() + 2;
                phase(&mut cpu, &mut bus, &mut state, complete);
                assert_eq!(cpu.interrupts_enabled(), complete);
                assert!(!cpu.last_step_was_interrupt());
            }
            assert_eq!(bus.io, [expected_io(opcode, 0x12, 0xa8, 0)]);
            bus.memory.clear_trace();
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert!(cpu.last_step_was_interrupt());
            assert_eq!(bus.io.len(), 1);
        }
    }
}

pub fn ed_io_repeated_prefixes_and_callback_offsets() {
    let prefixes = [0xdd, 0xfd, 0xdd, 0xfd];
    for opcode in OPCODES {
        literal_case(&prefixes, opcode, 0xff, 0xff, (0xa8, 0xa9), true);
        for count in 1..=prefixes.len() {
            let (mut cpu, mut bus) = fixture(&prefixes, opcode, 0x12, 0xff, 0xa8);
            let mut state = BusState::default();
            for _ in 0..count {
                phase(&mut cpu, &mut bus, &mut state, false);
            }
            assert!(state.prefix_pending());
            let mut cloned = (cpu.clone(), bus.clone(), state);
            finish(&mut cpu, &mut bus, &mut state, &prefixes, count);
            finish(
                &mut cloned.0,
                &mut cloned.1,
                &mut cloned.2,
                &prefixes,
                count,
            );
            assert_eq!((cpu, bus, state), cloned);
        }
    }
}
