use super::*;
use oracle::Expected;
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
    let direction = if write.is_some() {
        Direction::Write
    } else {
        Direction::Read
    };
    if a.logical_address & 1 != 0 {
        let (kind, elapsed) = if b.default_secondary {
            (Failure::Unavailable, 0)
        } else {
            b.secondary.unwrap_or((Failure::Unavailable, 0))
        };
        if !b.default_secondary {
            e.events.push(Event::EntryDetect(a, direction, e.clock));
        }
        e.clock += u64::from(elapsed);
        let cause = CriticalFault::AddressError {
            access: a,
            direction,
        };
        return Err(match kind {
            Failure::Host => {
                e.guard = Some(TimingFault::Frozen);
                EntryFault::HostFailure {
                    access: a,
                    direction,
                    source: "host",
                }
            }
            Failure::Boundary | Failure::Complete => halt(e, cause, true),
            Failure::Unavailable => halt(e, cause, false),
        });
    }
    let value = write.unwrap_or_else(|| e.words.sample(a.bus_address));
    if write.is_some() {
        e.events.push(Event::EntryWrite(a, value, e.clock));
        e.words.insert(a.bus_address, value);
    } else {
        e.events.push(Event::EntryRead(a, e.clock));
    }
    let elapsed = b.entry_elapsed[e.entries];
    let kind = b
        .entry_failure
        .filter(|(at, _)| *at == e.entries)
        .map_or(Failure::Complete, |(_, k)| k);
    e.entries += 1;
    e.scripts.push(true, kind, elapsed);
    e.clock += u64::from(elapsed);
    match kind {
        Failure::Complete if elapsed >= 4 => Ok(value),
        Failure::Complete => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::InvalidCompleteDuration { elapsed })
        }
        Failure::Host => {
            e.guard = Some(TimingFault::Frozen);
            Err(EntryFault::HostFailure {
                access: a,
                direction,
                source: "host",
            })
        }
        Failure::Boundary => Err(halt(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction,
            },
            true,
        )),
        Failure::Unavailable => Err(halt(
            e,
            CriticalFault::BusAbort {
                access: a,
                direction,
            },
            false,
        )),
    }
}
fn access(a: u32, program: bool, kind: AccessKind) -> BusAccess {
    super::super::oracle::access(a, true, program, kind)
}
fn long(
    e: &mut Expected,
    b: &Device,
    a: u32,
    program: bool,
    kind: AccessKind,
) -> Result<u32, EntryFault<&'static str>> {
    let hi = one(e, b, access(a, program, kind), None)?;
    let lo = one(e, b, access(a.wrapping_add(2), program, kind), None)?;
    Ok(u32::from(hi) << 16 | u32::from(lo))
}
fn handler(e: &mut Expected, b: &Device, pc: u32) -> Result<[u16; 2], EntryFault<&'static str>> {
    let first = one(e, b, access(pc, true, AccessKind::Prefetch), None)?;
    if !e.idle(2) {
        return Err(EntryFault::ClockOverflow);
    }
    let last = one(
        e,
        b,
        access(pc.wrapping_add(2), true, AccessKind::Prefetch),
        None,
    )?;
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
    let ssw = (cap.ird & 0xffe0) | cap.function_code as u16 | 16;
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
            access(sp.wrapping_sub(offset), false, AccessKind::Stack),
            Some(value),
        )?;
    }
    let pc = long(e, b, u32::from(cap.vector) * 4, false, AccessKind::Vector)?;
    let queue = handler(e, b, pc)?;
    e.r.ssp = sp.wrapping_sub(14);
    e.r.pc = pc;
    e.r.set_status_register(cap.phase_sr & !TRACE | SUPERVISOR);
    e.queue = Some(queue);
    Ok(())
}
pub(super) fn warm_reset(
    e: &mut Expected,
    b: &Device,
    release: u64,
) -> Result<(), EntryFault<&'static str>> {
    e.clock = release;
    e.queue = None;
    e.guard = None;
    e.entries = 0;
    e.edge = false;
    e.state = PhysicalEntryState::Critical(EntryKind::ExternalWarmReset);
    if !e.idle(14) {
        return Err(EntryFault::ClockOverflow);
    }
    let sp = long(e, b, 0, true, AccessKind::ResetVector)?;
    let pc = long(e, b, 4, true, AccessKind::ResetVector)?;
    let queue = handler(e, b, pc)?;
    e.r.ssp = sp;
    e.r.pc = pc;
    e.r.set_status_register(0x2700 | e.r.status_register() & 31);
    e.queue = Some(queue);
    e.inspection = PhysicalInspection::DiagnosticCompleteValues;
    Ok(())
}
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut t = Tally::default();
    model::each(|c| {
        for at in 0..c.form.count() {
            if index == 8 || index == 25 {
                for secondary in 0..11 {
                    for n in 0..3 {
                        t.add(check::run(
                            c,
                            |b| {
                                b.failure = Some((at, Failure::Boundary));
                                b.entry_failure = Some((
                                    secondary,
                                    if index == 25 {
                                        Failure::Complete
                                    } else {
                                        [Failure::Host, Failure::Boundary, Failure::Unavailable][n]
                                    },
                                ));
                                if index == 25 {
                                    b.entry_elapsed[secondary] = [0, 1, 3][n];
                                }
                            },
                            mark,
                        ));
                    }
                }
            } else {
                for route in 0..5 {
                    let mut odd = c;
                    if index == 9 {
                        odd.r.ssp |= 1;
                    }
                    t.add(check::run(
                        odd,
                        |b| {
                            b.failure = Some((at, Failure::Boundary));
                            if index == 26 {
                                b.word(10, 0x4001);
                            }
                            b.default_secondary = route == 0;
                            b.secondary = if route == 0 {
                                None
                            } else {
                                Some((
                                    [
                                        Failure::Unavailable,
                                        Failure::Host,
                                        Failure::Boundary,
                                        Failure::Complete,
                                        Failure::Unavailable,
                                    ][route],
                                    if route == 4 { 0 } else { 7 },
                                ))
                            };
                        },
                        mark,
                    ));
                }
            }
        }
    });
    t.finish()
}
