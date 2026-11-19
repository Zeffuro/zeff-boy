use super::*;

fn paths(case: Case) -> (u32, u32) {
    let mut bad = 0;
    for failure in [
        None,
        Some((case.form.count() - 1, Failure::Host)),
        Some((case.form.count() - 1, Failure::Boundary)),
    ] {
        bad += u32::from(!inputs(case) || !run::check(case, failure, 3));
    }
    (3, bad)
}
fn add(total: &mut (u32, u32), row: (u32, u32)) {
    total.0 += row.0;
    total.1 += row.1;
}
fn located(mut case: Case, address: u32) -> Case {
    match case.form.ea {
        Ea::Indirect | Ea::Post => {
            case.r
                .set_address_register(usize::from(case.register), address);
        }
        Ea::Pre => {
            case.r.set_address_register(
                usize::from(case.register),
                address.wrapping_add(case.form.stride()),
            );
        }
        Ea::Displacement => {
            case.extension = 0xfff0;
            case.r
                .set_address_register(usize::from(case.register), address.wrapping_add(16));
        }
        Ea::Index => {
            case.extension = 0x60f0;
            case.r.d[6] = 0;
            case.r
                .set_address_register(usize::from(case.register), address.wrapping_add(16));
        }
        Ea::AbsoluteWord => case.extension = address as u16,
        Ea::AbsoluteLong => {
            case.extension = (address >> 16) as u16;
            case.low = address as u16;
        }
        Ea::PcDisplacement => {
            case.extension = address.wrapping_sub(case.r.pc.wrapping_add(2)) as u16
        }
        Ea::PcIndex => {
            case.extension = 0x68f0;
            case.r.d[6] = address
                .wrapping_sub(case.r.pc.wrapping_add(2))
                .wrapping_add(16);
        }
    }
    case
}

pub(super) fn indices() -> (u32, u32) {
    let mut total = (0, 0);
    for op in OPS {
        for size in [Size::Word, Size::Long] {
            for ea in [Ea::Index, Ea::PcIndex] {
                for supervisor in [false, true] {
                    for selector in 0..16u16 {
                        for wide in [false, true] {
                            for negative in [false, true] {
                                for ignored in 0..8u16 {
                                    let (sign, mask) = if size == Size::Long {
                                        (0x8000_0000, u32::MAX)
                                    } else {
                                        (0x8000, 65535)
                                    };
                                    for operands in [
                                        (0, 0),
                                        (1, mask),
                                        (sign, sign),
                                        (if size == Size::Long { 0x10000 } else { 0x8001 }, 0),
                                    ] {
                                        let mut case = Case::new(Form { op, size, ea });
                                        case.r = registers(supervisor, 31);
                                        case.register = 7;
                                        case.destination = selector & 7;
                                        pair(&mut case, operands);
                                        let raw = if selector >= 8 {
                                            case.r
                                                .address_register(usize::from(selector & 7))
                                                .unwrap()
                                        } else {
                                            case.r.d[usize::from(selector)]
                                        };
                                        let displacement = if negative {
                                            if raw & 1 == 0 { -2i8 } else { -3 }
                                        } else if raw & 1 == 0 {
                                            2
                                        } else {
                                            3
                                        };
                                        case.extension = (selector << 12)
                                            | if wide { 0x800 } else { 0 }
                                            | ignored << 8
                                            | u16::from(displacement as u8);
                                        let (_, bus) = case.seed();
                                        let index = if wide {
                                            raw
                                        } else {
                                            i32::from(raw as i16) as u32
                                        };
                                        let base = if ea == Ea::PcIndex {
                                            case.r.pc.wrapping_add(2)
                                        } else {
                                            case.r.address_register(7).unwrap()
                                        };
                                        let address = base
                                            .wrapping_add(index)
                                            .wrapping_add_signed(i32::from(displacement));
                                        let proof = oracle::address(case, &bus.words) == address
                                            && address & 1 == 0
                                            && oracle::value(case, &bus.words) == operands.0;
                                        let row = paths(case);
                                        add(&mut total, (row.0, row.1 + u32::from(!proof)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    total.1 += u32::from(total.0 != 196608);
    total
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
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    case.value = 0x8000_0001;
                    let (cpu, mut bus) = case.seed();
                    let plan = oracle::plan(case, &bus.words);
                    let t = plan[at];
                    let value = if form.ea == Ea::AbsoluteLong && at == 0 {
                        0x6020
                    } else if t.phase == InstructionPhase::OperandRefill {
                        0x7680
                    } else if t.access.kind == AccessKind::Prefetch {
                        0x7601
                    } else {
                        0x8002
                    };
                    if form.ea == Ea::AbsoluteLong && at == 0 {
                        bus.operand(
                            u32::from(case.extension) << 16 | u32::from(value),
                            form.size,
                            0x8000_0002,
                        );
                    }
                    bus.mutate_read = Some((t.access.logical_address, value));
                    bus.failure = failure;
                    bus.elapsed = [7; 5];
                    total.0 += 1;
                    total.1 += u32::from(!run::seeded(case, cpu, bus));
                }
            }
        }
    }
    total.1 += u32::from(total.0 != 1416);
    total
}

pub(super) fn overlaps() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for final_word in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.r.pc = 0x1000;
                case.register = 0;
                case.value = if form.size == Size::Long {
                    0x8000_0002
                } else {
                    0x8000
                };
                let address = case
                    .r
                    .pc
                    .wrapping_add(form.length() + u32::from(final_word) * 2);
                case = located(case, address);
                let (_, bus) = case.seed();
                let proof = oracle::address(case, &bus.words) == address
                    && oracle::value(case, &bus.words) == case.value;
                let row = paths(case);
                add(&mut total, (row.0, row.1 + u32::from(!proof)));
            }
        }
    }
    total.1 += u32::from(total.0 != 864);
    total
}

pub(super) fn program_wrap() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for pc in [0xffff_fffc, 0xffff_fffe] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.r.pc = pc;
                case.register = 7;
                case.value = if form.size == Size::Long {
                    0x8000_0001
                } else {
                    0x8001
                };
                add(&mut total, paths(case));
            }
        }
    }
    total.1 += u32::from(total.0 != 864);
    total
}

pub(super) fn operand_wrap() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for address in [0xffff_fffeu32, 0x00ff_fffe] {
            if form.ea == Ea::AbsoluteWord && address == 0x00ff_fffe {
                continue;
            }
            for supervisor in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 0;
                case.value = if form.size == Size::Long {
                    0x8000_0001
                } else {
                    0x8001
                };
                if form.ea == Ea::PcDisplacement {
                    case.r.pc = address.wrapping_sub(2);
                }
                case = located(case, address);
                let (_, bus) = case.seed();
                let proof = oracle::address(case, &bus.words) == address
                    && oracle::value(case, &bus.words) == case.value;
                let row = paths(case);
                add(&mut total, (row.0, row.1 + u32::from(!proof)));
            }
        }
    }
    total.1 += u32::from(total.0 != 816);
    total
}

pub(super) fn secondary() -> (u32, u32) {
    let mut total = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..11 {
                for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    let (cpu, mut bus) = case.seed();
                    bus.failure = Some((form.count() - 1, Failure::Boundary));
                    bus.entry_failure = Some((at, kind));
                    total.0 += 1;
                    total.1 += u32::from(!run::seeded(case, cpu, bus));
                }
            }
        }
    }
    total.1 += u32::from(total.0 != 4752);
    total
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
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.r.ssp |= 1;
                case.register = 0;
                let (cpu, mut bus) = case.seed();
                bus.failure = Some((form.count() - 1, Failure::Boundary));
                total.0 += 1;
                total.1 += u32::from(!inputs(case) || !run::seeded_custom(case, cpu, bus, report));
            }
        }
    }
    total.1 += u32::from(total.0 != 720);
    total
}
