use super::*;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut t = Tally::default();
    model::each(|base| match index {
        0 => {
            for ccr in 0..32 {
                for p in model::values(base.form.size) {
                    for wait in [0, 3, 11] {
                        let c = model::status(model::pair(base, p, 0xff00), ccr);
                        t.add(check::run(c, |b| b.elapsed = [4 + wait; 7], mark));
                    }
                }
            }
        }
        1 => {
            for at in 0..base.form.count() {
                for ccr in [0, 15, 16, 31] {
                    for p in [
                        model::values(base.form.size)[0],
                        model::values(base.form.size)[8],
                    ] {
                        for (kind, elapsed) in [
                            (Failure::Host, 6),
                            (Failure::Boundary, 8),
                            (Failure::Unavailable, 5),
                            (Failure::Host, 4),
                            (Failure::Boundary, 4),
                            (Failure::Unavailable, 4),
                        ] {
                            let c = model::status(model::pair(base, p, 0xa500), ccr);
                            t.add(check::run(
                                c,
                                |b| {
                                    b.failure = Some((at, kind));
                                    b.elapsed[at] = elapsed;
                                },
                                mark,
                            ));
                        }
                    }
                }
            }
        }
        _ => {
            for at in 0..base.form.count() {
                for elapsed in [0, 1, 3] {
                    t.add(check::run(
                        base,
                        |b| {
                            b.failure = Some((at, Failure::Complete));
                            b.elapsed[at] = elapsed;
                        },
                        mark,
                    ));
                }
            }
        }
    });
    t.finish()
}
