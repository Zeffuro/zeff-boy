use super::*;
use device::Event;

fn reserve(c: Case, at: usize) -> bool {
    let prefix = c.idle() + at as u64 * 4;
    let c = Case {
        start: u64::MAX - (4 + u64::from(u32::MAX)) + 1 - prefix,
        ..c
    };
    let (mut cpu, mut bus, p) = run::seed(c);
    let memory = bus.words.clone();
    let expected = if at + 1 == c.count() {
        p.phases[at - 1].registers
    } else {
        p.phases[at].registers
    };
    let good = cpu.step_instruction(&mut bus) == Err(TimingFault::ClockOverflow)
        && cpu.registers == expected
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && bus.count == at
        && bus.valid
        && bus.words == memory
        && bus.events == run::events(c, &p, &bus.waits, at, 0)
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
    let expected = if room == 2 && c.family == 4 {
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
    let mut expected = p.phases.last().unwrap().registers;
    if c.long || room >= 2 {
        expected.set_address_register(
            c.destination,
            p.full.address_register(c.destination).unwrap(),
        );
    }
    let idles = if room == c.tail() {
        c.tail() / 2
    } else if !c.long && room >= 2 {
        1
    } else {
        0
    };
    let result = cpu.step_instruction(&mut bus);
    let good = if room < c.tail() {
        result == Err(TimingFault::ClockOverflow)
            && cpu.registers == expected
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
        && bus.events == run::events(c, &p, &bus.waits, c.count(), idles)
        && cpu.clock().0 == u64::MAX - room + idles * 2
        && bus.clock == cpu.clock().0
}
pub(super) fn goldens() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for supervisor in [false, true] {
                let c = Case {
                    subtract,
                    long,
                    supervisor,
                    family: 3,
                    source: 7,
                    destination: 7,
                    value: 2,
                    desired: if subtract { 0x20000 } else { 0x1fffe },
                    ..Case::default()
                };
                for room in 0..=c.tail() {
                    count += 1;
                    bad += u32::from(!tail(c, room));
                }
            }
        }
    }
    (count, bad + u32::from(count != 32))
}
pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for family in 2..=10 {
                for supervisor in [false, true] {
                    let c = Case {
                        subtract,
                        long,
                        family,
                        supervisor,
                        source: 7,
                        destination: 7,
                        value: 2,
                        desired: if subtract { 0x20000 } else { 0x1fffe },
                        ..Case::default()
                    };
                    for at in 0..c.count() {
                        count += 1;
                        bad += u32::from(!reserve(c, at));
                    }
                    if c.idle() != 0 {
                        for room in [1, 2] {
                            count += 1;
                            bad += u32::from(!source_idle(c, room));
                        }
                    }
                    for room in 0..=c.tail() {
                        count += 1;
                        bad += u32::from(!tail(c, room));
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 572))
}
