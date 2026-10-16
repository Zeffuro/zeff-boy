use super::*;
use device::Kind;
use oracle::Expected;

fn halted(e: &mut Expected, cause: CriticalFault, qualified: bool) -> EntryFault<&'static str> {
    let timing = if qualified {
        TimingQualification::Qualified
    } else {
        TimingQualification::Unavailable
    };
    e.state = PhysicalEntryState::DoubleFaultHalted { cause, timing };
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
    bus: &Device,
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
        if !bus.default_entry_detector {
            e.events.push(Event::EntryDetect(a, direction, e.clock));
        }
        let (kind, elapsed) = if bus.default_entry_detector {
            (Kind::Unavailable, 0)
        } else {
            bus.entry_detector.unwrap_or((Kind::Unavailable, 0))
        };
        e.clock += u64::from(elapsed);
        let cause = CriticalFault::AddressError {
            access: a,
            direction,
        };
        return Err(match kind {
            Kind::Host => {
                e.guard = Some(TimingFault::Frozen);
                EntryFault::HostFailure {
                    access: a,
                    direction,
                    source: "host",
                }
            }
            Kind::Complete | Kind::Boundary => halted(e, cause, true),
            Kind::Unavailable => halted(e, cause, false),
        });
    }
    let value = write.unwrap_or_else(|| model::sample(&e.words, a.logical_address));
    if write.is_some() {
        e.events.push(Event::EntryWrite(a, e.clock, value));
        e.words.insert(a.bus_address, value);
    } else {
        e.events.push(Event::EntryRead(a, e.clock, value));
    }
    let (_, kind, elapsed) = bus
        .entry_failure
        .filter(|(n, _, _)| *n == e.entries)
        .unwrap_or((e.entries, Kind::Complete, 4));
    e.entries += 1;
    e.clock += u64::from(elapsed);
    match kind {
        Kind::Complete if elapsed >= 4 => Ok(value),
        Kind::Complete => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::InvalidCompleteDuration { elapsed })
        }
        Kind::Host => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::HostFailure {
                access: a,
                direction,
                source: "host",
            })
        }
        Kind::Boundary => Err(halted(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction,
            },
            true,
        )),
        Kind::Unavailable => Err(halted(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction,
            },
            false,
        )),
    }
}
fn read(
    e: &mut Expected,
    bus: &Device,
    address: u32,
    fc: u8,
    kind: AccessKind,
) -> Result<u16, EntryFault<&'static str>> {
    one(e, bus, access(address, fc, kind), None)
}
fn long(
    e: &mut Expected,
    bus: &Device,
    address: u32,
    fc: u8,
    kind: AccessKind,
) -> Result<u32, EntryFault<&'static str>> {
    let high = read(e, bus, address, fc, kind)?;
    let low = read(e, bus, address.wrapping_add(2), fc, kind)?;
    Ok(u32::from(high) << 16 | u32::from(low))
}
fn handler(
    e: &mut Expected,
    bus: &Device,
    target: u32,
) -> Result<[u16; 2], EntryFault<&'static str>> {
    let first = read(e, bus, target, 6, AccessKind::Prefetch)?;
    if !e.idle(2) {
        return Err(EntryFault::ClockOverflow);
    }
    let second = read(e, bus, target.wrapping_add(2), 6, AccessKind::Prefetch)?;
    Ok([first, second])
}
pub(super) fn group0(
    e: &mut Expected,
    bus: &Device,
    cap: Group0Capture,
) -> Result<(), EntryFault<&'static str>> {
    let sp = e.r.ssp;
    if !e.idle(4) {
        return Err(EntryFault::ClockOverflow);
    }
    let ssw = (cap.ird & 0xffe0) | 0x0010 | cap.function_code as u16;
    let values = [
        cap.internal_pc as u16,
        cap.phase_sr,
        (cap.internal_pc >> 16) as u16,
        cap.ird,
        cap.logical_address as u16,
        ssw,
        (cap.logical_address >> 16) as u16,
    ];
    for (&offset, &value) in [2u32, 6, 4, 8, 10, 14, 12].iter().zip(&values) {
        one(
            e,
            bus,
            access(sp.wrapping_sub(offset), 5, AccessKind::Stack),
            Some(value),
        )?;
    }
    let target = long(e, bus, u32::from(cap.vector) * 4, 5, AccessKind::Vector)?;
    let words = handler(e, bus, target)?;
    e.r.ssp = sp.wrapping_sub(14);
    e.r.pc = target;
    e.r = status(e.r, (cap.phase_sr & 0x7fff) | 0x2000);
    e.queue = Some(words);
    Ok(())
}
pub(super) fn reset(
    e: &mut Expected,
    bus: &Device,
    release: u64,
) -> Result<(), EntryFault<&'static str>> {
    e.clock = release;
    e.queue = None;
    e.state = PhysicalEntryState::Critical(EntryKind::ExternalWarmReset);
    e.inspection = PhysicalInspection::Unavailable;
    e.guard = None;
    if !e.idle(14) {
        return Err(EntryFault::ClockOverflow);
    }
    let sp = long(e, bus, 0, 6, AccessKind::ResetVector)?;
    let pc = long(e, bus, 4, 6, AccessKind::ResetVector)?;
    let words = handler(e, bus, pc)?;
    e.r.ssp = sp;
    e.r.pc = pc;
    e.r = status(e.r, (e.r.status_register() & 31) | 0x2700);
    e.queue = Some(words);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(())
}
