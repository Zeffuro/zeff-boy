use super::*;
pub(super) const B: u64 = u32::MAX as u64;
pub(super) fn prefix(c: Case, at: usize) -> u64 {
    at as u64 * 4 + u64::from(c.form.idle() && at >= c.form.immediates()) * 2
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut t = Tally::default();
    model::each(|base| {
        if index == 3 {
            for at in 0..base.form.count() {
                for ccr in [0, 31] {
                    let mut c = model::status(base, ccr);
                    c.start = u64::MAX - B + 1 - prefix(c, at);
                    t.add(check::run(c, |_| {}, mark));
                }
            }
        } else if index == 4 && base.form.idle() {
            for room in [1, 2] {
                let mut c = base;
                c.start = u64::MAX - B - room;
                t.add(check::run(c, |b| b.elapsed[0] = u32::MAX, mark));
            }
        } else if index == 5 && base.form.tail() {
            for room in 0..=2 {
                let mut c = base;
                c.start = u64::MAX - B - 8 - room;
                t.add(check::run(c, |b| b.elapsed[2] = u32::MAX, mark));
            }
        } else if index == 16 {
            for at in 0..base.form.count() {
                for kind in [
                    Failure::Complete,
                    Failure::Host,
                    Failure::Unavailable,
                    Failure::Boundary,
                ] {
                    let mut c = base;
                    c.start = u64::MAX - B - prefix(c, at);
                    t.add(check::run(
                        c,
                        |b| {
                            b.failure = Some((at, kind));
                            b.elapsed[at] = u32::MAX;
                        },
                        mark,
                    ));
                }
            }
        } else if index == 17 {
            for at in 0..base.form.count() {
                for n in [0, 1, 3] {
                    t.add(check::run(
                        base,
                        |b| {
                            b.failure = Some((at, Failure::Boundary));
                            b.elapsed[at] = n;
                        },
                        mark,
                    ));
                }
            }
        } else if index == 18 {
            for at in 0..base.form.count() {
                for blocked in 0..11 {
                    let mut c = base;
                    c.start = u64::MAX - B + 1
                        - prefix(c, at)
                        - 8
                        - blocked as u64 * 4
                        - u64::from(blocked == 10) * 2;
                    t.add(check::run(
                        c,
                        |b| b.failure = Some((at, Failure::Boundary)),
                        mark,
                    ));
                }
            }
        } else if index == 19 {
            for at in 0..base.form.count() {
                for room in 0..=4 {
                    let mut c = base;
                    c.start = u64::MAX - B - prefix(c, at) - room;
                    t.add(check::run(
                        c,
                        |b| {
                            b.failure = Some((at, Failure::Boundary));
                            b.elapsed[at] = u32::MAX;
                        },
                        mark,
                    ));
                }
            }
        }
    });
    t.finish()
}
