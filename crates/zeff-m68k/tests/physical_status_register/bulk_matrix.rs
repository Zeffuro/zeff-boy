use super::*;
use bulk_device::{Device, Kind};
use bulk_model::{REPORTS, SOURCES, Spec, encoded, representatives, status, triggers};

#[derive(Default)]
pub(super) struct Count {
    pub count: u32,
    pub bad: u32,
}
impl Count {
    pub fn record(&mut self, good: bool) {
        self.count += 1;
        self.bad += u32::from(!good);
    }
    pub fn case(&mut self, s: Spec, b: Device) {
        let good = bulk_oracle::run(s, b);
        if !good && self.bad < 3 {
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("status register section row={} spec={s:?}", self.count + 1);
        }
        self.record(good);
    }
    pub fn result(self) -> (u32, u32) {
        (self.count, self.bad)
    }
}
pub(super) fn old(mut s: Spec, ccr: u16, mask: u16) -> Spec {
    s.r = status(s.r, s.r.status_register() & 0x2000 | mask << 8 | ccr);
    s
}
pub(super) fn profile(s: Spec, profile: u8) -> Device {
    let (_, mut b) = s.seed();
    b.report = match profile {
        0 => None,
        1 => Some((usize::from(s.immediate), Kind::Host, 6)),
        2 => Some((s.count() - 1, Kind::Boundary, 8)),
        _ => unreachable!(),
    };
    b
}
pub(super) fn validate(s: Spec, b: &Device) -> bulk_model::Plan {
    let p = bulk_plan::plan(s, b);
    assert_eq!(p.phases.len(), s.count());
    assert_eq!(p.final_r.pc, s.anchor());
    assert_eq!(p.final_r.status_register(), s.published().status_register());
    assert_eq!(p.final_r.d, s.r.d);
    assert_eq!(p.final_r.a, s.r.a);
    assert_eq!((p.final_r.usp, p.final_r.ssp), (s.r.usp, s.r.ssp));
    p
}
pub(super) fn completion() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for ccr in 0..32 {
            for mask in 0..8 {
                for new_ccr in 0..32 {
                    for upper in [0, 0x2000, 0x8000, 0xa700] {
                        for elapsed in [4, 7] {
                            let mut s = old(base, ccr, mask);
                            s.value(upper | new_ccr);
                            s.wait = elapsed;
                            let (_, b) = s.seed();
                            total.case(s, b);
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn faults() -> (u32, u32) {
    let mut total = Count::default();
    for (base, at) in triggers() {
        for ccr in 0..32 {
            for source in SOURCES {
                for mask in [0, 1, 4, 7] {
                    for (kind, elapsed) in REPORTS {
                        for wait in [4, 7] {
                            let mut s = old(base, ccr, mask);
                            s.value(source);
                            s.wait = wait;
                            let (_, mut b) = s.seed();
                            b.report = Some((at, kind, elapsed));
                            total.case(s, b);
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn all_sources() -> (u32, u32) {
    let mut total = Count::default();
    for base in representatives() {
        for source in 0..=u16::MAX {
            for p in 0..3 {
                let mut s = base;
                s.value(source);
                total.case(s, profile(s, p));
            }
        }
    }
    total.result()
}
pub(super) fn upper_junk() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| !s.immediate) {
        for upper in [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555] {
            for source in SOURCES {
                for p in 0..3 {
                    let mut s = base;
                    s.r.d[s.reg] = (upper << 16) | u32::from(source);
                    total.case(s, profile(s, p));
                }
            }
        }
    }
    total.result()
}
pub(super) fn upper_status() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| s.base < 9 || s.base >= 18) {
        for bits in 0..32 {
            for ccr in 0..32 {
                for source in [0, 0xffff] {
                    for p in 0..3 {
                        let upper =
                            (bits & 7) << 8 | ((bits >> 3) & 1) << 13 | ((bits >> 4) & 1) << 15;
                        let mut s = base;
                        s.r = status(s.r, upper | ccr);
                        s.value(source);
                        total.case(s, profile(s, p));
                    }
                }
            }
        }
    }
    total.result()
}
