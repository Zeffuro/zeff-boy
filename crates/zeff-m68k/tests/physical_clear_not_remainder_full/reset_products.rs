use super::device::{DefaultSecondary, Device};
use super::model::{Case, Failure, Report};
use super::oracle::Expected;
use super::{CpuClocks, EntryFault, PrefetchCpu, check, entry, seed};
use zeff_m68k::timing::physical::{EntryStep, ExternalResetBoundary, ResetQualification};

const REPORTS: [(Failure, u32); 6] = [
    (Failure::Complete, 0),
    (Failure::Complete, 1),
    (Failure::Complete, 3),
    (Failure::Host, 6),
    (Failure::Boundary, 8),
    (Failure::Unavailable, 5),
];

pub(super) fn detector(b: &mut Device, index: usize) {
    b.default_secondary = index == 0;
    b.secondary = [
        Report {
            kind: Failure::Unavailable,
            elapsed: 0,
        },
        Report {
            kind: Failure::Unavailable,
            elapsed: 5,
        },
        Report {
            kind: Failure::Host,
            elapsed: 6,
        },
        Report {
            kind: Failure::Complete,
            elapsed: 0,
        },
        Report {
            kind: Failure::Boundary,
            elapsed: 8,
        },
    ][index];
}

fn configure(b: &mut Device, c: Case, origin: usize) {
    let at = c.p.extensions() + usize::from(c.p.memory());
    match origin {
        0 => (),
        1 => b.fault(at, Failure::Host, 6),
        2 => b.fault(at, Failure::Unavailable, 5),
        3 => {
            b.fault(at, Failure::Boundary, 8);
            b.entry_fault(10, Failure::Boundary, 8);
        }
        4 => b.fault(at, Failure::Complete, 3),
        5 => {
            b.fault(at, Failure::Boundary, 8);
            b.entry_fault(0, Failure::Host, 6);
            detector(b, 2);
        }
        _ => panic!("reset origin"),
    }
}

fn step(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    boundary: ExternalResetBoundary,
) -> Result<EntryStep, EntryFault<&'static str>> {
    if b.default_secondary {
        cpu.external_warm_reset(&mut DefaultSecondary(b), boundary)
    } else {
        cpu.external_warm_reset(b, boundary)
    }
}

fn execute(
    mut cpu: PrefetchCpu,
    mut b: Device,
    mut e: Expected,
    release: u64,
    qualified: bool,
    mark: fn(bool),
) -> bool {
    b.clock = release;
    let boundary = ExternalResetBoundary {
        sampled_release: CpuClocks(release),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    };
    let rejected = e.frozen || !qualified || release < e.clock;
    e.entries = 0;
    let result = entry::warm_reset(&mut e, &b, release, qualified);
    let device_clock = if rejected { release } else { e.clock };
    mark(true);
    let mut copy = cpu.clone();
    let mut copied_bus = b.clone();
    let copied_result = step(&mut copy, &mut copied_bus, boundary);
    mark(false);
    if copied_result != result || !check::verify_at(&copy, &copied_bus, &e, device_clock) {
        return false;
    }
    mark(true);
    let actual = step(&mut cpu, &mut b, boundary);
    mark(false);
    actual == result
        && cpu == copy
        && b == copied_bus
        && check::verify_at(&cpu, &b, &e, device_clock)
        && e.guard
            .clone()
            .is_none_or(|fault| check::guards(&mut cpu, &mut b, fault, mark))
}

pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let expected = match index {
        30 => 77760,
        31 => 25920,
        32 => 1728,
        34 => 38880,
        36 => 12960,
        39 => 17280,
        _ => panic!("reset product"),
    };
    let (mut count, mut bad) = (0, 0);
    seed::each(|base| {
        let origins: usize = if index == 32 { 4 } else { 5 };
        for origin in 0..origins {
            let routes: usize = match index {
                30 => 36,
                31 => 12,
                32 => 1,
                34 => 18,
                36 => 6,
                39 => 8,
                _ => unreachable!(),
            };
            for route in 0..routes {
                let mut c = base;
                if index == 31 {
                    c.reset_ssp = [0x90000, 0x90001][route / 6];
                    c.reset_handler = if route % 6 == 0 { 0x7000 } else { 0x7001 };
                }
                let actual_origin = if index == 32 {
                    [0, 0, 1, 5][origin]
                } else {
                    origin
                };
                let mut config = Device::seed(c);
                configure(&mut config, c, actual_origin);
                let mut e = super::oracle::predict(c, &config);
                let good = if let Some((mut cpu, mut b)) = check::actual(c, config, &e, mark) {
                    b.entry_count = 0;
                    b.entry = [Report::default(); 16];
                    b.entry_deposit = [false; 16];
                    b.default_secondary = true;
                    let mut release = e.clock;
                    if index == 30 {
                        let (kind, elapsed) = REPORTS[route % 6];
                        b.entry_fault(route / 6, kind, elapsed);
                    }
                    if index == 31 {
                        detector(&mut b, (route % 6).saturating_sub(1));
                    }
                    if index == 32 && origin == 1 {
                        release = e.clock - 1;
                    }
                    if index == 34 {
                        let at = route / 3;
                        let room = u32::MAX as u64 - 1 + (route % 3) as u64;
                        let padding = if at == 0 { 0 } else { u32::MAX as u64 - 4 };
                        let prefix = 14 + at as u64 * 4 + u64::from(at == 5) * 2 + padding;
                        release = u64::MAX - prefix - room;
                        if at != 0 {
                            b.entry_fault(at - 1, Failure::Complete, u32::MAX);
                        }
                    }
                    if index == 36 {
                        let idle = route / 3;
                        let n = if idle == 0 { 14 } else { 2 };
                        let room = n - 1 + (route % 3) as u64;
                        let prefix = if idle == 0 {
                            0
                        } else {
                            14 + 5 * 4 + u32::MAX as u64 - 4
                        };
                        release = u64::MAX - prefix - room;
                        if idle != 0 {
                            b.entry_fault(4, Failure::Complete, u32::MAX);
                        }
                    }
                    if index == 39 {
                        mark(true);
                        cpu.interrupt_input().set_level(7);
                        cpu.interrupt_input().set_level(route as u8);
                        mark(false);
                        e.level = route as u8;
                        e.edge = true;
                    }
                    execute(cpu, b, e, release, index != 32 || origin != 0, mark)
                } else {
                    false
                };
                count += 1;
                bad += u32::from(!good);
            }
        }
    });
    assert_eq!(count, expected);
    (count, bad)
}
