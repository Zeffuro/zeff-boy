use super::*;
use fixture::{Case, OPS, STATUSES};

pub(super) fn completion() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for source in 0..if ea <= 6 { 8 } else { 1 } {
                    for destination in 0..8 {
                        for supervisor in [false, true] {
                            for status in STATUSES {
                                for (value, desired) in fixture::pairs(long) {
                                    for wait in [0, 3] {
                                        let c = Case {
                                            operation,
                                            long,
                                            ea,
                                            source,
                                            destination,
                                            supervisor,
                                            status,
                                            value,
                                            desired,
                                            wait,
                                            ..Case::default()
                                        };
                                        count += 1;
                                        bad += u32::from(!run::check(c));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 540672))
}
pub(super) fn faults() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for source in 0..if ea <= 6 { 8 } else { 1 } {
                    for destination in 0..8 {
                        let c = Case {
                            operation,
                            long,
                            ea,
                            source,
                            destination,
                            wait: 3,
                            ..Case::default()
                        };
                        for at in 0..c.count() {
                            for supervisor in [false, true] {
                                for status in STATUSES {
                                    for (value, desired) in fixture::pairs(long) {
                                        count += 1;
                                        bad += u32::from(!run::check(Case {
                                            supervisor,
                                            status,
                                            value,
                                            desired,
                                            failure: Some(at),
                                            ..c
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 804864))
}
pub(super) fn all_ccr() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for operation in OPS {
        for long in [false, true] {
            for ea in 2..=10 {
                for supervisor in [false, true] {
                    for ccr in 0..32 {
                        for (value, desired) in fixture::pairs(long) {
                            let c = Case {
                                operation,
                                long,
                                ea,
                                supervisor,
                                status: 0x0700 | ccr,
                                value,
                                desired,
                                wait: 3,
                                ..Case::default()
                            };
                            for failure in [None, Some(c.count() - 1)] {
                                count += 1;
                                bad += u32::from(!run::check(Case { failure, ..c }));
                            }
                        }
                    }
                }
            }
        }
    }
    (count, bad + u32::from(count != 110592))
}
