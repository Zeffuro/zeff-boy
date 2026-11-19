use super::device::Device;
use super::model::{Case, Failure};
use super::{check, matrix, reset_products, seed};

const REPORTS: [(Failure, u32); 6] = [
    (Failure::Complete, 0),
    (Failure::Complete, 1),
    (Failure::Complete, 3),
    (Failure::Host, 6),
    (Failure::Boundary, 8),
    (Failure::Unavailable, 5),
];

fn request(c: Case, b: Device, mark: fn(bool)) -> bool {
    let expected = super::oracle::predict(c, &b);
    check::actual(c, b, &expected, mark).is_some()
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let expected = match index {
        28 => 109272,
        29 => 50240,
        33 => 41448,
        35 => 7536,
        _ => panic!("entry product"),
    };
    let (mut count, mut bad) = (0, 0);
    let mut attempt = |c: Case, b: Device| {
        count += 1;
        bad += u32::from(!request(c, b, mark));
    };
    seed::each(|base| {
        for at in 0..base.p.count() {
            match index {
                28 => {
                    for position in 0..11 {
                        for (kind, elapsed) in REPORTS {
                            let mut b = Device::seed(base);
                            b.fault(at, Failure::Boundary, 8);
                            b.entry_fault(position, kind, elapsed);
                            attempt(base, b);
                        }
                    }
                    for position in 0..7 {
                        for (kind, elapsed) in REPORTS.into_iter().skip(3) {
                            let mut b = Device::seed(base);
                            b.fault(at, Failure::Boundary, 8);
                            b.entry_fault(position, kind, elapsed);
                            b.entry_deposit[position] = true;
                            attempt(base, b);
                        }
                    }
                }
                29 => {
                    for site in 0..2 {
                        let addresses = if site == 0 {
                            [0x9001, 0xffff_fffd]
                        } else {
                            [0x4001, 0x00ff_ffff]
                        };
                        for address in addresses {
                            for detector in 0..5 {
                                for value in [0, 1u32 << (base.p.bits - 1)] {
                                    let mut c = base;
                                    if site == 0 {
                                        c.r.ssp = address;
                                    } else {
                                        c.handler = address;
                                    }
                                    c = matrix::source(c, value);
                                    let mut b = Device::seed(c);
                                    b.fault(at, Failure::Boundary, 8);
                                    reset_products::detector(&mut b, detector);
                                    attempt(c, b);
                                }
                            }
                        }
                    }
                }
                33 | 35 => {
                    let positions: usize = if index == 33 { 11 } else { 2 };
                    for position in 0..positions {
                        let idle = if position == 0 { 4 } else { 2 };
                        let rooms = if index == 33 {
                            [u32::MAX as u64 - 1, u32::MAX as u64, u32::MAX as u64 + 1]
                        } else {
                            [idle - 1, idle, idle + 1]
                        };
                        for room in rooms {
                            let previous = if index == 33 {
                                position.checked_sub(1)
                            } else {
                                (position != 0).then_some(9)
                            };
                            let padding = if previous.is_some() {
                                u32::MAX as u64 - 4
                            } else {
                                0
                            };
                            let prefix = if index == 33 {
                                4 + position as u64 * 4 + u64::from(position == 10) * 2 + padding
                            } else if position == 0 {
                                0
                            } else {
                                4 + 10 * 4 + padding
                            };
                            let mut c = base;
                            c.start = u64::MAX
                                - u64::from(c.p.idle()) * 2
                                - at as u64 * 4
                                - u32::MAX as u64
                                - prefix
                                - room;
                            let mut b = Device::seed(c);
                            b.fault(at, Failure::Boundary, u32::MAX);
                            if let Some(previous) = previous {
                                b.entry_fault(previous, Failure::Complete, u32::MAX);
                            }
                            attempt(c, b);
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    });
    assert_eq!(count, expected);
    (count, bad)
}
