use super::*;
use fixture::{Case, OPS};

pub(super) fn target(mut c: Case, address: u32) -> Case {
    c.source = 0;
    c.destination = 5;
    match c.ea {
        2 | 3 => c.source_base = Some(address),
        4 => c.source_base = Some(address.wrapping_add(c.stride())),
        5 => {
            c.extension = 0;
            c.source_base = Some(address);
        }
        6 => {
            c.extension = 0x0800;
            c.source_base = Some(address.wrapping_sub(0xaabb9000));
        }
        7 => {
            assert_eq!(i32::from(address as i16) as u32, address);
            c.absolute = address;
        }
        8 => c.absolute = address,
        9 => {
            let delta = address.wrapping_sub(c.pc.wrapping_add(2));
            assert_eq!(i32::from(delta as i16) as u32, delta);
            c.extension = delta as u16;
        }
        10 => {
            c.extension = 0x0800;
            c.d0 = Some(address.wrapping_sub(c.pc.wrapping_add(2)));
        }
        _ => unreachable!(),
    }
    c
}
fn check(c: Case, count: &mut u32, bad: &mut u32) {
    *count += 1;
    *bad += u32::from(!run::check(c));
}
pub(super) fn indices() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in [6, 10] {
                for supervisor in [false, true] {
                    for selector in 0..16 {
                        for wide in [false, true] {
                            for negative in [false, true] {
                                for ignored in 0..8 {
                                    for (value, desired) in fixture::index_pairs(long) {
                                        let mut c = Case {
                                            operation,
                                            long,
                                            ea,
                                            supervisor,
                                            destination: selector & 7,
                                            value,
                                            desired,
                                            wait: 3,
                                            extension: (selector as u16) << 12
                                                | if wide { 0x800 } else { 0 }
                                                | ignored << 8,
                                            ..Case::default()
                                        };
                                        let r = c.initial();
                                        let raw = if selector < 8 {
                                            r.d[selector]
                                        } else {
                                            oracle::an(r, selector & 7)
                                        };
                                        let magnitude = 2 + (raw & 1) as i8;
                                        c.extension |=
                                            (if negative { -magnitude } else { magnitude }) as u8
                                                as u16;
                                        let (_, _, p) = run::seed(c);
                                        bad += u32::from(
                                            p.phases[c.extensions()].access.logical_address & 1
                                                != 0
                                                || r.d[c.destination]
                                                    != if long {
                                                        desired
                                                    } else {
                                                        0xaabb0000 | desired
                                                    },
                                        );
                                        for failure in [None, Some(c.count() - 1)] {
                                            check(Case { failure, ..c }, &mut count, &mut bad);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 131072))
}
pub(super) fn mutation() -> (u32, u32) {
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
                        wait: 3,
                        ..Case::default()
                    };
                    for at in 0..c.count() {
                        let (word, extra) = if ea == 8 && at == 0 {
                            (
                                0x6020,
                                Some((0xabcd6020, if long { 0x80000001 } else { 0x8001 })),
                            )
                        } else {
                            (
                                if at == c.count() - 1 || at < c.extensions() {
                                    0x8aa6
                                } else {
                                    0x8001
                                },
                                None,
                            )
                        };
                        let mutated = Case {
                            mutate: Some((at, word)),
                            extra,
                            ..c
                        };
                        let (_, _, p) = run::seed(mutated);
                        let expected = if ea == 8 && at == 0 {
                            if long { 0x80000001 } else { 0x8001 }
                        } else if at == c.extensions() {
                            if long { 0x80010002 } else { 0x8001 }
                        } else if long && at == c.extensions() + 1 {
                            0x8001
                        } else {
                            c.value
                        };
                        bad += u32::from(p.source != expected);
                        for failure in [None, Some(at)] {
                            check(Case { failure, ..mutated }, &mut count, &mut bad);
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 944))
}
pub(super) fn overlap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for supervisor in [false, true] {
                    let base = Case {
                        operation,
                        long,
                        ea,
                        supervisor,
                        pc: 0x1000,
                        wait: 3,
                        ..Case::default()
                    };
                    for address in [base.end(), base.end().wrapping_add(2)] {
                        let c = target(base, address);
                        let (_, _, p) = run::seed(c);
                        bad +=
                            u32::from(p.phases[c.extensions()].access.logical_address != address);
                        for failure in [None, Some(c.count() - 1)] {
                            check(Case { failure, ..c }, &mut count, &mut bad);
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 576))
}
pub(super) fn program_wrap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for supervisor in [false, true] {
                    for pc in [0xffff_fffc, 0xffff_fffe] {
                        let c = Case {
                            operation,
                            long,
                            ea,
                            supervisor,
                            pc,
                            wait: 3,
                            ..Case::default()
                        };
                        for failure in [None, Some(c.count() - 1)] {
                            check(Case { failure, ..c }, &mut count, &mut bad);
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 576))
}
pub(super) fn operand_wrap() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for supervisor in [false, true] {
                    for address in [0xffff_fffeu32, 0x00ff_fffe] {
                        if ea == 7 && address != 0xffff_fffe {
                            continue;
                        }
                        let pc = if ea == 9 {
                            address.wrapping_sub(0x1000)
                        } else {
                            PC
                        };
                        let c = target(
                            Case {
                                operation,
                                long,
                                ea,
                                supervisor,
                                pc,
                                wait: 3,
                                ..Case::default()
                            },
                            address,
                        );
                        let (_, _, p) = run::seed(c);
                        bad +=
                            u32::from(p.phases[c.extensions()].access.logical_address != address);
                        for failure in [None, Some(c.count() - 1)] {
                            check(Case { failure, ..c }, &mut count, &mut bad);
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 544))
}
