use super::device::Mutation;
use super::model::{Case, Failure};
use super::{Tally, check, model, seed};

fn source(mut c: Case, value: u32) -> Case {
    match c.p.mode {
        0 => c.r.d[c.p.register] = value,
        1 => {
            c.r.set_address_register(c.p.register, value);
        }
        7 if c.p.immediate() => {
            c.retained = if c.p.bits == 32 {
                (value >> 16) as u16
            } else {
                value as u16
            };
            c.low = value as u16;
        }
        _ => panic!("register source"),
    }
    c
}

fn target(mut c: Case, address: u32) -> Case {
    let p = c.p;
    match (p.mode, p.register) {
        (2 | 3, _) => {
            c.r.set_address_register(p.register, address);
        }
        (4, _) => {
            c.r.set_address_register(p.register, address.wrapping_add(p.step()));
        }
        (5, _) => {
            c.r.set_address_register(
                p.register,
                address.wrapping_sub_signed(i32::from(c.retained as i16)),
            );
        }
        (6, _) => {
            let delta = address.wrapping_sub(model::address(c, &c.r));
            c.r.set_address_register(
                p.register,
                c.r.address_register(p.register)
                    .unwrap()
                    .wrapping_add(delta),
            );
        }
        (7, 0) => c.retained = address as u16,
        (7, 1) => {
            c.retained = (address >> 16) as u16;
            c.low = address as u16;
        }
        (7, 2) => {
            let delta = address.wrapping_sub(c.r.pc.wrapping_add(2)) as i32;
            if delta == i32::from(delta as i16) {
                c.retained = delta as u16;
            } else {
                c.r.pc = (address & !1).wrapping_sub(2);
                c.retained = (address & 1) as u16;
            }
        }
        (7, 3) => {
            c.r.d[2] = address
                .wrapping_sub(c.r.pc.wrapping_add(2))
                .wrapping_add(16)
        }
        _ => panic!("memory source"),
    }
    c
}

fn results(tally: &mut Tally, c: Case, kinds: &[Failure], mark: fn(bool)) {
    for &kind in kinds {
        tally.add(check::run(c, |b| b.fault(c.p.count() - 1, kind, 4), mark));
    }
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    let two = [Failure::Complete, Failure::Boundary];
    let three = [Failure::Complete, Failure::Host, Failure::Boundary];
    seed::each(|base| match index {
        7 => {
            for next in [0x4afc, 0x4e72] {
                for ccr in [0, 31] {
                    let mut c = seed::status(base, ccr);
                    c.next = next;
                    if c.p.extensions() == 0 {
                        c.retained = next;
                    }
                    for elapsed in [4, 7] {
                        tally.add(check::run(
                            c,
                            |b| b.fault(c.p.count() - 1, Failure::Boundary, elapsed),
                            mark,
                        ));
                    }
                }
            }
        }
        8 => {
            for operand in [false, true] {
                if operand && !base.p.memory() {
                    continue;
                }
                let at = if operand {
                    base.p.extensions()
                } else {
                    base.p.count() - 1
                };
                let address = if operand {
                    model::address(base, &base.r)
                } else {
                    base.r.pc.wrapping_add(base.p.length() + 2)
                };
                for kind in three {
                    tally.add(check::run(
                        base,
                        |b| {
                            b.mutations[0] = Some(Mutation {
                                at,
                                address,
                                word: if operand { !base.operand } else { 0x4afc },
                                after: true,
                            });
                            b.fault(base.p.count() - 1, kind, 4);
                        },
                        mark,
                    ));
                }
            }
        }
        11 if base.p.index() => {
            for selector in 0..16 {
                for long in [false, true] {
                    for ignored in 0..8 {
                        for raw in [0, 0x7fff, 0xffff_8000, 0x1234_0001] {
                            for displacement in [-128_i8, -1, 0, 127] {
                                let mut c = base;
                                c.retained = selector << 12
                                    | u16::from(long) << 11
                                    | ignored << 8
                                    | u16::from(displacement as u8);
                                if selector < 8 {
                                    c.r.d[selector as usize] = raw;
                                } else {
                                    c.r.set_address_register(selector as usize - 8, raw);
                                }
                                tally.add(check::run(c, |_| {}, mark));
                            }
                        }
                    }
                }
            }
        }
        12 => {
            for pc in [0x00ff_fffc, 0xffff_fffc, 0xffff_fffe] {
                let mut c = base;
                c.r.pc = pc;
                results(&mut tally, c, &two, mark);
            }
        }
        13 if base.p.memory() => {
            for address in [0, 1, 0x00ff_fffe, 0x00ff_ffff, 0xffff_fffe, 0xffff_ffff] {
                results(&mut tally, target(base, address), &two, mark);
            }
        }
        14 if base.p.memory() => {
            for offset in [0, 2, base.p.length(), base.p.length() + 2] {
                results(
                    &mut tally,
                    target(base, base.r.pc.wrapping_add(offset)),
                    &two,
                    mark,
                );
            }
        }
        27 if base.p.immediate() => {
            for word in [0, 0xffff, 0x4afc, 0x7201] {
                for kind in three {
                    tally.add(check::run(
                        base,
                        |b| {
                            b.words.insert(base.r.pc.wrapping_add(2), word);
                            b.fault(base.p.count() - 1, kind, 4);
                        },
                        mark,
                    ));
                }
            }
        }
        28 if base.p.displacement() => {
            for displacement in [0, 1, 0x7fff, 0x8000, 0xffff, 0xff] {
                let mut c = base;
                c.retained = displacement;
                results(&mut tally, c, &two, mark);
            }
        }
        31 if base.p.mode == 0 => {
            let sign = 1 << (base.p.bits - 1);
            let mask = if base.p.bits == 32 {
                u32::MAX
            } else {
                (1 << base.p.bits) - 1
            };
            for low in [0, 1, sign, mask] {
                for high in [0, 0xaabb, 0xffff, 0x8000] {
                    let raw = if base.p.bits == 32 {
                        low ^ (high << 16)
                    } else if base.p.bits == 16 {
                        high << 16 | low
                    } else {
                        high << 16 | 0xcc00 | low
                    };
                    for ccr in [0, 31] {
                        tally.add(check::run(
                            source(seed::status(base, ccr), raw),
                            |_| {},
                            mark,
                        ));
                    }
                }
            }
        }
        32 if base.p.mode == 1 => {
            for raw in [
                0,
                1,
                0x7fff,
                0x8000,
                0xffff,
                0x1234_8000,
                0x8000_0000,
                0xffff_ffff,
            ] {
                for ccr in [0, 31] {
                    tally.add(check::run(
                        source(seed::status(base, ccr), raw),
                        |_| {},
                        mark,
                    ));
                }
            }
        }
        33 if base.p.cmpa && base.p.bits == 16 => {
            for raw in [0, 0x7ffe, 0x7fff, 0x8000, 0x8001, 0xffff] {
                for ccr in [0, 31] {
                    tally.add(check::run(
                        source(seed::status(base, ccr), raw),
                        |_| {},
                        mark,
                    ));
                }
            }
        }
        34 | 36
            if if index == 34 {
                base.p.cmpa && base.p.destination == 7
            } else {
                base.p.register == 7 && matches!(base.p.mode, 1..=6)
            } =>
        {
            for inactive in [0, 1, 0x1234_5678, 0xffff_ffff] {
                let original_ccr = [base.r.status_register() & 31];
                for &ccr in if index == 34 {
                    &[0, 15, 16, 31][..]
                } else {
                    &original_ccr[..]
                } {
                    let mut c = seed::status(base, ccr);
                    if c.r.supervisor() {
                        c.r.usp = inactive;
                    } else {
                        c.r.ssp = inactive;
                    }
                    results(&mut tally, c, &three, mark);
                }
            }
        }
        35 if base.p.immediate() && base.p.bits == 8 => {
            for upper in 0..256 {
                for low in [0, 1, 0x7f, 0x80, 0xff, 0x55, 0xaa, 0x40] {
                    for ccr in [0, 31] {
                        let mut c = seed::status(base, ccr);
                        c.retained = upper << 8 | low;
                        tally.add(check::run(c, |_| {}, mark));
                    }
                }
            }
        }
        11..=14 | 27 | 28 | 31..=36 => {}
        _ => panic!("compare directed section"),
    });
    tally.finish()
}
