use super::*;
use bulk_device::Kind;
use bulk_matrix::Count;
use bulk_model::{DETECTORS, ENTRY_REPORTS, SOURCES, triggers};

pub(super) fn failures() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        for entry in 0..11 {
            for (kind, elapsed) in ENTRY_REPORTS {
                let (_, mut b) = s.seed();
                b.report = Some((at, Kind::Boundary, 8));
                b.entry_failure = Some((entry, kind, elapsed));
                total.case(s, b);
            }
        }
        for entry in 0..7 {
            for (kind, elapsed) in ENTRY_REPORTS {
                let (_, mut b) = s.seed();
                b.report = Some((at, Kind::Boundary, 8));
                b.entry_failure = Some((entry, kind, elapsed));
                b.entry_write_deposit = false;
                total.case(s, b);
            }
        }
    }
    total.result()
}
pub(super) fn controls() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        let (_, mut b) = s.seed();
        b.report = Some((at, Kind::Boundary, 8));
        total.case(s, b);
    }
    total.result()
}
pub(super) fn secondary() -> (u32, u32) {
    let mut total = Count::default();
    for (base, at) in triggers() {
        for source in SOURCES {
            for route in 0..2 {
                let addresses = if route == 0 {
                    [0x9001, 0xffff_fffd]
                } else {
                    [0x4001, 0x00ff_ffff]
                };
                for address in addresses {
                    for (detector, &report) in DETECTORS.iter().enumerate() {
                        let mut s = base;
                        s.value(source);
                        if route == 0 {
                            s.r.ssp = address;
                        }
                        let (_, mut b) = s.seed();
                        b.report = Some((at, Kind::Boundary, 8));
                        if route == 1 {
                            b.word(8, (address >> 16) as u16);
                            b.word(10, address as u16);
                        }
                        if detector == 0 {
                            b.default_entry_detector = true;
                        } else {
                            b.entry_detector = Some(report);
                        }
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn wraps() -> (u32, u32) {
    let mut total = Count::default();
    let (mut redirected, mut odd) = (0, 0);
    for (base, at) in triggers() {
        for sp in [0, 2, 14, 0xffff_fff0, 0x00ff_fff0, 0x8000_0010] {
            for handler in [
                0x4000,
                0xffff_fffc,
                0xffff_fffe,
                0x00ff_fffc,
                0x00ff_fffe,
                0,
            ] {
                for source in [0, 0xffff] {
                    for wait in [4, 7] {
                        let mut s = base;
                        s.r.ssp = sp;
                        s.value(source);
                        s.wait = wait;
                        let (_, mut b) = s.seed();
                        b.word(8, (handler >> 16) as u16);
                        b.word(10, handler as u16);
                        b.word(handler, 0x4e71);
                        b.word(handler.wrapping_add(2), 0x7002);
                        b.report = Some((at, Kind::Boundary, 8));
                        b.default_entry_detector = true;
                        let p = bulk_plan::plan(s, &b);
                        let e = bulk_oracle::predict(s, &b, &p);
                        let target = u32::from(e.words.get(&8).copied().unwrap_or(0)) << 16
                            | u32::from(e.words.get(&10).copied().unwrap_or(0));
                        redirected += u32::from(target != handler);
                        odd += u32::from(target & 1 != 0);
                        total.case(s, b);
                    }
                }
            }
        }
    }
    let (count, bad) = total.result();
    (count, bad + u32::from((redirected, odd) != (2700, 720)))
}
