use super::*;
#[derive(Clone)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
    pub words: Ram,
    pub events: Events,
    pub scripts: Scripts,
    pub count: usize,
    pub entries: usize,
    pub mutations: usize,
    pub level: u8,
    pub edge: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub guard: Option<TimingFault<&'static str>>,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
}
impl Expected {
    pub fn reserve(&mut self) -> bool {
        if self.clock.checked_add(u64::from(u32::MAX)).is_some() {
            true
        } else {
            self.overflow();
            false
        }
    }
    pub fn overflow(&mut self) {
        self.guard = Some(TimingFault::Frozen);
        self.result = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
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
}
pub(super) fn predict(c: Case, b: &Device) -> Expected {
    let mut e = Expected {
        r: c.r,
        queue: Some([c.opcode(), c.retained()]),
        clock: c.start,
        words: b.words.clone(),
        events: Events::default(),
        scripts: Scripts::default(),
        count: 0,
        entries: 0,
        mutations: 0,
        level: 0,
        edge: false,
        state: PhysicalEntryState::Idle,
        inspection: PhysicalInspection::DiagnosticCompleteValues,
        guard: None,
        result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
    };
    let memory = b.words.memory();
    let mut plan = super::super::oracle::plan(c, &memory);
    for at in 0..plan.len() {
        if c.form.idle() && at == c.form.immediates() && !e.idle(2) {
            return e;
        }
        if !e.reserve() {
            return e;
        }
        let t = plan[at];
        e.r = t.registers;
        if let Some((address, value)) = b
            .mutation
            .filter(|(address, _)| *address == t.access.logical_address)
        {
            e.words.insert(address, value);
            e.mutations += 1;
            if t.phase == InstructionPhase::OperandReadHigh {
                let updated = super::super::oracle::plan(c, &e.words.memory());
                let last = plan.len() - 1;
                plan[last].registers = updated[last].registers;
            }
        }
        let kind = b
            .failure
            .filter(|(i, _)| *i == at)
            .map_or(Failure::Complete, |(_, k)| k);
        let elapsed = b.elapsed[at];
        e.events.push(Event::Read(t.access, e.clock));
        e.scripts.push(false, kind, elapsed);
        e.count += 1;
        e.clock += u64::from(elapsed);
        if kind == Failure::Complete && elapsed >= 4 {
            continue;
        }
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: Direction::Read,
        };
        e.guard = Some(TimingFault::Frozen);
        e.result = Err(match kind {
            Failure::Complete => {
                InstructionFault::Entry(EntryFault::InvalidCompleteDuration { elapsed })
            }
            Failure::Host => {
                e.queue = None;
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: Direction::Read,
                    source: "host",
                })
            }
            Failure::Boundary | Failure::Unavailable => {
                e.queue = None;
                if kind == Failure::Boundary && super::super::oracle::known(t.ird) {
                    e.state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
                    e.guard = None;
                    let cap = super::super::oracle::capture(t, 2);
                    let result = entry::group0(&mut e, b, cap);
                    e.result = result
                        .map(|()| PhysicalInstructionStep::Group0 {
                            opcode: c.opcode(),
                            capture: cap,
                            clocks: e.clock - c.start,
                        })
                        .map_err(InstructionFault::Entry);
                    return e;
                }
                e.state = PhysicalEntryState::FaultUnqualified { cause };
                e.inspection = PhysicalInspection::Unavailable;
                e.guard = Some(TimingFault::PhysicalFaultUnqualified);
                if kind == Failure::Boundary {
                    InstructionFault::CaptureUnavailable {
                        phase: t.phase,
                        cause,
                    }
                } else {
                    InstructionFault::TransitionTimingUnavailable {
                        phase: t.phase,
                        cause,
                    }
                }
            }
        });
        return e;
    }
    if c.form.tail() && !e.idle(2) {
        return e;
    }
    e.r.pc = c.r.pc.wrapping_add(c.form.length());
    e.queue = Some([
        super::super::oracle::next(c, &memory),
        e.words.sample(e.r.pc.wrapping_add(2)),
    ]);
    e.result = Ok(PhysicalInstructionStep::Instruction(
        zeff_m68k::timing::TimedStep {
            opcode: c.opcode(),
            clocks: e.clock - c.start,
            exception: None,
        },
    ));
    e
}
