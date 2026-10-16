use super::*;
use bulk_matrix::Count;
use bulk_model::triggers;
use device::Kind;

pub(super) fn failures() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        let (_, b) = s.seed();
        bulk_matrix::validate(s, &b);
        for entry in 0..11 {
            for report in [
                (Kind::Complete, 0),
                (Kind::Complete, 1),
                (Kind::Complete, 3),
                (Kind::Host, 6),
                (Kind::Boundary, 8),
                (Kind::Unavailable, 5),
            ] {
                let mut bus = b.clone();
                bus.report = Some((at, Kind::Boundary, 8));
                bus.entry_failure = Some((entry, report.0, report.1));
                total.case(s, bus);
            }
        }
    }
    total.result()
}
pub(super) fn controls() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        let (_, mut b) = s.seed();
        bulk_matrix::validate(s, &b);
        b.report = Some((at, Kind::Boundary, 8));
        total.case(s, b);
    }
    total.result()
}
pub(super) fn secondary() -> (u32, u32) {
    let mut total = Count::default();
    for (mut s, at) in triggers() {
        for route in 0..2 {
            for address in if route == 0 {
                &[0x9001, 0xffff_fffd][..]
            } else {
                &[0x4001, 0x00ff_ffff][..]
            } {
                for detector in 0..5 {
                    let original = s.r;
                    if route == 0 {
                        s.r.ssp = *address;
                    }
                    let (_, mut b) = s.seed();
                    b.report = Some((at, Kind::Boundary, 8));
                    if route == 1 {
                        b.word(8, (address >> 16) as u16);
                        b.word(10, *address as u16);
                    }
                    if detector == 0 {
                        b.default_entry_detector = true;
                    } else {
                        b.entry_detector = Some(
                            [
                                (Kind::Unavailable, 0),
                                (Kind::Unavailable, 5),
                                (Kind::Host, 6),
                                (Kind::Complete, 0),
                                (Kind::Boundary, 8),
                            ][detector],
                        );
                    }
                    total.case(s, b);
                    s.r = original;
                }
            }
        }
    }
    total.result()
}
