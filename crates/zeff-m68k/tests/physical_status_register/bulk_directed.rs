use super::*;
use bulk_device::Kind;
use bulk_matrix::Count;
use bulk_model::{SOURCES, encoded, triggers};

pub(super) const PCS: [u32; 6] = [
    0x2000,
    0xffff_fffc,
    0xffff_fffe,
    0x00ff_fffc,
    0x00ff_fffe,
    0,
];
pub(super) fn pc_wraps() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for pc in PCS {
            for wait in [4, 7] {
                for p in 0..4 {
                    let mut s = base;
                    s.r.pc = pc;
                    s.wait = wait;
                    let (_, mut b) = s.seed();
                    b.report = match p {
                        0 => None,
                        1 => Some((0, Kind::Host, 6)),
                        2 => Some((usize::from(s.immediate), Kind::Boundary, 8)),
                        3 => Some((s.count() - 1, Kind::Boundary, 8)),
                        _ => unreachable!(),
                    };
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn mutations() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        let phase_ssp = s.r.ssp;
        for address in [
            s.r.pc,
            s.r.pc.wrapping_add(2),
            s.anchor(),
            s.anchor().wrapping_add(2),
            phase_ssp.wrapping_sub(2),
            phase_ssp.wrapping_sub(14),
        ] {
            for value in [0x7002, 0xffff] {
                for (kind, elapsed) in [
                    (Kind::Complete, 4),
                    (Kind::Host, 6),
                    (Kind::Boundary, 8),
                    (Kind::Unavailable, 5),
                ] {
                    let (_, mut b) = s.seed();
                    b.report = Some((at, kind, elapsed));
                    b.mutation = Some((at, address, value));
                    total.case(s, b);
                }
            }
        }
    }
    total.result()
}
pub(super) fn overlaps() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for sp in [
            base.r.pc.wrapping_add(14),
            base.r.pc.wrapping_add(16),
            base.anchor().wrapping_add(14),
            base.anchor().wrapping_add(16),
            base.anchor().wrapping_add(12),
            0x9000,
            0xfffffff0,
            0x01000010,
        ] {
            for p in 0..4 {
                let mut s = base;
                s.r.ssp = sp;
                let (_, mut b) = s.seed();
                b.default_entry_detector = true;
                b.report = match p {
                    0 => None,
                    1 => Some((0, Kind::Boundary, 8)),
                    2 => Some((usize::from(s.immediate), Kind::Boundary, 8)),
                    3 => Some((s.count() - 1, Kind::Boundary, 8)),
                    _ => unreachable!(),
                };
                total.case(s, b);
            }
        }
    }
    total.result()
}
pub(super) fn retained() -> (u32, u32) {
    let mut total = Count::default();
    for (base, at) in triggers() {
        for first in [
            0x44c0, 0x44fc, 0x46c0, 0x46fc, 0x44d0, 0x46d0, 0x44c8, 0xffff,
        ] {
            for (kind, elapsed) in [(Kind::Host, 6), (Kind::Boundary, 8), (Kind::Unavailable, 5)] {
                let mut s = base;
                s.first = first;
                let (_, mut b) = s.seed();
                b.report = Some((at, kind, elapsed));
                total.case(s, b);
            }
        }
    }
    total.result()
}
pub(super) fn bank_switches() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| s.family == 0x46c0) {
        for supervisor in [0, 0x2000] {
            for trace in [0, 0x8000] {
                for mask in 0..8 {
                    for parity in 0..2 {
                        for p in 0..4 {
                            let mut s = base;
                            s.r.usp = 0x6000;
                            s.r.ssp = 0x9000 | parity;
                            s.value(supervisor | trace | mask << 8 | 31);
                            let (_, mut b) = s.seed();
                            b.default_entry_detector = true;
                            b.report = match p {
                                0 => None,
                                1 => Some((usize::from(s.immediate), Kind::Boundary, 8)),
                                2 => Some((s.count() - 1, Kind::Boundary, 8)),
                                3 => Some((s.count() - 1, Kind::Host, 6)),
                                _ => unreachable!(),
                            };
                            total.case(s, b);
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn exclusions() -> (u32, u32) {
    let mut total = Count::default();
    for base in bulk_model::all_banks() {
        for ccr in 0..32 {
            for mask in 0..8 {
                let mut s = base;
                s.r = bulk_model::status(
                    s.r,
                    (s.r.status_register() & 0x2000) | 0x8000 | mask << 8 | ccr,
                );
                let (_, b) = s.seed();
                total.case(s, b);
            }
        }
    }
    for base in encoded().into_iter().filter(|s| s.family == 0x46c0) {
        for ccr in 0..32 {
            for mask in 0..8 {
                for source in SOURCES {
                    let mut s = base;
                    s.r = bulk_model::status(s.r, mask << 8 | ccr);
                    s.value(source);
                    let (_, b) = s.seed();
                    total.case(s, b);
                }
            }
        }
    }
    for base in [bulk_model::Spec::new(18), bulk_model::Spec::new(26)] {
        for source in 0..=u16::MAX {
            let mut s = base;
            s.r = bulk_model::status(s.r, s.r.status_register() & !0x2000);
            s.value(source);
            let (_, b) = s.seed();
            total.case(s, b);
        }
    }
    total.result()
}
