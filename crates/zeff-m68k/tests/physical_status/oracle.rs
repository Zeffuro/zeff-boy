use super::*;
use device::Kind;
use model::{Action, Plan, Spec};

#[derive(Clone)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub count: usize,
    pub entries: usize,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub guard: Option<TimingFault<&'static str>>,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
}
impl Expected {
    pub fn new(s: Spec, bus: &Device) -> Self {
        Self {
            r: s.r,
            queue: Some([s.opcode(), s.extension]),
            clock: s.start,
            words: bus.words.clone(),
            events: Vec::new(),
            count: 0,
            entries: 0,
            state: PhysicalEntryState::Idle,
            inspection: PhysicalInspection::DiagnosticCompleteValues,
            guard: None,
            result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
        }
    }
    pub fn overflow(&mut self) {
        self.result = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
        self.guard = Some(TimingFault::Frozen);
    }
    pub fn idle(&mut self, duration: u32) -> bool {
        if let Some(end) = self.clock.checked_add(u64::from(duration)) {
            self.events.push(Event::Idle(self.clock, duration));
            self.clock = end;
            true
        } else {
            self.overflow();
            false
        }
    }
    pub fn reserve(&mut self) -> bool {
        if self.clock.checked_add(u64::from(u32::MAX)).is_some() {
            true
        } else {
            self.overflow();
            false
        }
    }
    fn unqualified(&mut self, t: model::Phase, vector: u8, missing: bool) {
        let cause = cause(t.access, vector, Direction::Read);
        self.queue = None;
        self.inspection = PhysicalInspection::Unavailable;
        self.state = PhysicalEntryState::FaultUnqualified { cause };
        self.guard = Some(TimingFault::PhysicalFaultUnqualified);
        self.result = Err(if missing {
            InstructionFault::CaptureUnavailable {
                phase: t.phase,
                cause,
            }
        } else {
            InstructionFault::TransitionTimingUnavailable {
                phase: t.phase,
                cause,
            }
        });
    }
    fn host(&mut self, a: BusAccess) {
        self.queue = None;
        self.guard = Some(TimingFault::Frozen);
        self.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
            access: a,
            direction: Direction::Read,
            source: "host",
        }));
    }
}
pub(super) fn cause(a: BusAccess, vector: u8, direction: Direction) -> CriticalFault {
    if vector == 3 {
        CriticalFault::AddressError {
            access: a,
            direction,
        }
    } else {
        CriticalFault::BusAbort {
            access: a,
            direction,
        }
    }
}
pub(super) fn capture(t: model::Phase, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: t.pc,
        phase_sr: t.during.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}
fn transition(e: &mut Expected, s: Spec, bus: &Device, t: model::Phase, vector: u8) {
    if !model::known(t.ird) {
        e.unqualified(t, vector, true);
        return;
    }
    let cap = capture(t, vector);
    e.queue = None;
    e.state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
    match entry_oracle::group0(e, bus, cap) {
        Ok(()) => {
            e.result = Ok(PhysicalInstructionStep::Group0 {
                opcode: s.opcode(),
                capture: cap,
                clocks: e.clock - s.start,
            })
        }
        Err(error) => e.result = Err(InstructionFault::Entry(error)),
    }
}
pub(super) fn predict(s: Spec, bus: &Device, plan: &Plan) -> Expected {
    let mut e = Expected::new(s, bus);
    for action in &plan.actions {
        match *action {
            Action::Idle(after) => {
                if !e.idle(2) {
                    return e;
                }
                e.r = after;
            }
            Action::Read(t) => {
                assert_eq!(e.r, t.before);
                if !e.reserve() {
                    return e;
                }
                e.r = t.during;
                if t.access.logical_address & 1 != 0 {
                    if !bus.default_detector {
                        e.events.push(Event::Detect(t.access, e.clock));
                    }
                    let (kind, elapsed) = if bus.default_detector {
                        (Kind::Unavailable, 0)
                    } else {
                        bus.detector.unwrap_or((Kind::Boundary, bus.detect_elapsed))
                    };
                    e.clock += u64::from(elapsed);
                    match kind {
                        Kind::Host => e.host(t.access),
                        Kind::Complete | Kind::Unavailable => e.unqualified(t, 3, false),
                        Kind::Boundary => transition(&mut e, s, bus, t, 3),
                    }
                    return e;
                }
                if let Some((at, a, value)) = bus.mutation
                    && at == e.count
                {
                    e.words.insert(a & 0xfffffe, value);
                }
                let (kind, elapsed) = model::report(bus, e.count);
                e.events.push(Event::Read(t.access, e.clock, t.value));
                e.count += 1;
                e.clock += u64::from(elapsed);
                match kind {
                    Kind::Complete if elapsed >= 4 => {}
                    Kind::Complete => {
                        e.result = Err(InstructionFault::Entry(
                            EntryFault::InvalidCompleteDuration { elapsed },
                        ));
                        e.guard = Some(TimingFault::Frozen);
                        return e;
                    }
                    Kind::Host => {
                        e.host(t.access);
                        return e;
                    }
                    Kind::Unavailable => {
                        e.unqualified(t, 2, false);
                        return e;
                    }
                    Kind::Boundary => {
                        transition(&mut e, s, bus, t, 2);
                        return e;
                    }
                }
            }
        }
    }
    e.r = plan.final_r;
    e.queue = Some(plan.queue);
    e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
        opcode: s.opcode(),
        clocks: e.clock - s.start,
        exception: None,
    }));
    e
}
pub(super) fn verify(
    cpu: &mut PrefetchCpu,
    bus: &mut Device,
    e: Expected,
    actual: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
) -> bool {
    let success = matches!(actual, Ok(PhysicalInstructionStep::Instruction(_)));
    let mut good = actual == e.result
        && cpu.registers == e.r
        && cpu.queued_words() == e.queue
        && cpu.clock().0 == e.clock
        && bus.clock == e.clock
        && bus.words == e.words
        && bus.events == e.events
        && bus.count == e.count
        && bus.entry_count == e.entries
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && !cpu.trace_pending();
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::atomic::{AtomicU32, Ordering};
            static FAILURES: AtomicU32 = AtomicU32::new(0);
            if FAILURES.fetch_add(1, Ordering::Relaxed) < 8 {
                eprintln!(
                    "status actual={actual:?} expected={:?} regs={:?}/{:?} queue={:?}/{:?} clocks={}/{} counts={}/{}, entry={}/{} state={:?}/{:?} events={:?}/{:?}",
                    e.result,
                    cpu.registers,
                    e.r,
                    cpu.queued_words(),
                    e.queue,
                    bus.clock,
                    e.clock,
                    bus.count,
                    e.count,
                    bus.entry_count,
                    e.entries,
                    cpu.physical_entry_state(),
                    e.state,
                    bus.events,
                    e.events
                );
            }
        }
    }
    if let Some(fault) = e.guard {
        good &= check::freeze(cpu, bus, fault);
    }
    if good && success && e.r.status_register() & 0x8000 != 0 {
        let before = cpu.clone();
        let device = bus.clone();
        good &= cpu.step_physical_instruction(bus) == Err(InstructionFault::TraceUnqualified)
            && *cpu == before
            && *bus == device;
    }
    good
}
pub(super) fn run(s: Spec, bus: Device) -> bool {
    let mut cpu =
        PrefetchCpu::from_prefetched(s.r, [s.opcode(), s.extension], CpuClocks(s.start)).unwrap();
    seeded(s, &mut cpu, bus)
}
pub(super) fn seeded(s: Spec, cpu: &mut PrefetchCpu, mut bus: Device) -> bool {
    let plan = model::plan(s, &bus);
    let expected = predict(s, &bus, &plan);
    let actual = if bus.default_entry_detector {
        cpu.step_physical_instruction(&mut device::DefaultEntryDetector(&mut bus))
    } else if bus.default_detector {
        cpu.step_physical_instruction(&mut device::DefaultDetector(&mut bus))
    } else {
        cpu.step_physical_instruction(&mut bus)
    };
    verify(cpu, &mut bus, expected, actual)
}
pub(super) fn ordinary(s: Spec, path: u8) -> bool {
    let (_, mut bus) = s.seed();
    bus.report = match path {
        0 => None,
        1 => Some((s.count() - 2, Kind::Host, 7)),
        _ => Some((s.count() - 1, Kind::Boundary, 7)),
    };
    run(s, bus)
}
