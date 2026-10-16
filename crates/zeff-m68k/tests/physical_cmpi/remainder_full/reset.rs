use super::*;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryStep, ExternalResetBoundary, ResetQualification,
};
#[derive(Clone, Copy, Default)]
struct Request {
    report: Option<(usize, Failure, u32)>,
    detector: usize,
    room: Option<(u8, usize, u64)>,
    level: u8,
}
fn boundary(release: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(release),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}
fn origin(b: &mut Device, c: Case, state: u8) {
    match state {
        0 => {}
        1 => b.failure = Some((c.form.count() - 1, Failure::Boundary)),
        2 => b.failure = Some((c.form.count() - 1, Failure::Unavailable)),
        3 | 4 => {
            b.failure = Some((c.form.count() - 1, Failure::Boundary));
            b.entry_failure = Some((
                0,
                if state == 3 {
                    Failure::Boundary
                } else {
                    Failure::Unavailable
                },
            ));
        }
        _ => unreachable!(),
    }
}
fn prepare(b: &mut Device, q: Request, release: u64) {
    let pc = if q.detector == 0 { 0x7000u32 } else { 0x7001 };
    for (a, v) in [
        (0, 0x1020),
        (2, 0x3001),
        (4, (pc >> 16) as u16),
        (6, pc as u16),
        (pc, 0x7201),
        (pc.wrapping_add(2), 0x4e71),
    ] {
        b.word(a, v);
    }
    b.clock = release;
    b.entry_count = 0;
    b.entry_failure = q.report.map(|(at, k, _)| (at, k));
    b.entry_elapsed = [4; 16];
    if let Some((at, _, n)) = q.report {
        b.entry_elapsed[at] = n;
    }
    b.default_secondary = q.detector == 1;
    b.secondary = match q.detector {
        2 => Some((Failure::Host, 7)),
        3 => Some((Failure::Boundary, 8)),
        4 => Some((Failure::Complete, 3)),
        5 => Some((Failure::Unavailable, 0)),
        _ => None,
    };
    if let Some((2, _, _)) = q.room {
        b.entry_elapsed[4] = u32::MAX;
    }
}
fn run_one(c: Case, state: u8, q: Request, mark: fn(bool)) -> bool {
    let mut config = Device::seed(c);
    origin(&mut config, c, state);
    let original = oracle::predict(c, &config);
    let release = match q.room {
        Some((0, at, room)) => {
            u64::MAX - capacity::B + room - 14 - at as u64 * 4 - u64::from(at == 5) * 2
        }
        Some((1, _, room)) => u64::MAX - room,
        Some((2, _, room)) => u64::MAX - 14 - 16 - capacity::B - room,
        _ => original.clock + 11,
    };
    let mut reset_config = config.clone();
    reset_config.words = original.words.clone();
    prepare(&mut reset_config, q, release);
    let mut e = original.clone();
    e.words = reset_config.words.clone();
    e.level = q.level;
    let expected = entry::warm_reset(&mut e, &reset_config, release).map(|()| EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: e.clock - release,
    });
    mark(true);
    let mut b = config.clone();
    let mut cpu =
        PrefetchCpu::from_prefetched(c.r, [c.opcode(), c.retained()], CpuClocks(c.start)).unwrap();
    let result_origin = check::step(&mut cpu, &mut b);
    if result_origin != original.result {
        mark(false);
        return false;
    }
    let guarded_origin = original
        .guard
        .clone()
        .is_none_or(|f| check::freeze(&mut cpu, &mut b, f));
    if !guarded_origin {
        mark(false);
        return false;
    }
    let saved_cpu = cpu.clone();
    let saved_bus = b.clone();
    cpu.interrupt_input().set_level(7);
    cpu.interrupt_input().set_level(q.level);
    prepare(&mut b, q, release);
    let result = if b.default_secondary {
        cpu.external_warm_reset(
            &mut device::DefaultSecondary(&mut b),
            boundary(release, true),
        )
    } else {
        cpu.external_warm_reset(&mut b, boundary(release, true))
    };
    let guarded = result == expected
        && e.guard
            .clone()
            .is_none_or(|f| check::freeze(&mut cpu, &mut b, f));
    mark(false);
    guarded_origin
        && guarded
        && result_origin == original.result
        && result == expected
        && check::verify(&saved_cpu, &saved_bus, &original, &config)
        && check::verify(&cpu, &b, &e, &reset_config)
}
fn rejected(c: Case, route: usize, mark: fn(bool)) -> bool {
    let mut config = Device::seed(c);
    if route >= 2 {
        config.failure = Some((
            c.form.count() - 1,
            if route == 2 {
                Failure::Host
            } else {
                Failure::Boundary
            },
        ));
        if route == 3 {
            config.entry_failure = Some((0, Failure::Host));
            config.secondary = Some((Failure::Host, 6));
        }
    }
    let e = oracle::predict(c, &config);
    let expected = if route >= 2 {
        EntryFault::HostFrozen
    } else {
        EntryFault::Input(if route == 0 {
            EntryInputError::UnqualifiedReset
        } else {
            EntryInputError::NonmonotonicRelease
        })
    };
    let release = e.clock - u64::from(route == 1);
    mark(true);
    let mut b = config.clone();
    let mut cpu =
        PrefetchCpu::from_prefetched(c.r, [c.opcode(), c.retained()], CpuClocks(c.start)).unwrap();
    let origin = check::step(&mut cpu, &mut b);
    if origin != e.result {
        mark(false);
        return false;
    }
    let guarded = e
        .guard
        .clone()
        .is_none_or(|f| check::freeze(&mut cpu, &mut b, f));
    if !guarded {
        mark(false);
        return false;
    }
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied = device.clone();
    let mut good = copy.external_warm_reset(&mut copied, boundary(release, route != 0))
        == Err(expected.clone());
    good &= copy == saved && copied == device;
    good &= cpu.external_warm_reset(&mut b, boundary(release, route != 0)) == Err(expected);
    good &= cpu == saved && b == device;
    mark(false);
    good && guarded && origin == e.result && check::verify(&cpu, &b, &e, &config)
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut t = Tally::default();
    model::each(|c| {
        if index == 15 {
            for route in 0..4 {
                t.add(rejected(c, route, mark));
            }
            return;
        }
        for state in 0..5 {
            match index {
                14 => t.add(run_one(c, state, Request::default(), mark)),
                22 => {
                    for at in 0..6 {
                        for (kind, elapsed) in [
                            (Failure::Host, 6),
                            (Failure::Boundary, 8),
                            (Failure::Unavailable, 5),
                            (Failure::Complete, 0),
                            (Failure::Complete, 1),
                            (Failure::Complete, 3),
                        ] {
                            t.add(run_one(
                                c,
                                state,
                                Request {
                                    report: Some((at, kind, elapsed)),
                                    ..Request::default()
                                },
                                mark,
                            ));
                        }
                    }
                }
                23 => {
                    for at in 0..6 {
                        t.add(run_one(
                            c,
                            state,
                            Request {
                                room: Some((0, at, 1)),
                                ..Request::default()
                            },
                            mark,
                        ));
                    }
                    for room in 0..=14 {
                        t.add(run_one(
                            c,
                            state,
                            Request {
                                room: Some((1, 0, room)),
                                ..Request::default()
                            },
                            mark,
                        ));
                    }
                    for room in 0..=2 {
                        t.add(run_one(
                            c,
                            state,
                            Request {
                                room: Some((2, 0, room)),
                                ..Request::default()
                            },
                            mark,
                        ));
                    }
                }
                24 => {
                    for detector in 1..=5 {
                        t.add(run_one(
                            c,
                            state,
                            Request {
                                detector,
                                ..Request::default()
                            },
                            mark,
                        ));
                    }
                }
                30 => {
                    for level in 0..8 {
                        t.add(run_one(
                            c,
                            state,
                            Request {
                                level,
                                ..Request::default()
                            },
                            mark,
                        ));
                    }
                }
                _ => unreachable!(),
            }
        }
    });
    t.finish()
}
