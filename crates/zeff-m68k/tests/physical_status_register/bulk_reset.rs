use super::*;
use bulk_device::Device;
use bulk_device::Kind;
use bulk_model::Spec;
use bulk_oracle::Expected;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryStep, ExternalResetBoundary, ResetQualification,
};

pub(super) fn seeds() -> Vec<Spec> {
    bulk_model::encoded()
}

pub(super) fn origin(s: Spec, state: u8) -> (PrefetchCpu, Device, Expected, bool) {
    let (mut cpu, mut b) = s.seed();
    match state {
        0 => {}
        1 => b.report = Some((s.count() - 1, Kind::Boundary, 8)),
        2 => b.report = Some((s.count() - 1, Kind::Unavailable, 5)),
        3 | 4 => {
            b.report = Some((s.count() - 1, Kind::Boundary, 8));
            b.entry_failure = Some((
                0,
                if state == 3 {
                    Kind::Boundary
                } else {
                    Kind::Unavailable
                },
                8,
            ));
        }
        _ => unreachable!(),
    }
    let p = bulk_plan::plan(s, &b);
    let e = bulk_oracle::predict(s, &b, &p);
    let actual = cpu.step_physical_instruction(&mut b);
    let good = bulk_oracle::verify(&mut cpu, &mut b, e.clone(), actual);
    (cpu, b, e, good)
}

pub(super) fn boundary(clock: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(clock),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}

pub(super) fn prepare(b: &mut Device, e: &mut Expected, release: u64, sp: u32, pc: u32) {
    for (a, v) in [
        (0, (sp >> 16) as u16),
        (2, sp as u16),
        (4, (pc >> 16) as u16),
        (6, pc as u16),
        (pc, 0x4e71),
        (pc.wrapping_add(2), 0x7002),
    ] {
        b.word(a, v);
        e.words.insert(a & 0x00ff_fffe, v);
    }
    b.clock = release;
    b.events.clear();
    b.entry_count = 0;
    b.entry_failure = None;
    b.entry_detector = None;
    e.events.clear();
    e.entries = 0;
}

pub(super) fn verify(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    e: Expected,
    actual: Result<EntryStep, EntryFault<&'static str>>,
    predicted: Result<EntryStep, EntryFault<&'static str>>,
) -> bool {
    let mut good = actual == predicted
        && cpu.registers == e.r
        && cpu.queued_words() == e.queue
        && cpu.clock().0 == e.clock
        && b.clock == e.clock
        && b.words == e.words
        && b.events == e.events
        && b.count == e.count
        && b.entry_count == e.entries
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && !cpu.trace_pending()
        && !cpu.stopped()
        && bulk_private::interrupts(cpu, e.level, e.edge)
        && bulk_private::verify(cpu, &e);
    if let Some(fault) = e.guard {
        good &= bulk_oracle::freeze(cpu, b, fault);
    }
    good
}

pub(super) fn execute(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    mut e: Expected,
    release: u64,
) -> bool {
    let predicted = bulk_entry_oracle::reset(&mut e, b, release).map(|()| EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: e.clock - release,
    });
    let actual = if b.default_entry_detector {
        cpu.external_warm_reset(
            &mut bulk_device::DefaultSecondary(b),
            boundary(release, true),
        )
    } else {
        cpu.external_warm_reset(b, boundary(release, true))
    };
    verify(cpu, b, e, actual, predicted)
}

pub(super) fn failures() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in seeds() {
        for state in 0..5 {
            for at in 0..6 {
                for (k, n) in [
                    (Kind::Complete, 0),
                    (Kind::Complete, 1),
                    (Kind::Complete, 3),
                    (Kind::Host, 6),
                    (Kind::Boundary, 8),
                    (Kind::Unavailable, 5),
                ] {
                    let (mut cpu, mut b, mut e, mut good) = origin(s, state);
                    cpu.interrupt_input().set_level(7);
                    cpu.interrupt_input().set_level(0);
                    let release = e.clock + 11;
                    prepare(&mut b, &mut e, release, 0x90000, 0x7000);
                    b.entry_failure = Some((at, k, n));
                    good &= execute(&mut cpu, &mut b, e, release);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}

pub(super) fn geometry() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in seeds() {
        for state in 0..5 {
            for parity in 0..2 {
                for route in 0..6 {
                    let (mut cpu, mut b, mut e, mut good) = origin(s, state);
                    cpu.interrupt_input().set_level(7);
                    cpu.interrupt_input().set_level(6);
                    e.level = 6;
                    e.edge = true;
                    let release = e.clock + 11;
                    let pc = if route == 0 { 0x7000 } else { 0x7001 };
                    prepare(&mut b, &mut e, release, 0x90000 | parity, pc);
                    if route == 1 {
                        b.default_entry_detector = true;
                    } else if route >= 2 {
                        b.entry_detector = Some(bulk_model::DETECTORS[route - 1]);
                    }
                    good &= execute(&mut cpu, &mut b, e, release);
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}

pub(super) fn inputs() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in seeds() {
        for input in 0..3 {
            let (mut cpu, mut b, e, mut good) = origin(s, 0);
            let (release, qualified, expected) = match input {
                0 => (
                    e.clock,
                    false,
                    EntryFault::Input(EntryInputError::UnqualifiedReset),
                ),
                1 => (
                    e.clock - 1,
                    true,
                    EntryFault::Input(EntryInputError::NonmonotonicRelease),
                ),
                _ => {
                    let (next, device) = s.seed();
                    cpu = next;
                    b = device;
                    b.report = Some((0, Kind::Host, 6));
                    let expected = bulk_oracle::predict(s, &b, &bulk_plan::plan(s, &b));
                    let actual = cpu.step_physical_instruction(&mut b);
                    good &= bulk_oracle::verify(&mut cpu, &mut b, expected, actual);
                    (b.clock + 11, true, EntryFault::HostFrozen)
                }
            };
            let c = cpu.clone();
            let device = b.clone();
            let mut copied = c.clone();
            let mut copied_device = device.clone();
            good &= copied.external_warm_reset(&mut copied_device, boundary(release, qualified))
                == Err(expected.clone())
                && copied == c
                && copied_device == device;
            good &= cpu.external_warm_reset(&mut b, boundary(release, qualified)) == Err(expected)
                && cpu == c
                && b == device;
            count += 1;
            bad += u32::from(!good);
        }
    }
    (count, bad)
}
