use super::device::{Device, Mutation};
use super::matrix::{source, status};
use super::model::{Case, Failure};
use super::{check, seed};

const REPORTS: [(Failure, u32); 4] = [
    (Failure::Complete, 4),
    (Failure::Host, 6),
    (Failure::Unavailable, 5),
    (Failure::Boundary, 8),
];
const ROOMS: [u64; 7] = [
    0,
    1,
    2,
    3,
    u32::MAX as u64 - 1,
    u32::MAX as u64,
    u32::MAX as u64 + 1,
];

fn place(mut c: Case, desired: u32) -> Case {
    if c.p.mode == 7 {
        c.retained = if c.p.register == 0 {
            desired as u16
        } else {
            (desired >> 16) as u16
        };
        c.low = desired as u16;
    } else {
        let old = super::model::address(c, &c.r);
        let base = c.r.address_register(c.p.register).unwrap();
        c.r.set_address_register(c.p.register, base.wrapping_add(desired.wrapping_sub(old)));
    }
    source(c, 0)
}
fn transfer(c: Case, at: usize) -> u32 {
    if at < c.p.extensions() {
        c.r.pc.wrapping_add(4 + at as u32 * 2)
    } else if c.p.memory() && (at == c.p.extensions() || at == c.p.count() - 1) {
        super::model::address(c, &c.r)
    } else {
        c.r.pc.wrapping_add(c.p.length() + 2)
    }
}
fn slots(c: Case, mut f: impl FnMut(usize, Failure, u32, bool)) {
    for at in 0..c.p.count() {
        for (kind, elapsed) in REPORTS {
            f(at, kind, elapsed, false);
        }
        if c.p.memory() && at == c.p.count() - 1 {
            for (kind, elapsed) in REPORTS.into_iter().skip(1) {
                f(at, kind, elapsed, true);
            }
        }
    }
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut count = 0;
    let mut bad = 0;
    let mut attempt = |c: Case, configure: &dyn Fn(&mut Device)| {
        count += 1;
        let good = if c.r.pc & 1 != 0 {
            assert_eq!(index, 19);
            let mut bus = Device::seed(c);
            configure(&mut bus);
            super::construction::reject(c, bus, mark)
        } else {
            check::run(c, configure, mark)
        };
        bad += u32::from(!good);
    };
    seed::each(|base| {
        let p = base.p;
        match index {
            10 if matches!(p.mode, 2..=6) && p.register == 7 => {
                for inactive in [0, 1, 0x1234_5678, u32::MAX] {
                    let mut c = base;
                    if c.r.supervisor() {
                        c.r.usp = inactive;
                    } else {
                        c.r.ssp = inactive;
                    }
                    for (kind, elapsed) in REPORTS {
                        attempt(c, &|b| b.fault(p.extensions() + 1, kind, elapsed));
                    }
                }
            }
            16 => {
                for pc in [
                    0x2000,
                    0xffff_fffa,
                    0xffff_fffc,
                    0xffff_fffe,
                    0x00ff_fffc,
                    0x00ff_fffe,
                ] {
                    let mut c = base;
                    c.r.pc = pc;
                    for (kind, elapsed) in REPORTS {
                        attempt(c, &|b| {
                            b.fault(p.extensions() + usize::from(p.memory()), kind, elapsed)
                        });
                    }
                }
            }
            17 if p.memory() => {
                for desired in [
                    0,
                    1,
                    0x00ff_fffe,
                    0x00ff_ffff,
                    0xffff_fffe,
                    u32::MAX,
                    0x8000_0000,
                    0x8000_0001,
                ] {
                    let c = place(base, desired);
                    for (kind, elapsed) in REPORTS {
                        attempt(c, &|b| b.fault(p.extensions(), kind, elapsed));
                    }
                }
            }
            18 => {
                for at in 0..p.count() {
                    let anchor = base.r.pc.wrapping_add(p.length());
                    let code = if p.extensions() == 2 && at == 0 {
                        base.r.pc.wrapping_add(6)
                    } else if at < p.extensions() || p.memory() && at == p.extensions() {
                        anchor.wrapping_add(2)
                    } else {
                        anchor.wrapping_add(4)
                    };
                    let operand = if p.memory() {
                        super::model::address(base, &base.r)
                    } else {
                        anchor
                    };
                    for address in [transfer(base, at), code, operand, operand] {
                        for (kind, elapsed) in REPORTS {
                            attempt(base, &|b| {
                                b.fault(at, kind, elapsed);
                                b.mutations[0] = Some(Mutation {
                                    at,
                                    address,
                                    word: 0xa581,
                                    after: false,
                                });
                            });
                        }
                    }
                }
            }
            19 => {
                for route in 0..6 {
                    let mut c = base;
                    let target = match route {
                        0 => c.r.pc,
                        1 => c.r.pc.wrapping_add(2),
                        2 => c.r.pc.wrapping_add(p.length() + 2),
                        3 => c.r.ssp.wrapping_sub(14),
                        4 => 8,
                        _ => c.handler,
                    };
                    if route < 3 && p.memory() {
                        c = place(c, target);
                    }
                    if route >= 3 {
                        c.r.pc = target;
                    }
                    for (kind, elapsed) in REPORTS {
                        attempt(c, &|b| {
                            if route < 3 && !p.memory() {
                                b.words.insert(target, 0xa581);
                            }
                            b.fault(p.extensions() + usize::from(p.memory()), kind, elapsed);
                        });
                    }
                }
            }
            20 => {
                for at in 0..p.count() {
                    for next in [0x4e71, 0x4a45, 0xffff, 0x4afc, 0x4e72] {
                        let mut c = base;
                        c.next = next;
                        if p.extensions() == 0 {
                            c.retained = next;
                        }
                        for (kind, elapsed) in REPORTS.into_iter().skip(1) {
                            attempt(c, &|b| b.fault(at, kind, elapsed));
                        }
                    }
                }
            }
            21 => {
                for at in 0..p.count() {
                    for room in [u32::MAX as u64 - 1, u32::MAX as u64, u32::MAX as u64 + 1] {
                        let mut c = base;
                        c.start = u64::MAX - u64::from(p.idle()) * 2 - at as u64 * 4 - room;
                        attempt(c, &|_| {});
                    }
                }
            }
            22 if p.idle() => {
                for room in ROOMS {
                    let mut c = base;
                    c.start = u64::MAX - room;
                    attempt(c, &|_| {});
                }
            }
            23 if p.tail() => {
                for room in ROOMS {
                    let mut c = base;
                    c.start = u64::MAX - u32::MAX as u64 - room;
                    attempt(c, &|b| b.fault(0, Failure::Complete, u32::MAX));
                }
            }
            24 | 25 => slots(base, |at, kind, _, deposit| {
                for duration_index in 0..if index == 24 { 4 } else { 1 } {
                    let elapsed = if index == 24 {
                        duration_index
                    } else {
                        u32::MAX
                    };
                    let mut c = base;
                    if index == 25 {
                        c.start =
                            u64::MAX - u64::from(p.idle()) * 2 - at as u64 * 4 - u32::MAX as u64;
                    }
                    attempt(c, &|b| {
                        b.fault(at, kind, elapsed);
                        b.primary_deposit[at] = deposit;
                    });
                }
            }),
            26 => {
                let mut c = base;
                c.start = u64::MAX
                    - u64::from(p.idle()) * 2
                    - (p.count() as u64 - 1) * 4
                    - u32::MAX as u64
                    - u64::from(p.tail()) * 2;
                attempt(c, &|b| b.fault(p.count() - 1, Failure::Complete, u32::MAX));
            }
            27 if p.memory() => {
                for ccr in 0..32 {
                    for value in [0, 1, 127, 128, 255, 85, 170, 64] {
                        let c = source(status(base, ccr, 5), value);
                        for (kind, elapsed) in REPORTS.into_iter().skip(1) {
                            attempt(c, &|b| {
                                b.fault(p.count() - 1, kind, elapsed);
                                b.primary_deposit[p.count() - 1] = true;
                            });
                        }
                    }
                }
            }
            10 | 17 | 22 | 23 | 27 => (),
            _ => panic!("directed section"),
        }
    });
    (count, bad)
}

pub(super) fn aliases(index: u32, mark: fn(bool)) -> (u32, u32) {
    assert!(matches!(index, 12 | 15));
    let mut count = 0;
    let mut bad = 0;
    let mut attempt = |c: Case, phase: usize| {
        count += 1;
        bad += u32::from(!check::run(
            c,
            |b| {
                if phase > 0 {
                    b.fault(phase - 1, Failure::Boundary, 8);
                }
            },
            mark,
        ));
    };
    if index == 12 {
        for register in 0..8 {
            for page in [0x4200, 0x4600] {
                for supervisor in [false, true] {
                    for selector in 0..16 {
                        for width in 0..2 {
                            for ignored in 0..8 {
                                for offset in [0, 1, 128, 255] {
                                    let mut c = seed::make(page | 0x30 | register, supervisor, 0);
                                    c.retained =
                                        selector << 12 | width << 11 | ignored << 8 | offset;
                                    c.r.d[3] = 0x4444_4444;
                                    c.r.set_address_register(
                                        c.p.register,
                                        if register == 7 {
                                            if supervisor { 0xa000 } else { 0xb000 }
                                        } else {
                                            0x3000 + u32::from(register) * 0x1000
                                        },
                                    );
                                    c = source(c, 0);
                                    for phase in 0..5 {
                                        attempt(c, phase);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        for page in [0x4200, 0x4600] {
            for supervisor in [false, true] {
                for high in [0, 1, 255, 256, 32767, 32768, 65280, 65535] {
                    for low in [0, 1, 255, 256, 32767, 32768, 65280, 65535] {
                        let mut c = seed::make(page | 0x39, supervisor, 0);
                        c.retained = high;
                        c.low = low;
                        c = source(c, 0);
                        for phase in 0..6 {
                            attempt(c, phase);
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
