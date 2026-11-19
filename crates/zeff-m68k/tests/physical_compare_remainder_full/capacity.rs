use super::model::{Case, Failure};
use super::{Tally, check, seed};

pub(super) fn prefix(c: Case, at: usize) -> u64 {
    u64::from(c.p.idle()) * 2 + at as u64 * 4
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    seed::each(|base| match index {
        4 => {
            for at in 0..base.p.count() {
                for ccr in [0, 31] {
                    let mut c = seed::status(base, ccr);
                    c.start = u64::MAX - prefix(c, at) - (u64::from(u32::MAX) - 1);
                    tally.add(check::run(c, |_| {}, mark));
                }
            }
        }
        5 if base.p.idle() => {
            for room in 0..=2 {
                let mut c = base;
                c.start = u64::MAX - room;
                tally.add(check::run(c, |_| {}, mark));
            }
        }
        6 if base.p.tail() => {
            let at = base.p.count() - 1;
            for room in 0..=2 {
                let mut c = base;
                c.start = u64::MAX - prefix(c, at) - u64::from(u32::MAX) - room;
                tally.add(check::run(
                    c,
                    |b| b.fault(at, Failure::Complete, u32::MAX),
                    mark,
                ));
            }
        }
        17 => {
            for at in 0..base.p.count() {
                let mut c = base;
                c.start = u64::MAX - prefix(c, at) - u64::from(u32::MAX);
                for kind in [
                    Failure::Complete,
                    Failure::Host,
                    Failure::Unavailable,
                    Failure::Boundary,
                ] {
                    tally.add(check::run(c, |b| b.fault(at, kind, u32::MAX), mark));
                }
            }
        }
        18 => {
            for at in 0..base.p.count() {
                for elapsed in [0, 1, 3] {
                    tally.add(check::run(
                        base,
                        |b| b.fault(at, Failure::Boundary, elapsed),
                        mark,
                    ));
                }
            }
        }
        5 | 6 => {}
        _ => panic!("compare capacity section"),
    });
    tally.finish()
}
