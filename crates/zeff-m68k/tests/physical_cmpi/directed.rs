use super::*;

fn paths(c: Case) -> (u32, u32) {
    let mut bad = 0;
    for failure in [
        None,
        Some((c.form.count() - 1, Failure::Host)),
        Some((c.form.count() - 1, Failure::Boundary)),
    ] {
        bad += u32::from(!run::check(c, failure, 3));
    }
    (3, bad)
}
fn add(total: &mut (u32, u32), row: (u32, u32)) {
    total.0 += row.0;
    total.1 += row.1;
}
fn located(mut c: Case, address: u32) -> Case {
    match c.form.ea {
        Ea::Dn => panic!("register has no operand address"),
        Ea::Indirect | Ea::Post => {
            c.r.set_address_register(usize::from(c.register), address);
        }
        Ea::Pre => {
            c.r.set_address_register(
                usize::from(c.register),
                address.wrapping_add(c.form.stride()),
            );
        }
        Ea::Displacement => {
            c.extension = 0xfff0;
            c.r.set_address_register(usize::from(c.register), address.wrapping_add(16));
        }
        Ea::Index => {
            c.extension = 0x60f0;
            c.r.d[6] = 0;
            c.r.set_address_register(usize::from(c.register), address.wrapping_add(16));
        }
        Ea::AbsoluteWord => c.extension = address as u16,
        Ea::AbsoluteLong => {
            c.extension = (address >> 16) as u16;
            c.low = address as u16;
        }
    }
    c
}
pub(super) fn indices() -> (u32, u32) {
    let mut total = (0, 0);
    for size in [Size::Word, Size::Long] {
        for supervisor in [false, true] {
            for register in [0u16, 7] {
                for selector in 0..16u16 {
                    for wide in [false, true] {
                        for negative in [false, true] {
                            for ignored in 0..8u16 {
                                let (sign, mask) = if size == Size::Long {
                                    (0x80000000, u32::MAX)
                                } else {
                                    (0x8000, 65535)
                                };
                                for operands in [
                                    (0, 0),
                                    (1, mask),
                                    (sign, sign),
                                    (if size == Size::Long { 0x10000 } else { 0x8001 }, 0),
                                ] {
                                    let mut c = Case::new(Form {
                                        size,
                                        ea: Ea::Index,
                                    });
                                    c.r = registers(supervisor, 31);
                                    c.register = register;
                                    pair(&mut c, operands);
                                    let raw = if selector >= 8 {
                                        c.r.address_register(usize::from(selector & 7)).unwrap()
                                    } else {
                                        c.r.d[usize::from(selector)]
                                    };
                                    let displacement = if negative { -2i8 } else { 2 };
                                    c.extension = selector << 12
                                        | if wide { 0x800 } else { 0 }
                                        | ignored << 8
                                        | u16::from(displacement as u8);
                                    let index = if wide {
                                        raw
                                    } else {
                                        i32::from(raw as i16) as u32
                                    };
                                    let expected =
                                        c.r.address_register(usize::from(register))
                                            .unwrap()
                                            .wrapping_add(index)
                                            .wrapping_add_signed(i32::from(displacement));
                                    let (_, bus) = c.seed();
                                    let proof = oracle::address(c, &bus.words) == expected
                                        && expected & 1 == 0
                                        && oracle::value(c, &bus.words) == operands.0;
                                    let row = paths(c);
                                    add(&mut total, (row.0, row.1 + u32::from(!proof)));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 49152))
}
pub(super) fn mutations() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..form.count() {
                for failure in [
                    None,
                    Some((form.count() - 1, Failure::Host)),
                    Some((form.count() - 1, Failure::Boundary)),
                ] {
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, 31);
                    c.register = 7;
                    let (cpu, mut bus) = c.seed();
                    let plan = oracle::plan(c, &bus.words);
                    let t = plan[at];
                    let value = if t.phase == InstructionPhase::OperandRefill {
                        0x7680
                    } else if t.access.kind == AccessKind::Prefetch {
                        0x7600
                    } else {
                        0x8002
                    };
                    let mut changed = bus.words.clone();
                    changed.insert(t.access.bus_address, value);
                    let ea = oracle::address(c, &changed);
                    if ea != oracle::address(c, &bus.words) {
                        bus.operand(ea, form.size, 0x80000002);
                    }
                    bus.mutate_read = Some((t.access.logical_address, value));
                    bus.failure = failure;
                    bus.elapsed = [7; 7];
                    total.0 += 1;
                    total.1 += u32::from(!run::seeded(c, cpu, bus));
                }
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 396))
}
pub(super) fn overlaps() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for target in 0..3 {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.r.pc = 0x1000;
                c.register = 0;
                let address = c.r.pc.wrapping_add(match target {
                    0 => {
                        if form.size == Size::Long {
                            4
                        } else {
                            2
                        }
                    }
                    1 => form.length(),
                    _ => form.length() + 2,
                });
                c = located(c, address);
                c.value = if form.size == Size::Long {
                    0x80000000
                } else {
                    0x8000
                };
                if target == 0 && form.size == Size::Long {
                    match form.ea {
                        Ea::Displacement => {
                            c.extension = 0;
                            c.r.set_address_register(0, address);
                        }
                        Ea::Index => {
                            c.extension = 0;
                            c.r.d[0] = 0;
                            c.r.set_address_register(0, address);
                        }
                        Ea::AbsoluteWord => c.value = 0x80001004,
                        _ => {}
                    }
                }
                let (_, bus) = c.seed();
                let proof = oracle::address(c, &bus.words) == address
                    && oracle::value(c, &bus.words) == c.value
                    && if form.size == Size::Word {
                        oracle::immediate(c, &bus.words) == c.immediate
                    } else {
                        oracle::immediate(c, &bus.words)
                            == u32::from(c.retained()) << 16 | u32::from(bus.sample(0x1004))
                    };
                let row = paths(c);
                add(&mut total, (row.0, row.1 + u32::from(!proof)));
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 252))
}
pub(super) fn program_wrap() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for pc in [0xfffffffcu32, 0xfffffffe] {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.r.pc = pc;
                c.register = 7;
                add(&mut total, paths(c));
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 168))
}
pub(super) fn operand_wrap() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for address in [0xfffffffeu32, 0x00fffffe] {
            if form.ea == Ea::AbsoluteWord && address == 0x00fffffe {
                continue;
            }
            for supervisor in [false, true] {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.register = 0;
                c.value = if form.size == Size::Long {
                    0x80000001
                } else {
                    0x8001
                };
                c = located(c, address);
                let (_, bus) = c.seed();
                let proof = oracle::address(c, &bus.words) == address
                    && oracle::value(c, &bus.words) == c.value;
                let row = paths(c);
                add(&mut total, (row.0, row.1 + u32::from(!proof)));
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 156))
}
pub(super) fn secondary() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..11 {
                for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, 31);
                    c.register = 7;
                    let (cpu, mut bus) = c.seed();
                    bus.failure = Some((form.count() - 1, Failure::Boundary));
                    bus.entry_failure = Some((at, kind));
                    total.0 += 1;
                    total.1 += u32::from(!run::seeded(c, cpu, bus));
                }
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 924))
}
pub(super) fn odd_ssp() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for report in [
                None,
                Some((Failure::Complete, 7)),
                Some((Failure::Host, 7)),
                Some((Failure::Unavailable, 7)),
                Some((Failure::Boundary, 7)),
            ] {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.r.ssp |= 1;
                c.register = 0;
                let (cpu, mut bus) = c.seed();
                bus.failure = Some((form.count() - 1, Failure::Boundary));
                total.0 += 1;
                total.1 += u32::from(!inputs(c) || !run::seeded_custom(c, cpu, bus, report));
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 140))
}
