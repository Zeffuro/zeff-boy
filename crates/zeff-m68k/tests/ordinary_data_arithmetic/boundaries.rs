use super::*;
use device::Event;
use fixture::{Case, OPS};

fn reserve(c: Case, at: usize) -> bool {
    let prefix = c.idle() + at as u64 * 4;
    let c = Case {
        start: u64::MAX - (4 + u64::from(u32::MAX)) + 1 - prefix,
        ..c
    };
    let (mut cpu, mut bus, p) = run::seed(c);
    let memory = bus.words.clone();
    let expected = if at + 1 == c.count() {
        p.before_result
    } else {
        p.phases[at].registers
    };
    let good = cpu.step_instruction(&mut bus) == Err(TimingFault::ClockOverflow)
        && cpu.registers == expected
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && bus.count == at
        && bus.valid
        && bus.words == memory
        && bus.events == run::events(c, &p, &bus.waits, at, false)
        && cpu.clock().0 == c.start + prefix
        && bus.clock == cpu.clock().0;
    good && run::frozen(&mut cpu, &mut bus)
}
fn source_idle(c: Case, room: u64) -> bool {
    let c = Case {
        start: u64::MAX - room,
        ..c
    };
    let (mut cpu, mut bus, p) = run::seed(c);
    let memory = bus.words.clone();
    let expected = if room == 2 && c.ea == 4 {
        p.phases[0].registers
    } else {
        c.initial()
    };
    let good = cpu.step_instruction(&mut bus) == Err(TimingFault::ClockOverflow)
        && cpu.registers == expected
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && bus.count == 0
        && bus.valid
        && bus.words == memory
        && bus.events
            == if room == 2 {
                vec![Event::Idle(c.start, 2)]
            } else {
                vec![]
            }
        && cpu.clock().0 == c.start + if room == 2 { 2 } else { 0 }
        && bus.clock == cpu.clock().0;
    good && run::frozen(&mut cpu, &mut bus)
}
fn tail(c: Case, room: u64) -> bool {
    let prefix = c.idle() + (c.count() as u64 - 1) * 4;
    let c = Case {
        start: u64::MAX - (4 + u64::from(u32::MAX)) - prefix,
        ..c
    };
    let (mut cpu, mut bus, p) = run::seed(c);
    bus.waits[c.count() - 1] = u32::MAX - room as u32;
    let memory = bus.words.clone();
    let mut partial_pc = p.full;
    partial_pc.pc = c.pc;
    let result = cpu.step_instruction(&mut bus);
    let good = if room < c.tail() {
        result == Err(TimingFault::ClockOverflow)
            && cpu.registers == partial_pc
            && cpu.queued_words() == Some([c.opcode(), c.retained()])
            && run::frozen(&mut cpu, &mut bus)
    } else {
        result
            == Ok(TimedStep {
                opcode: c.opcode(),
                clocks: u64::MAX - c.start,
                exception: None,
            })
            && cpu.registers == p.full
            && cpu.queued_words() == Some(p.queue)
    };
    good && bus.valid
        && bus.words == memory
        && bus.count == c.count()
        && bus.events == run::events(c, &p, &bus.waits, c.count(), room == c.tail())
        && cpu.clock().0 == u64::MAX - room + if room == c.tail() { c.tail() } else { 0 }
        && bus.clock == cpu.clock().0
}
fn maximum(c: Case, at: usize, host: bool) -> bool {
    let prefix = c.idle() + at as u64 * 4;
    let c = Case {
        start: u64::MAX - (4 + u64::from(u32::MAX)) - prefix,
        failure: host.then_some(at),
        ..c
    };
    let (mut cpu, mut bus, p) = run::seed(c);
    bus.waits[at] = u32::MAX;
    let memory = bus.words.clone();
    let last = at + 1 == c.count();
    let result = cpu.step_instruction(&mut bus);
    let mut expected = if host {
        p.phases[at].registers
    } else if last {
        p.full
    } else if at + 2 == c.count() {
        p.before_result
    } else {
        p.phases[at + 1].registers
    };
    let good = if host {
        result
            == Err(TimingFault::Bus {
                access: p.phases[at].access,
                direction: Direction::Read,
                source: at,
            })
            && cpu.queued_words() == Some([c.opcode(), c.retained()])
            && run::frozen(&mut cpu, &mut bus)
    } else if last && !c.long {
        result
            == Ok(TimedStep {
                opcode: c.opcode(),
                clocks: u64::MAX - c.start,
                exception: None,
            })
            && cpu.queued_words() == Some(p.queue)
    } else {
        expected.pc = c.pc;
        result == Err(TimingFault::ClockOverflow)
            && cpu.queued_words() == Some([c.opcode(), c.retained()])
            && run::frozen(&mut cpu, &mut bus)
    };
    good && cpu.registers == expected
        && bus.valid
        && bus.words == memory
        && bus.count == at + 1
        && bus.events == run::events(c, &p, &bus.waits, at + 1, false)
        && cpu.clock().0 == u64::MAX
        && bus.clock == u64::MAX
}
fn odd(c: Case) -> bool {
    let c = directed::target(Case { pc: 0x5000, ..c }, 0x6001);
    let c = Case { source: 7, ..c };
    let (mut cpu, mut bus, p) = run::seed(c);
    let first = p.phases[c.extensions()];
    let memory = bus.words.clone();
    let good = first.access.logical_address == 0x6001
        && cpu.step_instruction(&mut bus) == Err(TimingFault::AddressError { address: 0x6001 })
        && cpu.registers == first.registers
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && bus.count == c.extensions()
        && bus.valid
        && bus.words == memory
        && bus.events == run::events(c, &p, &bus.waits, c.extensions(), false)
        && cpu.clock().0 == c.start + c.idle() + c.extensions() as u64 * 7
        && bus.clock == cpu.clock().0;
    good && run::frozen(&mut cpu, &mut bus)
}
pub(super) fn capacity() -> (u32, u32) {
    sweep(0, 472)
}
pub(super) fn source_capacity() -> (u32, u32) {
    sweep(1, 96)
}
pub(super) fn tails() -> (u32, u32) {
    sweep(2, 288)
}
pub(super) fn consumed_maximum() -> (u32, u32) {
    sweep(3, 944)
}
pub(super) fn address_errors() -> (u32, u32) {
    sweep(4, 144)
}
fn sweep(section: u8, expected: u32) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for supervisor in [false, true] {
                    let c = Case {
                        operation,
                        long,
                        ea,
                        supervisor,
                        ..Case::default()
                    };
                    let results: Vec<bool> = match section {
                        0 => (0..c.count()).map(|at| reserve(c, at)).collect(),
                        1 if c.idle() != 0 => [1, 2].map(|room| source_idle(c, room)).to_vec(),
                        1 => vec![],
                        2 => (0..=c.tail()).map(|room| tail(c, room)).collect(),
                        3 => (0..c.count())
                            .flat_map(|at| [false, true].map(|host| maximum(c, at, host)))
                            .collect(),
                        4 => vec![odd(Case { wait: 3, ..c })],
                        _ => unreachable!(),
                    };
                    count += results.len() as u32;
                    bad += results
                        .into_iter()
                        .map(|good| u32::from(!good))
                        .sum::<u32>();
                }
            }
        }
    }
    (count, bad + u32::from(count != expected))
}
