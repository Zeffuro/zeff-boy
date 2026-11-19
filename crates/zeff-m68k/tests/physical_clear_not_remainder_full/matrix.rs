use super::model::{Case, Failure};
use super::{check, seed};

const VALUES: [[u32; 8]; 3] = [
    [0, 1, 127, 128, 255, 85, 170, 64],
    [0, 1, 32767, 32768, 65535, 21845, 43690, 16384],
    [
        0,
        1,
        65536,
        32768,
        2147483648,
        u32::MAX,
        1431655765,
        2863311530,
    ],
];
const WAITS: [[u32; 5]; 2] = [[7, 11, 5, 13, 9], [15, 9, 17, 7, 11]];
const REPORTS: [(Failure, u32); 3] = [
    (Failure::Host, 6),
    (Failure::Unavailable, 5),
    (Failure::Boundary, 8),
];

fn size(c: Case) -> usize {
    usize::from(c.p.bits / 16)
}
pub(super) fn source(mut c: Case, value: u32) -> Case {
    if c.p.memory() {
        c.operand = if super::model::address(c, &c.r) & 1 == 0 {
            (value as u16) << 8 | 0x5a
        } else {
            0x5a00 | value as u16
        };
    } else {
        c.r.d[c.p.register] = c.r.d[c.p.register] & !c.p.mask() | value & c.p.mask();
    }
    c
}
pub(super) fn status(mut c: Case, ccr: u16, mask: u16) -> Case {
    c.r.set_status_register(c.r.status_register() & 0x2000 | mask << 8 | ccr);
    c
}
fn waits(b: &mut super::device::Device, c: Case, variant: usize) {
    if variant > 0 {
        for (at, report) in b.primary.iter_mut().enumerate().take(c.p.count()) {
            report.elapsed = WAITS[variant - 1][at];
        }
    }
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let all_ccrs: [u16; 32] = std::array::from_fn(|i| i as u16);
    let mut count = 0;
    let mut bad = 0;
    let mut attempt = |c: Case, wait: usize, fault: Option<(usize, Failure, u32)>| {
        let good = check::run(
            c,
            |b| {
                waits(b, c, wait);
                if let Some((at, kind, elapsed)) = fault {
                    b.fault(at, kind, elapsed);
                }
            },
            mark,
        );
        count += 1;
        bad += u32::from(!good);
    };
    seed::each(|base| match index {
        3 => {
            for ccr in 0..32 {
                for mask in 0..8 {
                    for value in VALUES[size(base)] {
                        for wait in 0..2 {
                            attempt(source(status(base, ccr, mask), value), wait, None);
                        }
                    }
                }
            }
        }
        4..=6 => {
            for at in 0..base.p.count() {
                let ccrs: &[u16] = match index {
                    4 => &all_ccrs,
                    5 => &[31],
                    _ => &[0, 15, 16, 31],
                };
                for &ccr in ccrs {
                    let masks: &[u16] = if index == 5 {
                        &[0, 1, 2, 3, 4, 5, 6, 7]
                    } else {
                        &[5]
                    };
                    for &mask in masks {
                        let boundaries = [0, 1u32 << (base.p.bits - 1)];
                        let values: &[u32] = if index == 4 {
                            &VALUES[size(base)]
                        } else {
                            &boundaries
                        };
                        for &value in values {
                            let c = source(status(base, ccr, mask), value);
                            if index == 6 {
                                for (kind, elapsed) in
                                    [(Failure::Complete, 4), REPORTS[0], REPORTS[1], REPORTS[2]]
                                {
                                    for wait in 1..=2 {
                                        let fault = (kind != Failure::Complete)
                                            .then_some((at, kind, elapsed));
                                        attempt(c, wait, fault);
                                    }
                                }
                            } else {
                                for (kind, elapsed) in REPORTS {
                                    attempt(c, 0, Some((at, kind, elapsed)));
                                }
                            }
                        }
                    }
                }
            }
        }
        7 if base.p.bits == 8 => {
            for value in 0..256 {
                attempt(
                    source(status(base, if value & 1 == 0 { 0 } else { 16 }, 5), value),
                    0,
                    None,
                );
            }
        }
        8 if base.p.mode == 0 && base.p.bits == 32 => {
            for ccr in 0..32 {
                for high in [0, 1, 32768, 65535] {
                    for low in [0, 1, 32768, 65535] {
                        attempt(source(status(base, ccr, 5), high << 16 | low), 0, None);
                    }
                }
            }
        }
        9 if base.p.mode == 0 && base.p.bits < 32 => {
            for ccr in 0..32 {
                for low in [0, 1, 1 << (base.p.bits - 1), base.p.mask()] {
                    for upper in [0, 1, 1 << (31 - base.p.bits), u32::MAX >> base.p.bits] {
                        let mut c = source(status(base, ccr, 5), low);
                        c.r.d[c.p.register] = upper << base.p.bits | low;
                        attempt(c, 0, None);
                    }
                }
            }
        }
        7..=9 => (),
        _ => panic!("primary matrix section"),
    });
    (count, bad)
}

pub(super) fn extensions(index: u32, mark: fn(bool)) -> (u32, u32) {
    assert!(matches!(index, 11 | 13 | 14));
    let mut count = 0;
    let mut bad = 0;
    for page in [0x4200, 0x4600] {
        for supervisor in [false, true] {
            for register in 0..if index == 14 { 1 } else { 8 } {
                let mode = if index == 11 {
                    6
                } else if index == 13 {
                    5
                } else {
                    7
                };
                for extension in 0..=u16::MAX {
                    let mut c = seed::make(page | mode << 3 | register, supervisor, 0);
                    c.retained = extension;
                    if index == 11 {
                        c.r.d[3] = 0x4444_4444;
                    }
                    if mode < 7 {
                        c.r.set_address_register(
                            c.p.register,
                            if register == 7 {
                                if supervisor { 0xa000 } else { 0xb000 }
                            } else {
                                0x3000 + u32::from(register) * 0x1000
                            },
                        );
                    }
                    c = source(c, 0);
                    let good = check::run(c, |_| {}, mark);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}
