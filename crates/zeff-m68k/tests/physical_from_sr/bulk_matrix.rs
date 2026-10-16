use super::*;
use bulk_device::Device;
use bulk_device::Kind;
use bulk_model::{Spec, encoded, forms, status, values};

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
            eprintln!("MOVE-from-SR section row={} spec={s:?}", self.count + 1);
        }
        self.record(good);
    }
    pub fn result(self) -> (u32, u32) {
        (self.count, self.bad)
    }
}
pub(super) fn bank(mut s: Spec, supervisor: bool, ccr: u16) -> Spec {
    s.r = status(s.r, ccr | if supervisor { 0x2000 } else { 0 });
    s
}
pub(super) fn completion() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for value in values(base.mode) {
                    for elapsed in [4, 7] {
                        let mut s = bank(base, supervisor, ccr);
                        s.value(value);
                        s.wait = elapsed;
                        let (_, b) = s.seed();
                        validate(s, &b);
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn validate(s: Spec, b: &Device) -> bulk_model::Plan {
    let p = bulk_plan::plan(s, b);
    assert_eq!(p.phases.len(), s.count());
    assert_eq!(p.address, s.address(s.low));
    assert_eq!(p.operand, s.operand);
    p
}
pub(super) fn faults(alternate: bool) -> (u32, u32) {
    let mut total = Count::default();
    let mut geometry = 0;
    for base in encoded() {
        let (_, b) = base.seed();
        let p = validate(base, &b);
        let positions: Vec<_> = p
            .phases
            .iter()
            .enumerate()
            .filter(|(_, p)| !alternate || p.direction == Direction::Write)
            .map(|(i, _)| i)
            .collect();
        geometry += positions.len();
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for value in values(base.mode) {
                    let mut s = bank(base, supervisor, ccr);
                    s.value(value);
                    let (_, b) = s.seed();
                    validate(s, &b);
                    for &at in &positions {
                        for (kind, elapsed) in
                            [(Kind::Boundary, 8), (Kind::Host, 6), (Kind::Unavailable, 5)]
                        {
                            let mut bus = b.clone();
                            bus.report = Some((at, kind, elapsed));
                            bus.write_deposit = !alternate;
                            total.case(s, bus);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(geometry, if alternate { 42 } else { 153 });
    total.result()
}
pub(super) fn odd(absent: bool) -> (u32, u32) {
    let mut total = Count::default();
    let cases: Vec<_> = if absent {
        encoded()
            .into_iter()
            .filter(|s| s.mode < 3 && s.reg == 7)
            .collect()
    } else {
        forms().into_iter().filter(|s| s.mode != 7).collect()
    };
    assert_eq!(cases.len(), if absent { 3 } else { 7 });
    for base in cases {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for value in values(base.mode) {
                    let mut s = bank(base, supervisor, ccr);
                    s.value(value);
                    s.target(s.address(s.low) | 1);
                    let (_, b) = s.seed();
                    validate(s, &b);
                    for detector in [
                        (Kind::Boundary, 8),
                        (Kind::Host, 6),
                        (Kind::Unavailable, 0),
                        (Kind::Unavailable, 5),
                        (Kind::Complete, 0),
                        (Kind::Complete, 3),
                        (Kind::Complete, 8),
                    ] {
                        let mut bus = b.clone();
                        bus.detector = Some(detector);
                        bus.default_entry_detector = true;
                        total.case(s, bus);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn short() -> (u32, u32) {
    let mut total = Count::default();
    let mut geometry = 0;
    for s in encoded() {
        let (_, b) = s.seed();
        let p = validate(s, &b);
        geometry += p.phases.len()
            + p.phases
                .iter()
                .filter(|p| p.direction == Direction::Write)
                .count();
        for supervisor in [false, true] {
            let s = bank(s, supervisor, 31);
            let (_, b) = s.seed();
            for (at, t) in p.phases.iter().enumerate() {
                for deposit in if t.direction == Direction::Write {
                    &[true, false][..]
                } else {
                    &[true][..]
                } {
                    for elapsed in [0, 1, 3] {
                        let mut bus = b.clone();
                        bus.report = Some((at, Kind::Complete, elapsed));
                        bus.write_deposit = *deposit;
                        total.case(s, bus);
                    }
                }
            }
        }
    }
    assert_eq!(geometry, 195);
    total.result()
}
pub(super) fn maximum() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for supervisor in [false, true] {
            let s = bank(base, supervisor, 31);
            let (_, b) = s.seed();
            validate(s, &b);
            for at in 0..s.count() {
                for kind in [
                    Kind::Complete,
                    Kind::Host,
                    Kind::Boundary,
                    Kind::Unavailable,
                ] {
                    let mut bus = b.clone();
                    bus.report = Some((at, kind, u32::MAX));
                    total.case(s, bus);
                }
            }
        }
    }
    total.result()
}
pub(super) fn default_primary() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| s.mode != 7) {
        for supervisor in [false, true] {
            let mut s = bank(base, supervisor, 31);
            s.target(s.address(s.low) | 1);
            let (_, mut b) = s.seed();
            b.default_detector = true;
            validate(s, &b);
            total.case(s, b);
        }
    }
    total.result()
}
