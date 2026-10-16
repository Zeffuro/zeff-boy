use super::*;
use device::Kind;
use model::{Action, Spec};

fn prefix(s: Spec, at: usize, bus: &Device) -> u64 {
    let p = model::plan(s, bus);
    let mut elapsed = 0;
    let mut reads = 0;
    for action in p.actions {
        match action {
            Action::Idle(_) => elapsed += 2,
            Action::Read(_) if reads == at => return elapsed,
            Action::Read(_) => {
                elapsed += u64::from(bus.elapsed[reads]);
                reads += 1;
            }
        }
    }
    elapsed
}
pub(super) fn reservations() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded() {
        for at in 0..s.count() {
            let (_, original) = s.seed();
            let clocks = prefix(s, at, &original);
            s.start = u64::MAX - u64::from(u32::MAX) + 1 - clocks;
            let (_, bus) = s.seed();
            count += 1;
            bad += u32::from(!oracle::run(s, bus));
        }
    }
    (count, bad)
}
pub(super) fn source_idles() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::forms().into_iter().filter(|s| s.idle()) {
        for room in [1, 2] {
            s.start = u64::MAX - room;
            let (_, bus) = s.seed();
            count += 1;
            bad += u32::from(!oracle::run(s, bus));
        }
    }
    (count, bad)
}
pub(super) fn post_idles() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded() {
        for room in 0..5 {
            let operand = s.extensions();
            let (_, original) = s.seed();
            let clocks = prefix(s, operand, &original);
            s.start = u64::MAX - u64::from(u32::MAX) - clocks - room;
            let (_, mut bus) = s.seed();
            bus.elapsed[operand] = u32::MAX;
            count += 1;
            bad += u32::from(!oracle::run(s, bus));
        }
    }
    (count, bad)
}
pub(super) fn shorts(boundary: bool) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::encoded() {
        for at in 0..s.count() {
            for elapsed in 0..4 {
                if boundary && elapsed == 2 {
                    continue;
                }
                let (_, mut bus) = s.seed();
                bus.report = Some((
                    at,
                    if boundary {
                        Kind::Boundary
                    } else {
                        Kind::Complete
                    },
                    elapsed,
                ));
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
pub(super) fn maximum() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded() {
        for at in 0..s.count() {
            for kind in [
                Kind::Complete,
                Kind::Host,
                Kind::Boundary,
                Kind::Unavailable,
            ] {
                let (_, original) = s.seed();
                let clocks = prefix(s, at, &original);
                s.start = u64::MAX - u64::from(u32::MAX) - clocks;
                let (_, mut bus) = s.seed();
                bus.report = Some((at, kind, u32::MAX));
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
pub(super) fn finals() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded() {
        let at = s.count() - 1;
        let (_, original) = s.seed();
        s.start = u64::MAX - u64::from(u32::MAX) - prefix(s, at, &original);
        let (_, mut bus) = s.seed();
        bus.report = Some((at, Kind::Complete, u32::MAX));
        count += 1;
        bad += u32::from(!oracle::run(s, bus));
    }
    (count, bad)
}
