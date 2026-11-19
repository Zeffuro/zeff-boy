use super::device::Device;
use super::model::{Case, Failure};
use super::{check, seed};

const ROOMS: [u64; 7] = [
    0,
    1,
    2,
    3,
    u32::MAX as u64 - 1,
    u32::MAX as u64,
    u32::MAX as u64 + 1,
];
const REPORTS: [(Failure, u32); 4] = [
    (Failure::Complete, 4),
    (Failure::Host, 6),
    (Failure::Unavailable, 5),
    (Failure::Boundary, 8),
];

pub(super) fn repeated(mark: fn(bool)) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for _ in 0..100 {
        let mut attempt = |c: Case, configure: &dyn Fn(&mut Device)| {
            count += 1;
            bad += u32::from(!check::run(c, configure, mark));
        };
        seed::each(|base| {
            let p = base.p;
            attempt(base, &|_| {});
            for at in 0..p.count() {
                for (kind, elapsed) in REPORTS {
                    attempt(base, &|b| b.fault(at, kind, elapsed));
                }
                let mut c = base;
                c.start =
                    u64::MAX - u64::from(p.idle()) * 2 - at as u64 * 4 - (u32::MAX as u64 - 1);
                attempt(c, &|_| {});
            }
            if p.idle() {
                for room in ROOMS {
                    let mut c = base;
                    c.start = u64::MAX - room;
                    attempt(c, &|_| {});
                }
            }
            if p.tail() {
                for room in ROOMS {
                    let mut c = base;
                    c.start = u64::MAX - u32::MAX as u64 - room;
                    attempt(c, &|b| b.fault(0, Failure::Complete, u32::MAX));
                }
            }
            if p.memory() {
                for (kind, elapsed) in REPORTS.into_iter().skip(1) {
                    attempt(base, &|b| {
                        b.fault(p.count() - 1, kind, elapsed);
                        b.primary_deposit[p.count() - 1] = true;
                    });
                }
            }
        });
    }
    assert_eq!(count, 884000);
    (count, bad)
}
