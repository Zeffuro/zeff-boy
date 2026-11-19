use super::*;
use device::{DefaultSecondary, Device, Kind};
use model::Spec;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryStep, ExternalResetBoundary, ResetQualification,
};
#[derive(Clone, Copy, Debug)]
pub(super) struct Request {
    pub sp: u32,
    pub pc: u32,
    pub report: Option<(usize, Kind, u32)>,
    pub detector: usize,
    pub level: u8,
    pub room: Option<(u8, usize, u64)>,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            sp: 0x90000,
            pc: 0x7000,
            report: None,
            detector: 0,
            level: 0,
            room: None,
        }
    }
}
pub(super) fn origin(b: &mut Device, s: Spec, origin: u8) {
    let f = s.count() - 1;
    match origin {
        0 => {}
        1 => b.report = Some((f, Kind::Boundary, 8)),
        2 => b.report = Some((f, Kind::Unavailable, 5)),
        3 | 4 => {
            b.report = Some((f, Kind::Boundary, 8));
            b.entry_failure = Some((
                0,
                if origin == 3 {
                    Kind::Boundary
                } else {
                    Kind::Unavailable
                },
                if origin == 3 { 8 } else { 5 },
            ));
        }
        _ => unreachable!(),
    }
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
fn prepare(b: &mut Device, q: Request, release: u64) {
    for (a, v) in [
        (0, (q.sp >> 16) as u16),
        (2, q.sp as u16),
        (4, (q.pc >> 16) as u16),
        (6, q.pc as u16),
        (q.pc, 0x4e71),
        (q.pc.wrapping_add(2), 0x7002),
    ] {
        b.word(a, v);
    }
    b.clock = release;
    b.entry_count = 0;
    b.entry_failure = q.report;
    b.entry_write_deposit = false;
    b.entry_wait = 4;
    b.default_entry_detector = q.detector == 0;
    b.entry_detector = if q.detector == 0 {
        None
    } else {
        Some(model::DETECTORS[q.detector])
    };
    if let Some((kind, _, _)) = q.room
        && kind == 2
    {
        b.entry_failure = Some((4, Kind::Complete, u32::MAX));
    }
}
pub(super) fn run(s: Spec, state: u8, q: Request, mark: fn(bool)) -> bool {
    let (_, _, mut config, _, _) = s.seed();
    origin(&mut config, s, state);
    let origin_e = oracle::predict(s, &config);
    let release = match q.room {
        Some((0, at, room)) => u64::MAX - 14 - at as u64 * 4 - if at == 5 { 2 } else { 0 } - room,
        Some((1, _, room)) => u64::MAX - room,
        Some((2, _, room)) => u64::MAX - 14 - 4 * 4 - u64::from(u32::MAX) - room,
        _ => origin_e.clock + 11,
    };
    let mut e = origin_e.clone();
    let mut reset_config = config.clone();
    reset_config.words = origin_e.words.clone();
    prepare(&mut reset_config, q, release);
    e.words = reset_config.words.clone();
    e.level = q.level;
    e.edge = true;
    let predicted = entry_oracle::reset(&mut e, &reset_config, release).map(|()| EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: e.clock - release,
    });
    mark(true);
    let (r, queue, mut b, _, _) = s.seed();
    origin(&mut b, s, state);
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(s.start)).unwrap();
    let actual_origin = check::step(&mut cpu, &mut b);
    let origin_guarded = if let Some(f) = origin_e.guard.clone() {
        check::freeze(&mut cpu, &mut b, f)
    } else {
        true
    };
    let saved_origin = cpu.clone();
    let saved_device = b.clone();
    check::interrupts(&mut cpu, q.level, true);
    prepare(&mut b, q, release);
    let actual = if b.default_entry_detector {
        cpu.external_warm_reset(&mut DefaultSecondary(&mut b), boundary(release, true))
    } else {
        cpu.external_warm_reset(&mut b, boundary(release, true))
    };
    let guarded = if let Some(f) = e.guard.clone() {
        check::freeze(&mut cpu, &mut b, f)
    } else {
        true
    };
    mark(false);
    guarded
        && origin_guarded
        && actual_origin == origin_e.result
        && check::verify(&saved_origin, &saved_device, &origin_e, &config)
        && actual == predicted
        && check::verify(&cpu, &b, &e, &reset_config)
}
pub(super) fn rejected(s: Spec, route: u8, mark: fn(bool)) -> bool {
    let (_, _, mut config, _, _) = s.seed();
    if route == 2 {
        config.report = Some((s.count() - 1, Kind::Host, 6));
    } else if route == 3 {
        config.report = Some((s.count() - 1, Kind::Boundary, 8));
        config.entry_failure = Some((0, Kind::Host, 6));
    }
    let e = oracle::predict(s, &config);
    let error = match route {
        0 => EntryFault::Input(EntryInputError::UnqualifiedReset),
        1 => EntryFault::Input(EntryInputError::NonmonotonicRelease),
        _ => EntryFault::HostFrozen,
    };
    let release = if route == 1 { e.clock - 1 } else { e.clock };
    mark(true);
    let (r, q, mut b, _, _) = s.seed();
    b.report = config.report;
    b.entry_failure = config.entry_failure;
    let mut cpu = PrefetchCpu::from_prefetched(r, q, CpuClocks(s.start)).unwrap();
    let result = check::step(&mut cpu, &mut b);
    let origin_guarded = if let Some(f) = e.guard.clone() {
        check::freeze(&mut cpu, &mut b, f)
    } else {
        true
    };
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied_device = device.clone();
    let good = copy.external_warm_reset(&mut copied_device, boundary(release, route != 0))
        == Err(error.clone())
        && copy == saved
        && copied_device == device
        && cpu.external_warm_reset(&mut b, boundary(release, route != 0)) == Err(error)
        && cpu == saved
        && b == device;
    mark(false);
    good && origin_guarded && result == e.result && check::verify(&cpu, &b, &e, &config)
}
