use super::*;

pub(super) fn goldens() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for supervisor in [false, true] {
                for failure in [false, true] {
                    let (value, desired, q, r) = if subtract {
                        (1, 0x20000, 0x2ffff, 0x1ffff)
                    } else {
                        (3, 0x1fffe, 0x10001, 0x20001)
                    };
                    let mut c = Case {
                        subtract,
                        long,
                        supervisor,
                        family: 3,
                        source: 7,
                        destination: 7,
                        value,
                        desired,
                        ..Case::default()
                    };
                    c.failure = failure.then_some(c.count() - 1);
                    let (_, bus, p) = run::seed(c);
                    let ea = desired - c.stride();
                    let good = p.source == value
                        && p.sampled == desired
                        && p.phases[c.extensions()].access.logical_address == ea
                        && sample(&bus.words, ea) == if long { 0 } else { value as u16 }
                        && p.phases.last().unwrap().registers.address_register(7) == Some(q)
                        && p.full.address_register(7) == Some(r)
                        && p.phases.iter().all(|t| {
                            t.registers.status_register() == c.initial().status_register()
                        })
                        && run::run(c);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad + u32::from(count != 16))
}

pub(super) fn completion() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for family in 2..=10 {
                for source in 0..if family <= 6 { 8 } else { 1 } {
                    for destination in 0..8 {
                        for supervisor in [false, true] {
                            for status in STATUS {
                                for (value, desired) in fixture::pairs(subtract, long) {
                                    for wait in [0, 3] {
                                        let c = Case {
                                            subtract,
                                            long,
                                            family,
                                            source,
                                            destination,
                                            supervisor,
                                            status,
                                            value,
                                            desired,
                                            wait,
                                            ..Case::default()
                                        };
                                        let (.., p) = run::seed(c);
                                        count += 1;
                                        bad += u32::from(p.sampled != desired || !run::run(c));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 90112))
}

pub(super) fn faults() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for family in 2..=10 {
                for source in 0..if family <= 6 { 8 } else { 1 } {
                    for destination in 0..8 {
                        let base = Case {
                            subtract,
                            long,
                            family,
                            source,
                            destination,
                            wait: 3,
                            ..Case::default()
                        };
                        for at in 0..base.count() {
                            for supervisor in [false, true] {
                                for status in STATUS {
                                    for (value, desired) in fixture::pairs(subtract, long) {
                                        let c = Case {
                                            supervisor,
                                            status,
                                            value,
                                            desired,
                                            failure: Some(at),
                                            ..base
                                        };
                                        let (.., p) = run::seed(c);
                                        count += 1;
                                        bad += u32::from(p.sampled != desired || !run::run(c));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 134144))
}

fn target(mut c: Case, address: u32) -> Case {
    c.source = 0;
    c.destination = 1;
    match c.family {
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
    *bad += u32::from(!run::run(c));
}

pub(super) fn directed() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for subtract in [false, true] {
        for long in [false, true] {
            for family in [6, 10] {
                for supervisor in [false, true] {
                    for selector in 0..16 {
                        for wide in [false, true] {
                            for sign in [0x70, 0x90] {
                                for ignored in 0..8 {
                                    for (value, desired) in fixture::pairs(subtract, long) {
                                        let base = Case {
                                            subtract,
                                            long,
                                            family,
                                            supervisor,
                                            destination: selector & 7,
                                            value,
                                            desired,
                                            extension: (selector as u16) << 12
                                                | if wide { 0x800 } else { 0 }
                                                | ignored << 8
                                                | sign,
                                            ..Case::default()
                                        };
                                        for failure in [None, Some(base.count() - 1)] {
                                            check(Case { failure, ..base }, &mut count, &mut bad);
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
    bad += u32::from(count != 65536);
    for subtract in [false, true] {
        for supervisor in [false, true] {
            for value in [0xffff, 0x8000, 0x7fff] {
                for desired in [0, 0x20000, 0xffff8000] {
                    for failure in [None, Some(1)] {
                        check(
                            Case {
                                subtract,
                                supervisor,
                                value,
                                desired,
                                failure,
                                ..Case::default()
                            },
                            &mut count,
                            &mut bad,
                        );
                    }
                }
            }
        }
    }
    bad += u32::from(count != 65608);
    for subtract in [false, true] {
        for long in [false, true] {
            for family in 2..=10 {
                for supervisor in [false, true] {
                    let base = Case {
                        subtract,
                        long,
                        family,
                        supervisor,
                        ..Case::default()
                    };
                    for at in 0..base.count() {
                        let (word, extra) = if family == 8 && at == 0 {
                            (
                                0x6020,
                                Some((0xabcd6020, if long { 0x80000001 } else { 0x8001 })),
                            )
                        } else {
                            (
                                if at == base.count() - 1 || at < base.extensions() {
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
                            ..base
                        };
                        let (.., plan) = run::seed(mutated);
                        let expected = if family == 8 && at == 0 {
                            if long { 0x8000_0001 } else { 0x8001 }
                        } else if at == base.extensions() {
                            if long { 0x8001_0002 } else { 0x8001 }
                        } else if long && at == base.extensions() + 1 {
                            0x8001
                        } else {
                            base.value
                        };
                        bad += u32::from(plan.source != expected);
                        for failure in [None, Some(at)] {
                            check(
                                Case {
                                    mutate: Some((at, word)),
                                    extra,
                                    failure,
                                    ..base
                                },
                                &mut count,
                                &mut bad,
                            );
                        }
                    }
                    for offset in [
                        base.end().wrapping_sub(base.pc),
                        base.end().wrapping_sub(base.pc) + 2,
                    ] {
                        let c = target(Case { pc: 0x1000, ..base }, 0x1000 + offset);
                        for failure in [None, Some(c.count() - 1)] {
                            check(Case { failure, ..c }, &mut count, &mut bad);
                        }
                    }
                    for pc in [0xffff_fffc, 0xffff_fffe] {
                        for failure in [None, Some(base.count() - 1)] {
                            check(
                                Case {
                                    pc,
                                    failure,
                                    ..base
                                },
                                &mut count,
                                &mut bad,
                            );
                        }
                    }
                    for address in [0xffff_fffeu32, 0x00ff_fffe] {
                        if family == 7 && address != 0xffff_fffe {
                            continue;
                        }
                        let pc = if family == 9 {
                            address.wrapping_sub(0x1000)
                        } else {
                            PC
                        };
                        let c = target(Case { pc, ..base }, address);
                        let (.., p) = run::seed(c);
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
    (count, bad + u32::from(count != 66928))
}
