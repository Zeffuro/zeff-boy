use super::*;

pub(super) fn completion() -> (u32, u32) {
    let (mut count, mut bad, mut enc) = (0, 0, 0);
    for form in forms() {
        for source in 0..if form.variable() { 8 } else { 1 } {
            for destination in 0..8 {
                enc += 1;
                for supervisor in [false, true] {
                    for status in STATUSES {
                        for operands in pairs(form.size) {
                            for wait in [0, 3] {
                                let mut case = Case::new(form);
                                case.r = registers(supervisor, status);
                                case.register = source;
                                case.destination = destination;
                                pair(&mut case, operands);
                                count += 1;
                                bad += u32::from(!inputs(case) || !run::check(case, None, wait));
                            }
                        }
                    }
                }
            }
        }
    }
    bad += u32::from(enc != 2816 || count != 540672);
    (count, bad)
}

pub(super) fn faults(encoded: bool) -> (u32, u32) {
    let (mut count, mut bad, mut positions) = (0, 0, 0);
    for form in forms() {
        for source in 0..if encoded && form.variable() { 8 } else { 1 } {
            for destination in 0..if encoded { 8 } else { 1 } {
                for at in 0..form.count() {
                    positions += 1;
                    for supervisor in [false, true] {
                        for status in STATUSES {
                            for operands in pairs(form.size) {
                                for failure in [
                                    Failure::Host,
                                    Failure::Boundary,
                                    Failure::Unavailable,
                                    Failure::Complete,
                                ] {
                                    let mut case = Case::new(form);
                                    case.r = registers(supervisor, status);
                                    case.register = if encoded { source } else { 7 };
                                    case.destination = if encoded { destination } else { 5 };
                                    pair(&mut case, operands);
                                    count += 1;
                                    bad += u32::from(
                                        !inputs(case) || !run::check(case, Some((at, failure)), 3),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let expected = if encoded {
        (8384, 3219456)
    } else {
        (236, 90624)
    };
    bad += u32::from((positions, count) != expected);
    (count, bad)
}

pub(super) fn odd(encoded: bool) -> (u32, u32) {
    let reports = [
        None,
        Some((Failure::Complete, 17)),
        Some((Failure::Host, 17)),
        Some((Failure::Unavailable, 17)),
        Some((Failure::Boundary, 17)),
        Some((Failure::Boundary, 0)),
        Some((Failure::Boundary, 3)),
    ];
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for status in STATUSES {
                for operands in pairs(form.size) {
                    for report in reports.into_iter().take(if encoded { 7 } else { 5 }) {
                        let mut case = Case::new(form);
                        case.r = registers(supervisor, status);
                        case.register = 7;
                        pair(&mut case, operands);
                        count += 1;
                        bad += u32::from(!detection::check(fixture::odd(case), report));
                    }
                }
            }
        }
    }
    bad += u32::from(count != if encoded { 48384 } else { 34560 });
    (count, bad)
}

pub(super) fn all_ccr() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for operands in pairs(form.size) {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, ccr);
                    case.register = 7;
                    case.destination = 0;
                    pair(&mut case, operands);
                    for failure in [
                        None,
                        Some((form.count() - 1, Failure::Host)),
                        Some((form.count() - 1, Failure::Boundary)),
                    ] {
                        count += 1;
                        bad += u32::from(!inputs(case) || !run::check(case, failure, 3));
                    }
                }
            }
        }
    }
    bad += u32::from(count != 165888);
    (count, bad)
}
