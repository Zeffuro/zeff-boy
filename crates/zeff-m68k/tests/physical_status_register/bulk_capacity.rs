use super::*;
use bulk_device::{Device, Kind};
use bulk_matrix::Count;
use bulk_model::{Action, Plan, Spec, encoded, triggers};

pub(super) fn prefix(p: &Plan, b: &Device, target: usize) -> u64 {
    let mut elapsed = 0;
    let mut at = 0;
    for action in &p.actions {
        match action {
            Action::Idle(n) => elapsed += u64::from(*n),
            Action::Publish(_) => {}
            Action::Transfer(_) => {
                if at == target {
                    return elapsed;
                }
                elapsed += u64::from(bulk_model::report(b, at).1);
                at += 1;
            }
        }
    }
    panic!("status callback");
}
fn idle_prefix(s: Spec, b: &Device, target: usize) -> u64 {
    let p = bulk_plan::plan(s, b);
    let (mut elapsed, mut transfers, mut idles) = (0, 0, 0);
    for action in p.actions {
        match action {
            Action::Idle(n) => {
                if idles == target {
                    return elapsed;
                }
                elapsed += u64::from(n);
                idles += 1;
            }
            Action::Publish(_) => {}
            Action::Transfer(_) => {
                elapsed += u64::from(bulk_model::report(b, transfers).1);
                transfers += 1;
            }
        }
    }
    panic!("status idle");
}
pub(super) fn reserves() -> (u32, u32) {
    let mut total = Count::default();
    for (base, at) in triggers() {
        for room in [
            u64::from(u32::MAX) - 1,
            u64::from(u32::MAX),
            u64::from(u32::MAX) + 1,
        ] {
            for wait in [4, 7] {
                for ccr in [0, 31] {
                    for source in [0, 0xffff] {
                        let mut s = base;
                        s.wait = wait;
                        s.value(source);
                        s.r = bulk_model::status(s.r, s.r.status_register() & 0xffe0 | ccr);
                        let (_, mut b) = s.seed();
                        let p = bulk_matrix::validate(s, &b);
                        s.start = u64::MAX - prefix(&p, &b, at) - room;
                        b.clock = s.start;
                        b.report = Some((at, Kind::Complete, u32::MAX));
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn idles() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for at in 0..2 {
            for room in [
                0,
                1,
                2,
                3,
                u64::from(u32::MAX),
                u64::from(u32::MAX) + 1,
                u64::from(u32::MAX) + 2,
            ] {
                for ccr in [0, 31] {
                    for source in [0, 0xffff] {
                        let mut s = base;
                        s.value(source);
                        s.r = bulk_model::status(s.r, s.r.status_register() & 0xffe0 | ccr);
                        let (_, mut b) = s.seed();
                        if s.immediate {
                            b.elapsed[0] = u32::MAX;
                        }
                        s.start = u64::MAX - idle_prefix(s, &b, at) - room;
                        b.clock = s.start;
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn shorts() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        for elapsed in 0..4 {
            for kind in [Kind::Complete, Kind::Boundary] {
                let (_, mut b) = s.seed();
                b.report = Some((at, kind, elapsed));
                total.case(s, b);
            }
        }
    }
    total.result()
}
pub(super) fn maximum() -> (u32, u32) {
    let mut total = Count::default();
    for (s, at) in triggers() {
        for kind in [
            Kind::Complete,
            Kind::Host,
            Kind::Boundary,
            Kind::Unavailable,
        ] {
            let (_, mut b) = s.seed();
            b.report = Some((at, kind, u32::MAX));
            total.case(s, b);
        }
    }
    total.result()
}
pub(super) fn exact() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for wait in [4, 7] {
            let mut s = base;
            s.wait = wait;
            let (_, mut b) = s.seed();
            let p = bulk_matrix::validate(s, &b);
            let at = s.count() - 1;
            s.start = u64::MAX - prefix(&p, &b, at) - u64::from(u32::MAX);
            b.clock = s.start;
            b.report = Some((at, Kind::Complete, u32::MAX));
            total.case(s, b);
        }
    }
    total.result()
}
