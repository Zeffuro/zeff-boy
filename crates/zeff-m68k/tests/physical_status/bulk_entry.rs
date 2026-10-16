use super::*;
use device::Kind;

pub(super) fn secondary() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::publications() {
        for fault in [s.count() - 2, s.count() - 1] {
            for at in 0..11 {
                for kind in [Kind::Host, Kind::Boundary, Kind::Unavailable] {
                    let (_, mut bus) = s.seed();
                    bus.report = Some((fault, Kind::Boundary, 7));
                    bus.entry_failure = Some((at, kind, 7));
                    count += 1;
                    bad += u32::from(!oracle::run(s, bus));
                }
            }
        }
    }
    (count, bad)
}
pub(super) fn odd_ssp() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::publications() {
        for fault in [s.count() - 2, s.count() - 1] {
            for detector in 0..5 {
                s.r.ssp = 0x80001;
                let (_, mut bus) = s.seed();
                bus.report = Some((fault, Kind::Boundary, 7));
                bus.default_entry_detector = detector == 0;
                bus.entry_detector = match detector {
                    0 => None,
                    1 => Some((Kind::Complete, 7)),
                    2 => Some((Kind::Host, 7)),
                    3 => Some((Kind::Unavailable, 7)),
                    _ => Some((Kind::Boundary, 7)),
                };
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}
