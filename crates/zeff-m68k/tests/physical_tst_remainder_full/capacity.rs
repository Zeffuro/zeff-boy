use super::*;
use device::Kind;
pub(super) const B: u64 = u32::MAX as u64;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        18 => {
            for (mut s, at) in model::triggers() {
                for room in [B - 1, B, B + 1] {
                    s.start = u64::MAX - s.idle() - at as u64 * 4 - room;
                    tally.add(check::run(s, |_| {}, mark));
                }
            }
        }
        19 => {
            for mut s in model::encoded().filter(|s| matches!(s.mode(), 4 | 6)) {
                for room in [0, 1, 2, 3, B - 1, B, B + 1] {
                    s.start = u64::MAX - room;
                    tally.add(check::run(s, |_| {}, mark));
                }
            }
        }
        20 => {
            for (s, at) in model::triggers() {
                for n in 0..4 {
                    for k in [
                        Kind::Complete,
                        Kind::Host,
                        Kind::Boundary,
                        Kind::Unavailable,
                    ] {
                        tally.add(check::run(s, |b| b.report = Some((at, k, n)), mark));
                    }
                }
            }
        }
        21 => {
            for (mut s, at) in model::triggers() {
                s.start = u64::MAX - s.idle() - at as u64 * 4 - B;
                for k in [
                    Kind::Complete,
                    Kind::Host,
                    Kind::Boundary,
                    Kind::Unavailable,
                ] {
                    tally.add(check::run(s, |b| b.report = Some((at, k, u32::MAX)), mark));
                }
            }
        }
        22 => {
            for mut s in model::encoded() {
                let at = s.count() - 1;
                s.start = u64::MAX - s.idle() - at as u64 * 4 - B;
                tally.add(check::run(s, |b| b.elapsed[at] = u32::MAX, mark));
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
