use super::*;
use device::Kind;
use device::{Device, Event, Stage};
use model::{access, status};
use oracle::Expected;
use zeff_m68k::timing::physical::TimingQualification;

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
fn one(
    e: &mut Expected,
    b: &Device,
    a: BusAccess,
    write: Option<u16>,
) -> Result<u16, EntryFault<&'static str>> {
    if !e.reserve() {
        return Err(EntryFault::ClockOverflow);
    }
    let d = if write.is_some() {
        Direction::Write
    } else {
        Direction::Read
    };
    if a.logical_address & 1 != 0 {
        let (k, n) = if b.default_entry_detector {
            (Kind::Unavailable, 0)
        } else {
            b.entry_detector.unwrap_or((Kind::Unavailable, 0))
        };
        if b.default_entry_detector {
            e.omitted_detectors += 1;
        }
        if !b.default_entry_detector {
            e.events
                .push(Event::Detect(Stage::Entry, a, d, e.clock, n, k));
        }
        e.clock += u64::from(n);
        let cause = CriticalFault::AddressError {
            access: a,
            direction: d,
        };
        return Err(match k {
            Kind::Complete | Kind::Boundary => halt(e, cause, true),
            Kind::Unavailable => halt(e, cause, false),
            Kind::Host => {
                e.guard = Some(TimingFault::Frozen);
                EntryFault::HostFailure {
                    access: a,
                    direction: d,
                    source: "host",
                }
            }
        });
    }
    let v = write.unwrap_or_else(|| {
        e.words
            .get(&a.bus_address)
            .copied()
            .unwrap_or(e.default_word)
    });
    let (_, k, n) = b
        .entry_failure
        .filter(|(at, _, _)| *at == e.entries)
        .unwrap_or((e.entries, Kind::Complete, b.entry_wait));
    let deposited = write.is_some() && (k == Kind::Complete && n >= 4 || b.entry_write_deposit);
    if deposited {
        e.words.insert(a.bus_address, v);
    }
    e.events.push(Event::Transfer(
        Stage::Entry,
        a,
        d,
        v,
        e.clock,
        n,
        k,
        deposited,
    ));
    e.entries += 1;
    e.scripts.push((Stage::Entry, k, n));
    e.clock += u64::from(n);
    match k {
        Kind::Complete if n >= 4 => Ok(v),
        Kind::Complete => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::InvalidCompleteDuration { elapsed: n })
        }
        Kind::Host => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::HostFailure {
                access: a,
                direction: d,
                source: "host",
            })
        }
        Kind::Boundary => Err(halt(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction: d,
            },
            true,
        )),
        Kind::Unavailable => Err(halt(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction: d,
            },
            false,
        )),
    }
}
fn read(
    e: &mut Expected,
    b: &Device,
    a: u32,
    fc: u8,
    k: AccessKind,
) -> Result<u16, EntryFault<&'static str>> {
    one(e, b, access(a, fc, k), None)
}
fn long(
    e: &mut Expected,
    b: &Device,
    a: u32,
    fc: u8,
    k: AccessKind,
) -> Result<u32, EntryFault<&'static str>> {
    let hi = read(e, b, a, fc, k)?;
    let lo = read(e, b, a.wrapping_add(2), fc, k)?;
    Ok(u32::from(hi) << 16 | u32::from(lo))
}
fn handler(e: &mut Expected, b: &Device, a: u32) -> Result<[u16; 2], EntryFault<&'static str>> {
    let first = read(e, b, a, 6, AccessKind::Prefetch)?;
    if !e.idle(2) {
        return Err(EntryFault::ClockOverflow);
    }
    let last = read(e, b, a.wrapping_add(2), 6, AccessKind::Prefetch)?;
    Ok([first, last])
}
pub(super) fn group0(
    e: &mut Expected,
    b: &Device,
    cap: Group0Capture,
) -> Result<(), EntryFault<&'static str>> {
    let sp = e.r.ssp;
    if !e.idle(4) {
        return Err(EntryFault::ClockOverflow);
    }
    let ssw = (cap.ird & 0xffe0)
        | cap.function_code as u16
        | if cap.direction == Direction::Read {
            0x10
        } else {
            0
        };
    let values = [
        cap.internal_pc as u16,
        cap.phase_sr,
        (cap.internal_pc >> 16) as u16,
        cap.ird,
        cap.logical_address as u16,
        ssw,
        (cap.logical_address >> 16) as u16,
    ];
    for (offset, value) in [2u32, 6, 4, 8, 10, 14, 12].into_iter().zip(values) {
        one(
            e,
            b,
            access(sp.wrapping_sub(offset), 5, AccessKind::Stack),
            Some(value),
        )?;
    }
    let pc = long(e, b, u32::from(cap.vector) * 4, 5, AccessKind::Vector)?;
    let queue = handler(e, b, pc)?;
    e.r.ssp = sp.wrapping_sub(14);
    e.r.pc = pc;
    e.r = status(e.r, cap.phase_sr & 0x7fff | 0x2000);
    e.queue = Some(queue);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(())
}
pub(super) fn reset(
    e: &mut Expected,
    b: &Device,
    release: u64,
) -> Result<(), EntryFault<&'static str>> {
    e.clock = release;
    e.queue = None;
    e.state = PhysicalEntryState::Critical(EntryKind::ExternalWarmReset);
    e.guard = None;
    e.edge = false;
    e.entries = 0;
    if !e.idle(14) {
        return Err(EntryFault::ClockOverflow);
    }
    let sp = long(e, b, 0, 6, AccessKind::ResetVector)?;
    let pc = long(e, b, 4, 6, AccessKind::ResetVector)?;
    let queue = handler(e, b, pc)?;
    e.r.ssp = sp;
    e.r.pc = pc;
    e.r = status(e.r, (e.r.status_register() & 31) | 0x2700);
    e.queue = Some(queue);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(())
}
