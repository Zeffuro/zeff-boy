use super::device::Device;
use super::model::{Case, Failure};
use super::{Tally, check, reset, seed};

fn primary_prefix(c: Case, at: usize) -> u64 {
    u64::from(c.p.idle()) * 2 + at as u64 * 4
}
fn entry_prefix(at: usize) -> u64 {
    4 + at as u64 * 4 + u64::from(at == 10) * 2
}
fn request(c: Case, b: Device, mark: fn(bool)) -> bool {
    let e = super::oracle::predict(c, &b);
    check::actual(c, b, &e, mark).is_some()
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    seed::each(|base| {
        for at in 0..base.p.count() {
            match index {
                9 | 25 => {
                    for position in 0..11 {
                        for variant in 0..3 {
                            let mut b = Device::seed(base);
                            b.fault(at, Failure::Boundary, 4);
                            if index == 9 {
                                b.entry_fault(
                                    position,
                                    [Failure::Host, Failure::Boundary, Failure::Unavailable]
                                        [variant],
                                    4,
                                );
                            } else {
                                b.entry_fault(position, Failure::Complete, [0, 1, 3][variant]);
                            }
                            tally.add(request(base, b, mark));
                        }
                    }
                }
                10 | 26 => {
                    for detector in 0..5 {
                        let mut c = base;
                        if index == 10 {
                            c.r.ssp |= 1;
                        }
                        let mut b = Device::seed(c);
                        if index == 26 {
                            b.words.insert(8, 0x4321);
                            b.words.insert(10, 0x4001);
                        }
                        b.fault(at, Failure::Boundary, 4);
                        reset::detector(&mut b, detector);
                        tally.add(request(c, b, mark));
                    }
                }
                19 => {
                    for position in 0..11 {
                        let mut c = base;
                        c.start = u64::MAX
                            - primary_prefix(c, at)
                            - u64::from(u32::MAX)
                            - entry_prefix(position)
                            - (u64::from(u32::MAX) - 1);
                        let mut b = Device::seed(c);
                        b.fault(at, Failure::Boundary, u32::MAX);
                        tally.add(request(c, b, mark));
                    }
                }
                20 => {
                    for room in 0..=4 {
                        let mut c = base;
                        c.start = u64::MAX - primary_prefix(c, at) - u64::from(u32::MAX) - room;
                        let mut b = Device::seed(c);
                        b.fault(at, Failure::Boundary, u32::MAX);
                        tally.add(request(c, b, mark));
                    }
                }
                21 => {
                    let original = Device::seed(base);
                    for stack in [
                        8,
                        10,
                        12,
                        14,
                        16,
                        18,
                        20,
                        22,
                        0x4321_4002,
                        0x4321_4008,
                        0,
                        0x0100_0008,
                    ] {
                        let mut c = base;
                        c.r.ssp = stack | (base.r.ssp & 1);
                        let mut b = original.clone();
                        b.fault(at, Failure::Boundary, 4);
                        tally.add(request(c, b, mark));
                    }
                }
                _ => panic!("compare entry section"),
            }
        }
    });
    tally.finish()
}
