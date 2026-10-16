use super::*;
use device::{Event, Kind, Stage};
use fixture::{Case, ResultKind};

pub(super) fn run(c: Case, diagnose: bool) -> bool {
    let mut bus = c.device();
    let mut cpu = PrefetchCpu::from_prefetched(
        c.initial.r,
        c.initial.queue.unwrap(),
        CpuClocks(c.initial.clock),
    )
    .unwrap();
    let failed = c.transfers.iter().find(|t| {
        matches!(
            t.event,
            Event::Transfer {
                stage: Stage::Instruction,
                outcome: Kind::Host | Kind::Unavailable,
                ..
            }
        )
    });
    let cause = failed.map(|t| {
        if let Event::Transfer {
            access, direction, ..
        } = t.event
        {
            CriticalFault::BusAbort { access, direction }
        } else {
            unreachable!()
        }
    });
    let expected = match c.kind {
        ResultKind::Instruction => Ok(PhysicalInstructionStep::Instruction(TimedStep {
            opcode: c.initial.queue.unwrap()[0],
            clocks: c.duration,
            exception: None,
        })),
        ResultKind::Group0 => Ok(PhysicalInstructionStep::Group0 {
            opcode: c.initial.queue.unwrap()[0],
            capture: c.capture.unwrap().0,
            clocks: c.duration,
        }),
        ResultKind::Host => {
            let Event::Transfer {
                access, direction, ..
            } = failed.unwrap().event
            else {
                unreachable!()
            };
            Err(InstructionFault::Entry(EntryFault::HostFailure {
                access,
                direction,
                source: "host",
            }))
        }
        ResultKind::Unavailable => Err(InstructionFault::TransitionTimingUnavailable {
            phase: failed.unwrap().phase.unwrap(),
            cause: cause.unwrap(),
        }),
        ResultKind::Blocked => Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
    };
    let state = match c.kind {
        ResultKind::Group0 => PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
        ResultKind::Unavailable => PhysicalEntryState::FaultUnqualified {
            cause: cause.unwrap(),
        },
        _ => PhysicalEntryState::Idle,
    };
    let inspection = if matches!(c.kind, ResultKind::Unavailable) {
        PhysicalInspection::Unavailable
    } else {
        PhysicalInspection::DiagnosticCompleteValues
    };
    let events: Vec<_> = c.transfers.iter().map(|t| t.event).collect();
    let instruction_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Transfer {
                    stage: Stage::Instruction,
                    ..
                }
            )
        })
        .count();
    let entry_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Transfer {
                    stage: Stage::Entry,
                    ..
                }
            )
        })
        .count();
    let actual = cpu.step_physical_instruction(&mut bus);
    let mut good = actual == expected
        && cpu.registers == c.expected.r
        && cpu.queued_words() == c.expected.queue
        && cpu.clock().0 == c.expected.clock
        && bus.clock == c.expected.clock
        && bus.events == events
        && bus.words == c.final_memory.iter().copied().collect()
        && bus.instruction_count == instruction_count
        && bus.entry_count == entry_count
        && cpu.physical_entry_state() == state
        && cpu.physical_inspection() == inspection
        && !cpu.trace_pending()
        && !cpu.stopped();
    let writes: Vec<_> = bus
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Transfer {
                stage: Stage::Instruction,
                access,
                direction: Direction::Write,
                value,
                deposited: true,
                ..
            } => Some((access.logical_address, value)),
            _ => None,
        })
        .collect();
    let frame: Vec<_> = bus
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Transfer {
                stage: Stage::Entry,
                access,
                direction: Direction::Write,
                value,
                ..
            } => Some((access.logical_address, value)),
            _ => None,
        })
        .collect();
    good &= writes == c.writes && frame == c.frame;
    if let Some((capture, ssw)) = c.capture {
        if let Ok(PhysicalInstructionStep::Group0 {
            capture: actual, ..
        }) = &actual
        {
            good &= actual.special_status_word() == ssw;
        }
        let boundary = c
            .transfers
            .iter()
            .find(|t| {
                matches!(
                    t.event,
                    Event::Transfer {
                        stage: Stage::Instruction,
                        outcome: Kind::Boundary,
                        ..
                    }
                )
            })
            .unwrap()
            .boundary
            .unwrap();
        assert_eq!(
            (
                capture.internal_pc,
                u32::from(capture.ird),
                u32::from(capture.phase_sr),
                u32::from(ssw)
            ),
            (boundary[0], boundary[1], boundary[2], boundary[3])
        );
    }
    if let Some(t) = failed {
        let boundary = t.boundary.unwrap();
        assert_eq!(
            (
                c.expected.r.status_register(),
                c.expected.r.usp,
                c.expected.r.ssp
            ),
            (boundary[2] as u16, boundary[5], boundary[6])
        );
        let active = if c.source < 7 {
            c.expected.r.a[c.source]
        } else if c.expected.r.status_register() & 0x2000 == 0 {
            c.expected.r.usp
        } else {
            c.expected.r.ssp
        };
        assert_eq!(active, boundary[4]);
    }
    let guard = match c.kind {
        ResultKind::Host | ResultKind::Blocked => Some(TimingFault::Frozen),
        ResultKind::Unavailable => Some(TimingFault::PhysicalFaultUnqualified),
        _ => None,
    };
    if let Some(fault) = guard {
        let saved = cpu.clone();
        let device = bus.clone();
        good &= cpu.step_physical_instruction(&mut bus)
            == Err(InstructionFault::Execution(fault.clone()))
            && cpu.step_instruction(&mut bus) == Err(fault.clone())
            && cpu.step_boundary(&mut bus) == Err(fault.clone())
            && cpu.prime(&mut bus) == Err(fault.clone())
            && cpu.service_pending_trace(&mut bus) == Err(fault)
            && cpu == saved
            && bus == device;
    }
    if !good && diagnose {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "physical RMW {} actual={actual:?} expected={expected:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} state={:?}/{state:?} inspect={:?}/{inspection:?} device={bus:?} events={events:?}",
            c.label,
            cpu.registers,
            c.expected.r,
            cpu.queued_words(),
            c.expected.queue,
            cpu.clock().0,
            c.expected.clock,
            cpu.physical_entry_state(),
            cpu.physical_inspection()
        );
    }
    let _ = (c.label, diagnose);
    good
}
