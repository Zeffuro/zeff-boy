use super::*;

pub(super) enum Path {
    Complete,
    Host(usize),
    Qualified(usize, [u16; 7]),
    Unknown(usize),
    Odd(Transfer, [u16; 7], u32),
}
pub(super) fn freeze(
    cpu: &mut PrefetchCpu,
    bus: &mut Device,
    fault: TimingFault<&'static str>,
) -> bool {
    let before = cpu.clone();
    let device = bus.clone();
    let mut good =
        cpu.step_physical_instruction(bus) == Err(InstructionFault::Execution(fault.clone()));
    good &= cpu.step_instruction(bus) == Err(fault.clone());
    good &= cpu.step_boundary(bus) == Err(fault.clone());
    good &= cpu.prime(bus) == Err(fault.clone());
    good &= cpu.service_pending_trace(bus) == Err(fault);
    good && *cpu == before && *bus == device
}
fn entered(
    t: Transfer,
    vector: u8,
    frame: [u16; 7],
    start: u64,
    events: &mut Vec<Event>,
    words: &mut BTreeMap<u32, u16>,
) -> (Registers, Group0Capture) {
    let capture = Group0Capture {
        vector,
        internal_pc: t.pc,
        phase_sr: t.r.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    events.push(Event::Idle(start, 4));
    for (i, (&off, &value)) in [2u32, 6, 4, 8, 10, 14, 12].iter().zip(&frame).enumerate() {
        let address = t.r.ssp.wrapping_sub(off);
        events.push(Event::EntryWrite(
            access(address, 5, AccessKind::Stack),
            start + 4 + i as u64 * 4,
            value,
        ));
        words.insert(address & 0xfffffe, value);
    }
    let v = u32::from(vector) * 4;
    events.extend([
        Event::EntryRead(access(v, 5, AccessKind::Vector), start + 32, 0),
        Event::EntryRead(access(v + 2, 5, AccessKind::Vector), start + 36, 0x6000),
        Event::EntryRead(access(0x6000, 6, AccessKind::Prefetch), start + 40, 0x4e71),
        Event::Idle(start + 44, 2),
        Event::EntryRead(access(0x6002, 6, AccessKind::Prefetch), start + 46, 0x7201),
    ]);
    let mut r = t.r;
    r.ssp = t.r.ssp - 14;
    r.pc = 0x6000;
    r.set_status_register((t.r.status_register() | 0x2000) & 0x7fff);
    (r, capture)
}
pub(super) fn run(mut c: Case, path: Path) -> bool {
    let mut cpu = c.seed();
    let mut words = c.bus.words.clone();
    let (reads, until, r, queue, result, state, inspect, events, guard) = match path {
        Path::Complete => (
            c.transfers.len(),
            c.duration,
            c.final_r,
            Some(c.final_queue),
            Ok(PhysicalInstructionStep::Instruction(TimedStep {
                opcode: c.opcode,
                clocks: c.duration,
                exception: None,
            })),
            PhysicalEntryState::Idle,
            PhysicalInspection::DiagnosticCompleteValues,
            c.events(c.transfers.len(), c.duration),
            None,
        ),
        Path::Host(at) => {
            c.bus.failure = Some((at, false));
            c.bus.elapsed[at] = 7;
            let t = c.transfers[at];
            let end = t.at + 7;
            (
                at + 1,
                end,
                t.r,
                None,
                Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: Direction::Read,
                    source: "host",
                })),
                PhysicalEntryState::Idle,
                PhysicalInspection::DiagnosticCompleteValues,
                c.events(at + 1, t.at),
                Some(TimingFault::Frozen),
            )
        }
        Path::Unknown(at) => {
            c.bus.failure = Some((at, true));
            c.bus.elapsed[at] = 7;
            let t = c.transfers[at];
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: Direction::Read,
            };
            (
                at + 1,
                t.at + 7,
                t.r,
                None,
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause,
                }),
                PhysicalEntryState::FaultUnqualified { cause },
                PhysicalInspection::Unavailable,
                c.events(at + 1, t.at),
                Some(TimingFault::PhysicalFaultUnqualified),
            )
        }
        Path::Qualified(at, frame) => {
            c.bus.failure = Some((at, true));
            c.bus.elapsed[at] = 7;
            let t = c.transfers[at];
            let mut events = c.events(at + 1, t.at);
            let (r, capture) = entered(t, 2, frame, t.at + 7, &mut events, &mut words);
            (
                at + 1,
                t.at + 57,
                r,
                Some([0x4e71, 0x7201]),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: c.opcode,
                    capture,
                    clocks: t.at + 57,
                }),
                PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
                PhysicalInspection::DiagnosticCompleteValues,
                events,
                None,
            )
        }
        Path::Odd(t, frame, elapsed) => {
            c.bus.detect_elapsed = elapsed;
            let n = c.transfers.len();
            let mut events = c.events(n, t.at);
            events.push(Event::Detect(t.access, t.at));
            let (r, capture) = entered(
                t,
                3,
                frame,
                t.at + u64::from(elapsed),
                &mut events,
                &mut words,
            );
            let end = t.at + u64::from(elapsed) + 50;
            (
                n,
                end,
                r,
                Some([0x4e71, 0x7201]),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: c.opcode,
                    capture,
                    clocks: end,
                }),
                PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
                PhysicalInspection::DiagnosticCompleteValues,
                events,
                None,
            )
        }
    };
    if let Some((at, address, value)) = c.bus.mutation
        && at < reads
    {
        words.insert(address & 0xfffffe, value);
    }
    let actual = cpu.step_physical_instruction(&mut c.bus);
    let mut good = actual == result
        && cpu.registers == r
        && cpu.queued_words() == queue
        && cpu.clock().0 == until
        && c.bus.clock == until
        && c.bus.events == events
        && c.bus.count == reads
        && c.bus.words == words
        && cpu.physical_entry_state() == state
        && cpu.physical_inspection() == inspect
        && !cpu.trace_pending();
    let qualified = matches!(actual, Ok(PhysicalInstructionStep::Group0 { .. }));
    good &= c.bus.entry_count == if qualified { 11 } else { 0 };
    if let Some(fault) = guard {
        good &= freeze(&mut cpu, &mut c.bus, fault);
    }
    if good
        && matches!(actual, Ok(PhysicalInstructionStep::Instruction(_)))
        && r.status_register() & 0x8000 != 0
    {
        let before = cpu.clone();
        let bus = c.bus.clone();
        good &= cpu.step_physical_instruction(&mut c.bus)
            == Err(InstructionFault::TraceUnqualified)
            && cpu == before
            && c.bus == bus;
    }
    good
}
pub(super) fn capacity(
    mut c: Case,
    r: Registers,
    end: u64,
    events: Vec<Event>,
    count: usize,
) -> bool {
    let mut cpu = c.seed();
    let words = c.bus.words.clone();
    let result = cpu.step_physical_instruction(&mut c.bus);
    result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        && cpu.registers == r
        && cpu.queued_words() == Some([c.opcode, c.queued])
        && cpu.clock().0 == end
        && c.bus.clock == end
        && c.bus.events == events
        && c.bus.words == words
        && c.bus.count == count
        && c.bus.entry_count == 0
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues
        && !cpu.trace_pending()
        && freeze(&mut cpu, &mut c.bus, TimingFault::Frozen)
}
