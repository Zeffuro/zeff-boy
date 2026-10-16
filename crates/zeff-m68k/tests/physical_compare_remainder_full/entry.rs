use super::device::{Device, access};
use super::fixed::Event;
use super::model::{Failure, Report};
use super::oracle::Expected;
use super::{
    AccessKind, BusAccess, CriticalFault, Direction, EntryFault, EntryKind, Group0Capture,
    PhysicalEntryState, PhysicalInspection, TimingFault, TimingQualification,
};

fn halt(e: &mut Expected, cause: CriticalFault, qualified: bool) -> EntryFault<&'static str> {
    e.state = PhysicalEntryState::DoubleFaultHalted {
        cause,
        timing: if qualified {
            TimingQualification::Qualified
        } else {
            TimingQualification::Unavailable
        },
    };
    e.inspection = PhysicalInspection::Unavailable;
    e.guard = Some(TimingFault::DoubleFaultHalted);
    if qualified {
        EntryFault::DoubleFaultHalted(cause)
    } else {
        EntryFault::TimingUnavailable(cause)
    }
}
fn freeze(e: &mut Expected) {
    e.frozen = true;
    e.guard = Some(TimingFault::Frozen);
}
fn one(
    e: &mut Expected,
    b: &Device,
    a: BusAccess,
    write: Option<u16>,
) -> Result<u16, EntryFault<&'static str>> {
    if !e.reserve() {
        return Err(EntryFault::ClockOverflow);
    }
    let direction = if write.is_some() {
        Direction::Write
    } else {
        Direction::Read
    };
    if a.logical_address & 1 != 0 {
        let report = if b.default_secondary {
            Report {
                kind: Failure::Unavailable,
                elapsed: 0,
            }
        } else {
            b.secondary
        };
        if !b.default_secondary {
            e.events.push(Event::Transfer {
                stage: 3,
                phase: None,
                access: a,
                direction,
                value: None,
                start: e.clock,
                elapsed: report.elapsed,
                kind: report.kind,
                deposited: false,
            });
            e.counts[3] += 1;
            e.scripts.push(3, report.kind, report.elapsed);
        }
        e.clock += u64::from(report.elapsed);
        let cause = CriticalFault::AddressError {
            access: a,
            direction,
        };
        return Err(match report.kind {
            Failure::Host => {
                freeze(e);
                EntryFault::HostFailure {
                    access: a,
                    direction,
                    source: "host",
                }
            }
            Failure::Unavailable => halt(e, cause, false),
            Failure::Complete | Failure::Boundary => halt(e, cause, true),
        });
    }
    let report = b.entry[e.entries];
    let value = write.unwrap_or_else(|| e.words.sample(a.bus_address));
    if write.is_some() {
        e.words.insert(a.bus_address, value);
    }
    e.events.push(Event::Transfer {
        stage: 2,
        phase: None,
        access: a,
        direction,
        value: Some(value),
        start: e.clock,
        elapsed: report.elapsed,
        kind: report.kind,
        deposited: write.is_some(),
    });
    e.scripts.push(2, report.kind, report.elapsed);
    e.entries += 1;
    e.counts[2] += 1;
    e.clock += u64::from(report.elapsed);
    match report.kind {
        Failure::Complete if report.elapsed >= 4 => Ok(value),
        Failure::Complete => {
            freeze(e);
            Err(EntryFault::InvalidCompleteDuration {
                elapsed: report.elapsed,
            })
        }
        Failure::Host => {
            freeze(e);
            Err(EntryFault::HostFailure {
                access: a,
                direction,
                source: "host",
            })
        }
        Failure::Boundary | Failure::Unavailable => Err(halt(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction,
            },
            report.kind == Failure::Boundary,
        )),
    }
}
fn begin(e: &mut Expected, kind: EntryKind) {
    e.queue = None;
    e.pipeline_address = None;
    e.pending_trace = false;
    e.stopped = false;
    e.edge = false;
    e.imported = None;
    e.state = PhysicalEntryState::Critical(kind);
}
fn handler(
    e: &mut Expected,
    b: &Device,
    target: u32,
) -> Result<[u16; 2], EntryFault<&'static str>> {
    let first = one(
        e,
        b,
        access(target, true, true, AccessKind::Prefetch, false),
        None,
    )?;
    if !e.idle(2) {
        return Err(EntryFault::ClockOverflow);
    }
    let last = one(
        e,
        b,
        access(
            target.wrapping_add(2),
            true,
            true,
            AccessKind::Prefetch,
            false,
        ),
        None,
    )?;
    Ok([first, last])
}
fn long(
    e: &mut Expected,
    b: &Device,
    address: u32,
    kind: AccessKind,
    program: bool,
) -> Result<u32, EntryFault<&'static str>> {
    let high = one(e, b, access(address, true, program, kind, false), None)?;
    let low = one(
        e,
        b,
        access(address.wrapping_add(2), true, program, kind, false),
        None,
    )?;
    Ok(u32::from(high) << 16 | u32::from(low))
}
pub(super) fn group0(
    e: &mut Expected,
    b: &Device,
    capture: Group0Capture,
) -> Result<(), EntryFault<&'static str>> {
    begin(e, EntryKind::ImportedGroup0);
    let sp = e.r.ssp;
    if e.clock.checked_add(4).is_some() && e.before_entry.is_none() {
        e.before_entry = Some((e.clock, e.words.clone()));
    }
    if !e.idle(4) {
        return Err(EntryFault::ClockOverflow);
    }
    let fields = [
        (2, capture.internal_pc as u16),
        (6, capture.phase_sr),
        (4, (capture.internal_pc >> 16) as u16),
        (8, capture.ird),
        (10, capture.logical_address as u16),
        (
            14,
            (capture.ird & 0xffe0) | capture.function_code as u16 | 16,
        ),
        (12, (capture.logical_address >> 16) as u16),
    ];
    for (delta, value) in fields {
        one(
            e,
            b,
            access(
                sp.wrapping_sub(delta),
                true,
                false,
                AccessKind::Stack,
                false,
            ),
            Some(value),
        )?;
    }
    let target = long(e, b, 8, AccessKind::Vector, false)?;
    let queue = handler(e, b, target)?;
    e.r.ssp = sp.wrapping_sub(14);
    e.r.pc = target;
    e.r.set_status_register((capture.phase_sr | 0x2000) & !0x8000);
    e.queue = Some(queue);
    e.pipeline_address = Some(target);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(())
}
pub(super) fn warm_reset(
    e: &mut Expected,
    b: &Device,
    release: u64,
    qualified: bool,
) -> Result<zeff_m68k::timing::physical::EntryStep, EntryFault<&'static str>> {
    use zeff_m68k::timing::physical::{EntryInputError, EntryStep};
    if e.frozen {
        return Err(EntryFault::HostFrozen);
    }
    if !qualified {
        return Err(EntryFault::Input(EntryInputError::UnqualifiedReset));
    }
    if release < e.clock {
        return Err(EntryFault::Input(EntryInputError::NonmonotonicRelease));
    }
    e.clock = release;
    e.entries = 0;
    e.guard = None;
    begin(e, EntryKind::ExternalWarmReset);
    if !e.idle(14) {
        return Err(EntryFault::ClockOverflow);
    }
    let sp = long(e, b, 0, AccessKind::ResetVector, true)?;
    let target = long(e, b, 4, AccessKind::ResetVector, true)?;
    let queue = handler(e, b, target)?;
    e.r.ssp = sp;
    e.r.pc = target;
    e.r.set_status_register(0x2700 | e.r.status_register() & 31);
    e.queue = Some(queue);
    e.pipeline_address = Some(target);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: e.clock - release,
    })
}
