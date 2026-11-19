use super::*;
use device::Kind;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryStep, ExternalResetBoundary, ResetQualification,
};

fn boundary(clock: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(clock),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}
fn check(s: model::Spec, state: u8, failure: Option<(usize, Kind, u32)>) -> bool {
    let (mut cpu, mut bus) = s.seed();
    let fault = s.count() - 1;
    bus.report = Some((
        fault,
        if state == 0 {
            Kind::Host
        } else if state == 1 {
            Kind::Unavailable
        } else {
            Kind::Boundary
        },
        7,
    ));
    if state > 1 {
        bus.entry_failure = Some((
            0,
            if state == 2 {
                Kind::Boundary
            } else {
                Kind::Unavailable
            },
            7,
        ));
    }
    let plan = model::plan(s, &bus);
    let pre = oracle::predict(s, &bus, &plan);
    let mut expected = pre.clone();
    let actual = cpu.step_physical_instruction(&mut bus);
    let mut good = oracle::verify(&mut cpu, &mut bus, pre, actual);
    let before = cpu.clone();
    let device = bus.clone();
    good &= cpu.external_warm_reset(&mut bus, boundary(expected.clock, false))
        == Err(if state == 0 {
            EntryFault::HostFrozen
        } else {
            EntryFault::Input(EntryInputError::UnqualifiedReset)
        });
    good &= cpu == before && bus == device;
    if state == 0 {
        good &= cpu.external_warm_reset(&mut bus, boundary(expected.clock + 11, true))
            == Err(EntryFault::HostFrozen)
            && cpu == before
            && bus == device;
        return good;
    }
    good &= cpu.external_warm_reset(&mut bus, boundary(expected.clock - 1, true))
        == Err(EntryFault::Input(EntryInputError::NonmonotonicRelease))
        && cpu == before
        && bus == device;
    for (a, value) in [
        (0, 0),
        (2, 0x8000),
        (4, 0),
        (6, 0x7000),
        (0x7000, 0x4e71),
        (0x7002, 0x7201),
    ] {
        bus.word(a, value);
        expected.words.insert(a & 0xfffffe, value);
    }
    let release = expected.clock + 11;
    bus.clock = release;
    bus.events.clear();
    bus.entry_count = 0;
    bus.entry_failure = failure;
    expected.clock = release;
    expected.events.clear();
    expected.entries = 0;
    let outcome = entry_oracle::reset(&mut expected, &bus, release);
    let predicted = outcome.map(|()| EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: expected.clock - release,
    });
    let actual = cpu.external_warm_reset(&mut bus, boundary(release, true));
    good &= actual == predicted
        && cpu.registers == expected.r
        && cpu.queued_words() == expected.queue
        && cpu.clock().0 == expected.clock
        && bus.clock == expected.clock
        && bus.words == expected.words
        && bus.events == expected.events
        && bus.count == expected.count
        && bus.entry_count == expected.entries
        && cpu.physical_entry_state() == expected.state
        && cpu.physical_inspection() == expected.inspection
        && !cpu.trace_pending()
        && !cpu.stopped();
    if let Some(fault) = expected.guard {
        good &= check::freeze(&mut cpu, &mut bus, fault);
    }
    good
}
pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::publications() {
        count += 1;
        bad += u32::from(!check(s, 0, None));
        for state in 1..4 {
            for at in 0..6 {
                for kind in [Kind::Host, Kind::Boundary, Kind::Unavailable] {
                    count += 1;
                    bad += u32::from(!check(s, state, Some((at, kind, 7))));
                }
            }
            count += 1;
            bad += u32::from(!check(s, state, None));
        }
    }
    (count, bad)
}
