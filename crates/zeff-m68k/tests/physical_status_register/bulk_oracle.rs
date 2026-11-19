use super::*;
use bulk_device::Kind;
use bulk_device::{Device, Event, Stage};
use bulk_model::{Action, Phase, Plan, Spec};

#[derive(Clone)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub default_word: u16,
    pub events: Vec<Event>,
    pub count: usize,
    pub entries: usize,
    pub level: u8,
    pub edge: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub guard: Option<TimingFault<&'static str>>,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
}
impl Expected {
    pub fn new(s: Spec, b: &Device) -> Self {
        Self {
            r: s.r,
            queue: Some([s.opcode(), s.queued()]),
            clock: s.start,
            words: b.words.clone(),
            default_word: b.default_word,
            events: Vec::new(),
            count: 0,
            entries: 0,
            level: s.level,
            edge: s.edge,
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
    pub fn reserve(&mut self) -> bool {
        if self.clock.checked_add(u64::from(u32::MAX)).is_some() {
            true
        } else {
            self.overflow();
            false
        }
    }
    pub fn idle(&mut self, n: u32) -> bool {
        if let Some(end) = self.clock.checked_add(u64::from(n)) {
            self.events.push(Event::Idle(self.clock, n));
            self.clock = end;
            true
        } else {
            self.overflow();
            false
        }
    }
    fn host(&mut self, t: Phase) {
        self.queue = None;
        self.guard = Some(TimingFault::Frozen);
        self.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
            access: t.access,
            direction: t.direction,
            source: "host",
        }));
    }
    fn unqualified(&mut self, t: Phase, vector: u8, missing: bool) {
        let cause = cause(t.access, vector, t.direction);
        self.queue = None;
        self.state = PhysicalEntryState::FaultUnqualified { cause };
        self.inspection = PhysicalInspection::Unavailable;
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
pub(super) fn capture(t: Phase, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: t.pc,
        phase_sr: t.during.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: t.direction,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}
fn transition(e: &mut Expected, s: Spec, b: &Device, t: Phase, vector: u8) {
    if !bulk_guards::known(t.ird) {
        e.unqualified(t, vector, true);
        return;
    }
    let cap = capture(t, vector);
    e.queue = None;
    e.state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
    match bulk_entry_oracle::group0(e, b, cap) {
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
pub(super) fn predict(s: Spec, b: &Device, p: &Plan) -> Expected {
    let mut e = Expected::new(s, b);
    if s.r.status_register() & 0x8000 != 0 {
        e.result = Err(InstructionFault::TraceUnqualified);
        return e;
    }
    if s.edge || s.level > ((s.r.status_register() >> 8) & 7) as u8 {
        e.result = Err(InstructionFault::InterruptUnqualified);
        return e;
    }
    if s.family == 0x46c0 && !s.r.supervisor() {
        e.result = Err(InstructionFault::Execution(
            TimingFault::UnsupportedInstruction {
                pc: s.r.pc,
                opcode: s.opcode(),
            },
        ));
        return e;
    }
    for action in &p.actions {
        match *action {
            Action::Publish(r) => e.r = r,
            Action::Idle(n) => {
                if !e.idle(n) {
                    return e;
                }
            }
            Action::Transfer(t) => {
                assert_eq!(e.r, t.before);
                if !e.reserve() {
                    return e;
                }
                e.r = t.during;
                if t.access.logical_address & 1 != 0 {
                    let (k, n) = if b.default_detector {
                        (Kind::Unavailable, 0)
                    } else {
                        b.detector.unwrap_or((Kind::Boundary, 8))
                    };
                    if !b.default_detector {
                        e.events.push(Event::Detect(
                            Stage::Instruction,
                            t.access,
                            t.direction,
                            e.clock,
                            n,
                            k,
                        ));
                    }
                    e.clock += u64::from(n);
                    match k {
                        Kind::Host => e.host(t),
                        Kind::Complete | Kind::Unavailable => e.unqualified(t, 3, false),
                        Kind::Boundary => transition(&mut e, s, b, t, 3),
                    }
                    return e;
                }
                if let Some((at, a, v)) = b.mutation
                    && at == e.count
                {
                    e.words.insert(a & 0x00ff_fffe, v);
                }
                let (k, n) = bulk_model::report(b, e.count);
                let deposited = t.direction == Direction::Write
                    && (k == Kind::Complete && n >= 4 || b.write_deposit);
                if deposited {
                    e.words.insert(t.access.bus_address, t.value);
                } else if t.direction == Direction::Read {
                    assert_eq!(
                        t.value,
                        e.words
                            .get(&t.access.bus_address)
                            .copied()
                            .unwrap_or(e.default_word)
                    );
                }
                e.events.push(Event::Transfer(
                    Stage::Instruction,
                    t.access,
                    t.direction,
                    t.value,
                    e.clock,
                    n,
                    k,
                    deposited,
                ));
                e.count += 1;
                e.clock += u64::from(n);
                match k {
                    Kind::Complete if n >= 4 => {}
                    Kind::Complete => {
                        e.guard = Some(TimingFault::Frozen);
                        e.result = Err(InstructionFault::Entry(
                            EntryFault::InvalidCompleteDuration { elapsed: n },
                        ));
                        return e;
                    }
                    Kind::Host => {
                        e.host(t);
                        return e;
                    }
                    Kind::Unavailable => {
                        e.unqualified(t, 2, false);
                        return e;
                    }
                    Kind::Boundary => {
                        transition(&mut e, s, b, t, 2);
                        return e;
                    }
                }
            }
        }
    }
    e.r = p.final_r;
    e.queue = Some(p.queue);
    e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
        opcode: s.opcode(),
        clocks: e.clock - s.start,
        exception: None,
    }));
    e
}
pub(super) fn freeze(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    fault: TimingFault<&'static str>,
) -> bool {
    let saved = cpu.clone();
    let device = b.clone();
    let mut copied = saved.clone();
    let mut copied_device = device.clone();
    let mut good = copied.step_physical_instruction(&mut copied_device)
        == Err(InstructionFault::Execution(fault.clone()));
    good &= copied == saved && copied_device == device;
    good &= cpu.step_physical_instruction(b) == Err(InstructionFault::Execution(fault.clone()));
    good &= *cpu == saved && *b == device;
    good &= cpu.step_instruction(b) == Err(fault.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.step_boundary(b) == Err(fault.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.prime(b) == Err(fault.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.service_pending_trace(b) == Err(fault);
    good &= *cpu == saved && *b == device;
    good
}
pub(super) fn verify(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    e: Expected,
    actual: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
) -> bool {
    let mut good = actual == e.result
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
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::atomic::{AtomicU32, Ordering};
            static BAD: AtomicU32 = AtomicU32::new(0);
            if BAD.fetch_add(1, Ordering::Relaxed) < 8 {
                eprintln!(
                    "status register bulk actual={actual:?} expected={:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} device={b:?} events={:?} state={:?}/{:?} inspection={:?}/{:?}",
                    e.result,
                    cpu.registers,
                    e.r,
                    cpu.queued_words(),
                    e.queue,
                    b.clock,
                    e.clock,
                    e.events,
                    cpu.physical_entry_state(),
                    e.state,
                    cpu.physical_inspection(),
                    e.inspection
                );
            }
        }
    }
    if let Some(f) = e.guard {
        good &= freeze(cpu, b, f);
    } else if let Err(error) = e.result
        && e.events.is_empty()
    {
        good &= bulk_guards::rejected(cpu, b, error);
    }
    good
}
pub(super) fn seeded(s: Spec, cpu: &mut PrefetchCpu, mut b: Device) -> bool {
    let p = bulk_plan::plan(s, &b);
    let e = predict(s, &b, &p);
    let actual = match (b.default_detector, b.default_entry_detector) {
        (false, false) => cpu.step_physical_instruction(&mut b),
        (true, false) => cpu.step_physical_instruction(&mut bulk_device::DefaultPrimary(&mut b)),
        (false, true) => cpu.step_physical_instruction(&mut bulk_device::DefaultSecondary(&mut b)),
        (true, true) => cpu.step_physical_instruction(&mut bulk_device::DefaultBoth(&mut b)),
    };
    verify(cpu, &mut b, e, actual)
}
pub(super) fn run(s: Spec, b: Device) -> bool {
    let mut cpu =
        PrefetchCpu::from_prefetched(s.r, [s.opcode(), s.queued()], CpuClocks(s.start)).unwrap();
    bulk_private::set_interrupts(&mut cpu, s.level, s.edge);
    seeded(s, &mut cpu, b)
}
