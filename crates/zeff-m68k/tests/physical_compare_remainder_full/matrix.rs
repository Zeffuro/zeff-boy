use super::model::Failure;
use super::{Tally, check, seed};

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    seed::each(|base| match index {
        0 | 1 => {
            let (pairs, count) = seed::pairs(base);
            for ccr in 0..32 {
                for pair in pairs.into_iter().take(count) {
                    let c = seed::pair(seed::status(base, ccr), pair);
                    if index == 0 {
                        tally.add(check::run(c, |_| {}, mark));
                    } else {
                        for at in 0..c.p.count() {
                            for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                                tally.add(check::run(c, |b| b.fault(at, kind, 4), mark));
                            }
                        }
                    }
                }
            }
        }
        2 => {
            let (pairs, _) = seed::pairs(base);
            for ccr in [0, 15, 16, 31] {
                for pair in [pairs[0], pairs[4]] {
                    let c = seed::pair(seed::status(base, ccr), pair);
                    for at in 0..c.p.count() {
                        for kind in [
                            Failure::Complete,
                            Failure::Host,
                            Failure::Boundary,
                            Failure::Unavailable,
                        ] {
                            for waits in [[7, 11, 5, 13], [15, 9, 17, 7]] {
                                tally.add(check::run(
                                    c,
                                    |b| {
                                        for (report, elapsed) in b.primary.iter_mut().zip(waits) {
                                            report.elapsed = elapsed;
                                        }
                                        b.fault(at, kind, waits[at]);
                                    },
                                    mark,
                                ));
                            }
                        }
                    }
                }
            }
        }
        3 => {
            for at in 0..base.p.count() {
                for elapsed in [0, 1, 3] {
                    tally.add(check::run(
                        base,
                        |b| b.fault(at, Failure::Complete, elapsed),
                        mark,
                    ));
                }
            }
        }
        _ => panic!("compare arithmetic section"),
    });
    tally.finish()
}
