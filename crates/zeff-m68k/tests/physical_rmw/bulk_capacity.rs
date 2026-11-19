use super::*;
use bulk_matrix::{Count, bank};
use bulk_model::{Action, Plan, encoded, values};
use device::Kind;

pub(super) fn prefix(p: &Plan, b: &bulk_device::Device, target: usize) -> u64 {
    let mut elapsed = 0;
    let mut at = 0;
    for action in &p.actions {
        match action {
            Action::Idle(n) => elapsed += u64::from(*n),
            Action::Transfer(_) => {
                if at == target {
                    return elapsed;
                }
                elapsed += u64::from(bulk_model::report(b, at).1);
                at += 1;
            }
        }
    }
    panic!("RMW callback")
}
pub(super) fn reserves() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for supervisor in [false, true] {
            for edge in [
                u64::from(u32::MAX) - 1,
                u64::from(u32::MAX),
                u64::from(u32::MAX) + 1,
            ] {
                for elapsed in [4, 7] {
                    for ccr in [0, 31] {
                        for value in [0, *values(base.size).iter().max().unwrap()] {
                            let mut s = bank(base, supervisor, ccr);
                            s.wait = elapsed;
                            s.operand = value;
                            let (_, b) = s.seed();
                            let p = bulk_matrix::validate(s, &b);
                            for at in 0..s.count() {
                                let mut bus = b.clone();
                                s.start = u64::MAX - prefix(&p, &bus, at) - edge;
                                bus.clock = s.start;
                                bus.report = Some((at, Kind::Complete, u32::MAX));
                                total.case(s, bus);
                            }
                        }
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn idle() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| s.idle()) {
        for supervisor in [false, true] {
            for room in [0, 1, 2, 4294967295, 4294967296, 4294967297] {
                for ccr in [0, 31] {
                    for value in [0, *values(base.size).iter().max().unwrap()] {
                        let mut s = bank(base, supervisor, ccr);
                        s.operand = value;
                        s.start = u64::MAX - room;
                        let (_, b) = s.seed();
                        bulk_matrix::validate(s, &b);
                        total.case(s, b);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn exact() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded() {
        for supervisor in [false, true] {
            for elapsed in [4, 7] {
                let mut s = bank(base, supervisor, 31);
                s.wait = elapsed;
                let (_, mut b) = s.seed();
                let p = bulk_matrix::validate(s, &b);
                let at = s.count() - 1;
                s.start = u64::MAX - prefix(&p, &b, at) - u64::from(u32::MAX);
                b.clock = s.start;
                b.report = Some((at, Kind::Complete, u32::MAX));
                total.case(s, b);
            }
        }
    }
    total.result()
}
