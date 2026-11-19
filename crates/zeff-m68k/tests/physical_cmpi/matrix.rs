use super::*;

pub(super) fn completion() -> (u32, u32) {
    let (mut count, mut bad, mut enc) = (0, 0, 0);
    for form in forms() {
        for register in 0..if form.variable() { 8 } else { 1 } {
            enc += 1;
            for supervisor in [false, true] {
                for status in STATUSES {
                    for operands in pairs(form.size) {
                        for wait in [0, 3] {
                            let mut c = Case::new(form);
                            c.r = registers(supervisor, status);
                            c.register = register;
                            pair(&mut c, operands);
                            count += 1;
                            bad += u32::from(!inputs(c) || !run::check(c, None, wait));
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(enc != 84 || count != 16128))
}
pub(super) fn faults() -> (u32, u32) {
    let (mut count, mut bad, mut positions) = (0, 0, 0);
    for form in forms() {
        for register in 0..if form.variable() { 8 } else { 1 } {
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
                                let mut c = Case::new(form);
                                c.r = registers(supervisor, status);
                                c.register = register;
                                pair(&mut c, operands);
                                count += 1;
                                bad +=
                                    u32::from(!inputs(c) || !run::check(c, Some((at, failure)), 3));
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(positions != 374 || count != 143616))
}
pub(super) fn odd() -> (u32, u32) {
    let reports = [
        None,
        Some((Failure::Complete, 17)),
        Some((Failure::Host, 17)),
        Some((Failure::Unavailable, 17)),
        Some((Failure::Boundary, 0)),
        Some((Failure::Boundary, 3)),
        Some((Failure::Boundary, 17)),
    ];
    let (mut count, mut bad, mut enc) = (0, 0, 0);
    for form in forms() {
        for register in 0..if form.variable() { 8 } else { 1 } {
            enc += 1;
            for supervisor in [false, true] {
                for status in STATUSES {
                    for operands in pairs(form.size) {
                        for report in reports {
                            let mut c = Case::new(form);
                            c.r = registers(supervisor, status);
                            c.register = register;
                            pair(&mut c, operands);
                            count += 1;
                            bad += u32::from(!detection::check(fixture::odd(c), report));
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(enc != 84 || count != 56448))
}
pub(super) fn all_ccr() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for operands in pairs(form.size) {
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, ccr);
                    c.register = 7;
                    pair(&mut c, operands);
                    for failure in [
                        None,
                        Some((form.count() - 1, Failure::Host)),
                        Some((form.count() - 1, Failure::Boundary)),
                    ] {
                        count += 1;
                        bad += u32::from(!inputs(c) || !run::check(c, failure, 3));
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 32256))
}
