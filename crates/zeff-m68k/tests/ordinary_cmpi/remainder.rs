use super::*;
use fixture::{Case, Outcome};
use model::{Spec, bank, pairs};
use oracle::Count;

pub(super) const COUNTS: [u32; 7] = [40, 138240, 221440, 221440, 22144, 64, 96];

fn literal(size: u8, memory: bool, supervisor: bool, path: u8) -> Case {
    let long = size == 4;
    let prefix_reads = 1 + usize::from(long) + usize::from(memory);
    let tail = u64::from(long) * 2;
    let wait = u32::from(matches!(path, 1 | 2)) * 3;
    let prefix = prefix_reads as u64 * (4 + u64::from(wait));
    let start = match path {
        3 => u64::MAX - model::BUDGET + 1 - prefix,
        4 => u64::MAX - model::BUDGET - prefix - tail,
        _ => 37,
    };
    let mut r = fixture::registers(supervisor, 0x3000);
    r.d[0] = match size {
        1 => 0xaabb_cc7f,
        2 => 0xaabb_7fff,
        _ => 0x7fff_ffff,
    };
    r.a[0] = 0x3001;
    let word = 0x0c00
        | if memory { 0x10 } else { 0 }
        | match size {
            1 => 0,
            2 => 0x40,
            _ => 0x80,
        };
    let immediate = if size == 1 { 0xff80 } else { 0x8000 };
    let mut c = Case::new("remaining CMPI", word, r, start, false);
    c.queue = [word, immediate];
    c.final_queue = c.queue;
    for (address, value) in [
        (0x1002, immediate),
        (0x1004, if long { 0 } else { 0x4e71 }),
        (0x1006, if long { 0x4e71 } else { 0x7201 }),
        (0x1008, 0x7201),
        (0x3000, 0xaa7f),
    ] {
        c.bus.word(address, value);
    }
    let mut clock = start;
    c.read(clock, 0x1004, (false, if long { 0 } else { 0x4e71 }, wait));
    clock += 4 + u64::from(wait);
    if long {
        c.read(clock, 0x1006, (false, 0x4e71, wait));
        clock += 4 + u64::from(wait);
    }
    if memory {
        c.read(clock, 0x3001, (true, 0xaa7f, wait));
        if let Some(device::Event::Read(access, ..)) = c.events.last_mut() {
            access.lanes = ByteLanes::Lower;
        }
        clock += 4 + u64::from(wait);
    }
    let final_address = if long { 0x1008 } else { 0x1006 };
    if path != 3 {
        let final_wait = if path == 4 { u32::MAX } else { wait };
        c.read(clock, final_address, (false, 0x7201, final_wait));
        clock += 4 + u64::from(final_wait);
        c.final_r
            .set_status_register((r.status_register() & !15) | 11);
    }
    c.outcome = if path == 3 {
        Outcome::Overflow
    } else if path == 2 {
        c.bus.fail = Some(prefix_reads);
        Outcome::Host(fixture::access(final_address, supervisor, false))
    } else {
        if long {
            c.idle(clock);
            clock += tail;
        }
        c.final_r.pc = if long { 0x1006 } else { 0x1004 };
        c.final_queue = [0x4e71, 0x7201];
        Outcome::Complete
    };
    c.clock = clock;
    c.duration = clock - start;
    c
}

pub(super) fn first() -> (u32, u32) {
    let mut count = 0;
    let mut bad = 0;
    for (size, memory) in [(1, false), (2, false), (4, false), (1, true)] {
        for supervisor in [false, true] {
            for path in 0..5 {
                count += 1;
                bad += u32::from(!check::run(literal(size, memory, supervisor, path)));
            }
        }
    }
    (count, bad)
}

fn each(mut visit: impl FnMut(Spec)) {
    for supervisor in [false, true] {
        for size in [1, 2, 4] {
            for reg in 0..8 {
                visit(Spec::new(size, 0, reg, supervisor));
            }
        }
        for mode in 2..=7 {
            for reg in 0..if mode == 7 { 2 } else { 8 } {
                for lane in 0..2 {
                    let mut s = Spec::new(1, mode, reg, supervisor);
                    let address = model::address(s, s.extension) & !1 | lane;
                    s.target(address);
                    visit(s);
                }
            }
        }
    }
}

fn matrix(path: u32) -> (u32, u32) {
    let mut total = Count::default();
    each(|form| {
        assert!(!form.selected());
        for ccr in 0..32 {
            for (operand, immediate) in pairs(form.size) {
                for wait in if path == 1 { &[0, 3][..] } else { &[3][..] } {
                    let mut s = bank(form, form.r.supervisor(), ccr);
                    s.operand = operand;
                    s.immediate = immediate | if s.size == 1 { 0xff00 } else { 0 };
                    s.wait = *wait;
                    let (s, bus) = s.seed();
                    let p = plan::validate(s, &bus);
                    if path == 1 {
                        total.case(s, bus);
                    } else {
                        for at in 0..capacity::callbacks(&p) {
                            let mut b = bus.clone();
                            if path == 2 {
                                b.fail = Some(at);
                            } else {
                                capacity::blocked(&p, &mut b, at);
                            }
                            total.case(s, b);
                        }
                    }
                }
            }
        }
    });
    total.result()
}

fn maximum() -> (u32, u32) {
    let mut total = Count::default();
    each(|form| {
        for ccr in [0, 1, 16, 31] {
            for pair in [0, 3, 4, 9] {
                let mut s = bank(form, form.r.supervisor(), ccr);
                (s.operand, s.immediate) = pairs(s.size)[pair];
                let (s, bus) = s.seed();
                let p = plan::validate(s, &bus);
                for at in 0..capacity::callbacks(&p) {
                    for host in [false, true] {
                        let mut b = bus.clone();
                        b.waits[at] = u32::MAX;
                        if host {
                            b.fail = Some(at);
                        }
                        total.case(s, b);
                    }
                }
            }
        }
    });
    total.result()
}

fn idles() -> (u32, u32) {
    let mut total = Count::default();
    for supervisor in [false, true] {
        for mode in [4, 6] {
            for reg in 0..8 {
                for room in [1, 2] {
                    let (s, mut b) = Spec::new(1, mode, reg, supervisor).seed();
                    let p = plan::validate(s, &b);
                    capacity::source_idle(&p, &mut b, 0, room);
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}

fn tails() -> (u32, u32) {
    let mut total = Count::default();
    for supervisor in [false, true] {
        for reg in 0..8 {
            for room in 0..=2 {
                for host in [false, true] {
                    let (s, mut b) = Spec::new(4, 0, reg, supervisor).seed();
                    let p = plan::validate(s, &b);
                    b.clock =
                        u64::MAX - model::BUDGET - capacity::prefix(&p, &b, p.final_read) - room;
                    b.waits[p.final_read] = u32::MAX;
                    if host {
                        b.fail = Some(p.final_read);
                    }
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}

pub(super) fn section(index: u32) -> (u32, u32) {
    let result = match index {
        0 => first(),
        1..=3 => matrix(index),
        4 => maximum(),
        5 => idles(),
        6 => tails(),
        _ => panic!("ordinary CMPI remainder section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
