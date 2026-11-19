use super::*;
use device::{Device, Event};

#[derive(Clone, Copy)]
struct Literal {
    family: u16,
    long: bool,
    source: u32,
    old: u32,
    result: u32,
    partial: u32,
    ccr: u16,
}

const LITERALS: [Literal; 8] = [
    Literal {
        family: 13,
        long: false,
        source: 1,
        old: 0xaabb_7fff,
        result: 0xaabb_8000,
        partial: 0xaabb_8000,
        ccr: 10,
    },
    Literal {
        family: 13,
        long: true,
        source: 1,
        old: 0x7fff_ffff,
        result: 0x8000_0000,
        partial: 0x7fff_0000,
        ccr: 10,
    },
    Literal {
        family: 9,
        long: false,
        source: 1,
        old: 0xaabb_8000,
        result: 0xaabb_7fff,
        partial: 0xaabb_7fff,
        ccr: 2,
    },
    Literal {
        family: 9,
        long: true,
        source: 1,
        old: 0x8000_0000,
        result: 0x7fff_ffff,
        partial: 0x8000_ffff,
        ccr: 2,
    },
    Literal {
        family: 12,
        long: false,
        source: 0,
        old: 0xaabb_ffff,
        result: 0xaabb_0000,
        partial: 0xaabb_0000,
        ccr: 20,
    },
    Literal {
        family: 12,
        long: true,
        source: 0xffff,
        old: 0x8000_0000,
        result: 0,
        partial: 0x8000_0000,
        ccr: 20,
    },
    Literal {
        family: 8,
        long: false,
        source: 0x8000,
        old: 0xaabb_0000,
        result: 0xaabb_8000,
        partial: 0xaabb_8000,
        ccr: 24,
    },
    Literal {
        family: 8,
        long: true,
        source: 0x8000_0000,
        old: 0,
        result: 0x8000_0000,
        partial: 0,
        ccr: 24,
    },
];

#[derive(Clone, Copy)]
enum Path {
    Completion,
    Host,
    Reserve,
}

fn frozen(cpu: &mut PrefetchCpu, bus: &mut Device) -> bool {
    let before = cpu.clone();
    let device = bus.clone();
    cpu.prime(bus) == Err(TimingFault::Frozen)
        && cpu.step_instruction(bus) == Err(TimingFault::Frozen)
        && cpu.step(bus) == Err(TimingFault::Frozen)
        && cpu.step_boundary(bus) == Err(TimingFault::Frozen)
        && cpu.service_pending_trace(bus) == Err(TimingFault::Frozen)
        && *cpu == before
        && *bus == device
}

fn check(literal: Literal, supervisor: bool, path: Path) -> bool {
    let opcode = (literal.family << 12) | (5 << 9) | if literal.long { 0x9f } else { 0x5f };
    let mut initial = Registers::new();
    initial.d = std::array::from_fn(|i| 0x1234_5600 | i as u32);
    initial.a = std::array::from_fn(|i| 0x2345_6000 | (i as u32 * 16));
    initial.usp = 0x3456_7000;
    initial.ssp = 0x8765_8000;
    initial.pc = PC;
    initial.d[5] = literal.old;
    initial.set_status_register(if supervisor { 0x201f } else { 31 });
    let address = if supervisor { initial.ssp } else { initial.usp };
    let source_count = if literal.long { 2 } else { 1 };
    let prefix = source_count as u64 * (4 + u64::from(WAIT));
    let start = if matches!(path, Path::Reserve) {
        u64::MAX - (4 + u64::from(u32::MAX)) + 1 - prefix
    } else {
        START
    };
    let mut memory = BTreeMap::new();
    put(&mut memory, PC, opcode);
    put(&mut memory, PC + 2, NEXT);
    put(&mut memory, PC + 4, FETCH);
    put(
        &mut memory,
        address,
        if literal.long {
            (literal.source >> 16) as u16
        } else {
            literal.source as u16
        },
    );
    if literal.long {
        put(&mut memory, address.wrapping_add(2), literal.source as u16);
    }
    let source = if literal.long {
        (u32::from(sample(&memory, address)) << 16)
            | u32::from(sample(&memory, address.wrapping_add(2)))
    } else {
        u32::from(sample(&memory, address))
    };
    let arithmetic = oracle::arithmetic(
        literal.family,
        literal.long,
        literal.old,
        source,
        initial.status_register(),
    );
    let input_good = address & 1 == 0
        && source == literal.source
        && arithmetic
            == (
                literal.result,
                literal.ccr | if supervisor { 0x2000 } else { 0 },
            )
        && ((literal.old & 0xffff_0000) | (literal.result & 0xffff)) == literal.partial;
    let failure = matches!(path, Path::Host).then_some(source_count);
    let mut bus = Device::new(memory.clone(), start, failure);
    let mut cpu = PrefetchCpu::from_prefetched(initial, [opcode, NEXT], CpuClocks(start)).unwrap();
    let mut expected = initial;
    if supervisor {
        expected.ssp = address.wrapping_add(if literal.long { 4 } else { 2 });
    } else {
        expected.usp = address.wrapping_add(if literal.long { 4 } else { 2 });
    }
    if !matches!(path, Path::Reserve) {
        expected.d[5] = if matches!(path, Path::Completion) {
            literal.result
        } else {
            literal.partial
        };
        expected.set_status_register(arithmetic.1);
    }
    let mut events = Vec::new();
    let mut clock = start;
    for at in 0..source_count {
        let a = address.wrapping_add(at as u32 * 2);
        events.push(Event::Read(
            oracle::access(a, supervisor, false),
            clock,
            sample(&memory, a),
        ));
        clock += 4 + u64::from(WAIT);
    }
    if !matches!(path, Path::Reserve) {
        events.push(Event::Read(
            oracle::access(PC + 4, supervisor, true),
            clock,
            FETCH,
        ));
        clock += 4 + u64::from(WAIT);
    }
    if matches!(path, Path::Completion) && literal.long {
        events.push(Event::Idle(clock, 2));
        clock += 2;
    }
    let result = cpu.step_instruction(&mut bus);
    let state = match path {
        Path::Completion => {
            expected.pc = PC + 2;
            result
                == Ok(TimedStep {
                    opcode,
                    clocks: clock - start,
                    exception: None,
                })
                && cpu.queued_words() == Some([NEXT, FETCH])
        }
        Path::Host => {
            result
                == Err(TimingFault::Bus {
                    access: oracle::access(PC + 4, supervisor, true),
                    direction: Direction::Read,
                    source: source_count,
                })
                && cpu.queued_words() == Some([opcode, NEXT])
                && frozen(&mut cpu, &mut bus)
        }
        Path::Reserve => {
            result == Err(TimingFault::ClockOverflow)
                && cpu.queued_words() == Some([opcode, NEXT])
                && frozen(&mut cpu, &mut bus)
        }
    };
    input_good
        && state
        && cpu.registers == expected
        && !cpu.trace_pending()
        && bus.count == source_count + usize::from(!matches!(path, Path::Reserve))
        && bus.valid
        && bus.words == memory
        && bus.events == events
        && bus.clock == clock
        && cpu.clock().0 == clock
}

pub(super) fn run() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for literal in LITERALS {
        for supervisor in [false, true] {
            for path in [Path::Completion, Path::Host, Path::Reserve] {
                count += 1;
                bad += u32::from(!check(literal, supervisor, path));
            }
        }
    }
    (count, bad + u32::from(count != 48))
}
