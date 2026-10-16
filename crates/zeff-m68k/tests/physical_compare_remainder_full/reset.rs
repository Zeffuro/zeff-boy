use super::device::{DefaultSecondary, Device};
use super::model::{Case, Failure, Report};
use super::oracle::Expected;
use super::{CpuClocks, EntryFault, PrefetchCpu, Tally};
use zeff_m68k::timing::physical::{EntryStep, ExternalResetBoundary, ResetQualification};

pub(super) fn configure(b: &mut Device, c: Case, origin: usize) {
    let at = c.p.count() - 1;
    match origin {
        0 => (),
        1 => b.fault(at, Failure::Host, 4),
        2 => b.fault(at, Failure::Unavailable, 4),
        3 => {
            b.fault(at, Failure::Boundary, 4);
            b.entry_fault(10, Failure::Boundary, 4);
        }
        4 => b.fault(at, Failure::Complete, 3),
        5 => {
            b.fault(at, Failure::Boundary, 4);
            b.entry_fault(0, Failure::Host, 4);
            b.secondary = Report {
                kind: Failure::Host,
                elapsed: 7,
            };
        }
        _ => panic!("reset origin"),
    }
}
pub(super) fn origin(
    c: Case,
    config: Device,
    mark: fn(bool),
) -> Option<(PrefetchCpu, Device, Expected)> {
    let e = super::oracle::predict(c, &config);
    super::check::actual(c, config, &e, mark).map(|(cpu, b)| (cpu, b, e))
}
pub(super) fn preparation(b: &mut Device) {
    b.entry_count = 0;
    b.entry = [Report::default(); 16];
    b.default_secondary = false;
    b.secondary = Report {
        kind: Failure::Boundary,
        elapsed: 7,
    };
}
pub(super) fn boundary(release: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(release),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}
fn step(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    release: u64,
    qualified: bool,
) -> Result<EntryStep, EntryFault<&'static str>> {
    let boundary = boundary(release, qualified);
    if b.default_secondary {
        cpu.external_warm_reset(&mut DefaultSecondary(b), boundary)
    } else {
        cpu.external_warm_reset(b, boundary)
    }
}
pub(super) fn execute(
    mut cpu: PrefetchCpu,
    mut b: Device,
    mut e: Expected,
    boundary: ExternalResetBoundary,
    mark: fn(bool),
    numeric: impl FnOnce(&Expected, &Result<EntryStep, EntryFault<&'static str>>) -> bool,
) -> bool {
    let release = boundary.sampled_release.0;
    let qualified = boundary.qualification == ResetQualification::QualifiedSafeAssertionAndRelease;
    e.entries = b.entry_count;
    let rejected = e.frozen || !qualified || release < e.clock;
    let expected = super::entry::warm_reset(&mut e, &b, release, qualified);
    let device_clock = if rejected { b.clock } else { e.clock };
    if !numeric(&e, &expected) {
        return false;
    }
    mark(true);
    let mut copy = cpu.clone();
    let mut copied_bus = b.clone();
    let copied_result = step(&mut copy, &mut copied_bus, release, qualified);
    mark(false);
    if copied_result != expected || !super::check::verify_at(&copy, &copied_bus, &e, device_clock) {
        return false;
    }
    mark(true);
    let actual = step(&mut cpu, &mut b, release, qualified);
    mark(false);
    if actual != expected
        || cpu != copy
        || b != copied_bus
        || !super::check::verify_at(&cpu, &b, &e, device_clock)
    {
        return false;
    }
    if let Some(fault) = e.guard {
        return super::check::guards(&mut cpu, &mut b, fault, mark);
    }
    true
}
pub(super) fn detector(b: &mut Device, index: usize) {
    b.default_secondary = index == 0;
    b.secondary = [
        Report {
            kind: Failure::Unavailable,
            elapsed: 0,
        },
        Report {
            kind: Failure::Host,
            elapsed: 7,
        },
        Report {
            kind: Failure::Boundary,
            elapsed: 8,
        },
        Report {
            kind: Failure::Complete,
            elapsed: 3,
        },
        Report {
            kind: Failure::Unavailable,
            elapsed: 0,
        },
    ][index];
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    super::seed::each(|c| {
        let origins = if index == 16 { 4 } else { 5 };
        for requested_origin in 0..origins {
            let actual_origin = if index == 16 {
                [0, 0, 1, 5][requested_origin]
            } else {
                requested_origin
            };
            let mut config = Device::seed(c);
            if index == 24 {
                config.words.insert(4, 0x4321);
                config.words.insert(6, 0x5001);
            }
            configure(&mut config, c, actual_origin);
            let count = match index {
                15 | 16 => 1,
                22 => 36,
                23 => 24,
                24 => 5,
                30 => 8,
                _ => panic!("reset section"),
            };
            for position in 0..count {
                let Some((cpu, mut b, mut e)) = origin(c, config.clone(), mark) else {
                    tally.add(false);
                    continue;
                };
                preparation(&mut b);
                let mut cpu = cpu;
                if index == 30 {
                    mark(true);
                    cpu.interrupt_input().set_level(7);
                    cpu.interrupt_input().set_level(position as u8);
                    mark(false);
                    e.level = position as u8;
                    e.edge = true;
                }
                let mut release = e.clock + 11;
                let qualified = index != 16 || requested_origin != 0;
                if index == 16 {
                    release = if requested_origin == 1 {
                        e.clock - 1
                    } else {
                        e.clock
                    };
                }
                if index == 22 {
                    let at = position / 6;
                    let (kind, elapsed) = [
                        (Failure::Host, 4),
                        (Failure::Boundary, 4),
                        (Failure::Unavailable, 4),
                        (Failure::Complete, 0),
                        (Failure::Complete, 1),
                        (Failure::Complete, 3),
                    ][position % 6];
                    b.entry_fault(at, kind, elapsed);
                }
                if index == 23 {
                    if position < 6 {
                        release = u64::MAX
                            - (14 + position as u64 * 4 + u64::from(position == 5) * 2)
                            - (u64::from(u32::MAX) - 1);
                    } else if position < 21 {
                        release = u64::MAX - (position as u64 - 6);
                    } else {
                        release = u64::MAX - 14 - 16 - u64::from(u32::MAX) - (position as u64 - 21);
                        b.entry_fault(4, Failure::Complete, u32::MAX);
                    }
                }
                if index == 24 {
                    detector(&mut b, position);
                }
                b.clock = release;
                tally.add(execute(
                    cpu,
                    b,
                    e,
                    boundary(release, qualified),
                    mark,
                    |_, _| true,
                ));
            }
        }
    });
    tally.finish()
}
