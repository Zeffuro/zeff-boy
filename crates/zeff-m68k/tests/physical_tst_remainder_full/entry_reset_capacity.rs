use super::*;
use capacity::B;
use device::Kind;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        28 => {
            for (mut s, primary) in model::triggers() {
                for at in 0..11 {
                    for room in [B - 1, B, B + 1] {
                        s.start = u64::MAX
                            - s.idle()
                            - primary as u64 * 4
                            - 8
                            - 4
                            - at as u64 * 4
                            - if at == 10 { 2 } else { 0 }
                            - room;
                        tally.add(check::run(
                            s,
                            |b| b.report = Some((primary, Kind::Boundary, 8)),
                            mark,
                        ));
                    }
                }
            }
            for s in model::encoded() {
                for state in 0..5 {
                    for at in 0..6 {
                        for room in [B - 1, B, B + 1] {
                            let q = reset::Request {
                                room: Some((0, at, room)),
                                ..Default::default()
                            };
                            tally.add(reset::run(s, state, q, mark));
                        }
                    }
                }
            }
        }
        29 => {
            for (mut s, primary) in model::triggers() {
                for idle in 0..2 {
                    let n = if idle == 0 { 4 } else { 2 };
                    for room in [n - 1, n, n + 1] {
                        s.start = if idle == 0 {
                            u64::MAX - s.idle() - primary as u64 * 4 - B - room
                        } else {
                            u64::MAX - s.idle() - primary as u64 * 4 - 8 - 4 - 9 * 4 - B - room
                        };
                        tally.add(check::run(
                            s,
                            |b| {
                                b.report = Some((
                                    primary,
                                    Kind::Boundary,
                                    if idle == 0 { u32::MAX } else { 8 },
                                ));
                                if idle == 1 {
                                    b.entry_failure = Some((9, Kind::Complete, u32::MAX));
                                }
                            },
                            mark,
                        ));
                    }
                }
            }
            for s in model::encoded() {
                for state in 0..5 {
                    for idle in 0..2 {
                        let n = if idle == 0 { 14 } else { 2 };
                        for room in [n - 1, n, n + 1] {
                            let q = reset::Request {
                                room: Some((if idle == 0 { 1 } else { 2 }, 0, room)),
                                ..Default::default()
                            };
                            tally.add(reset::run(s, state, q, mark));
                        }
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
