use super::*;

fn case(kind: u8, supervisor: bool) -> Case {
    let mut c = Case::new(Form {
        size: if kind == 0 { Size::Word } else { Size::Long },
        ea: match kind {
            0 => Ea::Pre,
            1 => Ea::Post,
            _ => Ea::AbsoluteLong,
        },
    });
    c.r = registers(supervisor, 0x14);
    c.r.pc = 0x1000;
    c.r.usp = if kind == 0 { 0x3002 } else { 0x3000 };
    c.r.ssp = if supervisor { c.r.usp } else { 0x4000 };
    c.register = 7;
    c.start = 0;
    c.immediate = if kind == 0 { 0x8000 } else { 1 };
    c.value = if kind == 0 { 0x7fff } else { 0x8000_0000 };
    c.extension = 0xabcd;
    c.low = 0x6000;
    c.next = 0x4e71;
    c.last = 0x4afc;
    c
}
fn literals(c: Case, kind: u8) -> Vec<oracle::Transfer> {
    use InstructionPhase::{
        ExtensionFirst as E1, ExtensionSecond as E2, ImmediateFirst as I1, ImmediateSecond as I2,
        OperandReadHigh as H, OperandReadLow as L, OperandRefill as F,
    };
    let rows: Vec<(u32, u32, InstructionPhase, u16, u16, u32)> = match kind {
        0 => vec![
            (0x1004, 0x1004, I1, 0x0c67, 0x14, 0x3002),
            (0x3000, 0x1006, H, 0x0c67, 0x14, 0x3000),
            (0x1006, 0x1006, F, 0x4e71, 0x1b, 0x3000),
        ],
        1 => vec![
            (0x1004, 0x1002, I1, 0x0c9f, 0x14, 0x3000),
            (0x1006, 0x1006, I2, 0x0c9f, 0x14, 0x3000),
            (0x3000, 0x1006, H, 0x0c9f, 0x14, 0x3000),
            (0x3002, 0x1006, L, 0x0c9f, 0x14, 0x3004),
            (0x1008, 0x1008, F, 0x4e71, 0x12, 0x3004),
        ],
        _ => vec![
            (0x1004, 0x1002, I1, 0x0cb9, 0x14, 0x3000),
            (0x1006, 0x1006, I2, 0x0cb9, 0x14, 0x3000),
            (0x1008, 0x1006, E1, 0x0cb9, 0x14, 0x3000),
            (0x100a, 0x100a, E2, 0x0cb9, 0x14, 0x3000),
            (0xabcd6000, 0x100a, H, 0x0cb9, 0x14, 0x3000),
            (0xabcd6002, 0x100a, L, 0x0cb9, 0x14, 0x3000),
            (0x100c, 0x100c, F, 0x4e71, 0x12, 0x3000),
        ],
    };
    rows.into_iter()
        .map(|(address, pc, phase, ird, sr, active)| {
            let mut r = c.r;
            r.set_address_register(7, active);
            r.set_status_register(sr | if c.r.supervisor() { 0x2000 } else { 0 });
            oracle::Transfer {
                access: oracle::access(
                    address,
                    c.r.supervisor(),
                    !matches!(phase, H | L),
                    if matches!(phase, H | L) {
                        AccessKind::Data
                    } else {
                        AccessKind::Prefetch
                    },
                ),
                phase,
                registers: r,
                internal_pc: pc,
                ird,
            }
        })
        .collect()
}
fn literal_entry(c: Case, t: oracle::Transfer, kind: u8, at: usize) -> bool {
    let (_, bus) = c.seed();
    let sp = t.registers.ssp;
    let ssw = match (kind, at) {
        (0, _) => {
            if c.r.supervisor() {
                0x0c76
            } else {
                0x0c72
            }
        }
        (1, _) => {
            if c.r.supervisor() {
                0x0c96
            } else {
                0x0c92
            }
        }
        (_, 6) => {
            if c.r.supervisor() {
                0x4e76
            } else {
                0x4e72
            }
        }
        (_, 4 | 5) => {
            if c.r.supervisor() {
                0x0cb5
            } else {
                0x0cb1
            }
        }
        _ => {
            if c.r.supervisor() {
                0x0cb6
            } else {
                0x0cb2
            }
        }
    };
    let expected = [
        ssw,
        (t.access.logical_address >> 16) as u16,
        t.access.logical_address as u16,
        t.ird,
        t.registers.status_register(),
        0,
        t.internal_pc as u16,
    ];
    let start = if kind == 0 { 4 } else { (at as u64 + 1) * 4 };
    let p = entry::predict(c, t, 2, start, bus.words, (None, None));
    let mut r = t.registers;
    r.ssp = sp - 14;
    r.pc = 0x43214000;
    r.set_status_register(t.registers.status_register() | 0x2000);
    expected
        .into_iter()
        .enumerate()
        .all(|(i, w)| oracle::sample(&p.words, sp - 14 + 2 * i as u32) == w)
        && p.registers == r
        && p.queue == Some([0x4e71, 0x7201])
        && p.count == 11
        && p.end == start + 50
}
fn reserve(mut c: Case, kind: u8) -> bool {
    let blocked = if kind == 0 { 2 } else { 4 };
    let prefix = if kind == 0 { 10 } else { 16 };
    c.start = u64::MAX - u64::from(u32::MAX) + 1 - prefix;
    let (mut cpu, mut bus) = c.seed();
    let memory = bus.words.clone();
    let literal = literals(c, kind);
    let mut r = literal[blocked - 1].registers;
    r.set_status_register(c.r.status_register());
    let result = cpu.step_physical_instruction(&mut bus);
    result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        && cpu.registers == r
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && bus.count == blocked
        && bus.entry_count == 0
        && bus.words == memory
        && bus.events == oracle::events(c, &literal[..blocked], &bus.elapsed, true)
        && bus.clock == c.start + prefix
        && cpu.clock().0 == c.start + prefix
        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)
}
pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for supervisor in [false, true] {
        for kind in 0..2 {
            let c = case(kind, supervisor);
            let (_, bus) = c.seed();
            let literal = literals(c, kind);
            let sound = oracle::plan(c, &bus.words) == literal
                && oracle::flags(c, &bus.words)
                    == if supervisor { 0x2000 } else { 0 } | if kind == 0 { 0x1b } else { 0x12 }
                && oracle::clocks(c, &bus.elapsed, literal.len(), true)
                    == if kind == 0 { 14 } else { 20 };
            for path in 0..3 {
                count += 1;
                bad += u32::from(
                    !sound
                        || if path == 2 {
                            !reserve(c, kind)
                        } else {
                            !run::check(
                                c,
                                if path == 1 {
                                    Some((literal.len() - 1, Failure::Host))
                                } else {
                                    None
                                },
                                0,
                            )
                        },
                );
            }
        }
        for (kind, at) in [
            (0, 0),
            (1, 0),
            (1, 1),
            (2, 2),
            (2, 3),
            (2, 4),
            (2, 5),
            (2, 6),
        ] {
            let c = case(kind, supervisor);
            let (_, bus) = c.seed();
            let literal = literals(c, kind);
            let sound =
                oracle::plan(c, &bus.words) == literal && literal_entry(c, literal[at], kind, at);
            for failure in [Failure::Host, Failure::Boundary] {
                count += 1;
                bad += u32::from(!sound || !run::check(c, Some((at, failure)), 0));
            }
        }
    }
    (count, bad + u32::from(count != 44))
}
