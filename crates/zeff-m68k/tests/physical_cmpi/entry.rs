use super::*;
use zeff_m68k::timing::physical::EntryKind;

pub(super) type Reports = (Option<(usize, Failure)>, Option<(Failure, u32)>);

pub(super) struct Prediction {
    pub words: Memory,
    pub events: Vec<Event>,
    pub registers: Registers,
    pub queue: Option<[u16; 2]>,
    pub end: u64,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub count: usize,
}

pub(super) fn predict(
    case: Case,
    t: oracle::Transfer,
    vector: u8,
    start: u64,
    words: Memory,
    reports: Reports,
) -> Prediction {
    let (secondary, odd_report) = reports;
    let c = oracle::capture(t, vector);
    let sp = t.registers.ssp;
    let mut p = Prediction {
        words,
        events: vec![Event::Idle(start, 4)],
        registers: t.registers,
        queue: None,
        end: start + 4,
        result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
        state: PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
        inspection: PhysicalInspection::DiagnosticCompleteValues,
        count: 0,
    };
    if sp & 1 != 0 {
        let a = oracle::access(sp.wrapping_sub(2), true, false, AccessKind::Stack);
        p.events
            .push(Event::EntryDetect(a, Direction::Write, p.end));
        let (failure, elapsed) = odd_report.unwrap_or((Failure::Unavailable, 0));
        p.end += u64::from(elapsed);
        p.terminal(a, Direction::Write, failure, true);
        return p;
    }
    let fc = match c.function_code {
        FunctionCode::UserData => 1,
        FunctionCode::UserProgram => 2,
        FunctionCode::SupervisorData => 5,
        FunctionCode::SupervisorProgram => 6,
        FunctionCode::CpuSpace => 7,
    };
    let ssw = (c.ird & 0xffe0) | fc | 16;
    let frame = [
        (2, c.internal_pc as u16),
        (6, c.phase_sr),
        (4, (c.internal_pc >> 16) as u16),
        (8, c.ird),
        (10, c.logical_address as u16),
        (14, ssw),
        (12, (c.logical_address >> 16) as u16),
    ];
    let mut vector_high = 0;
    let mut target = 0;
    let mut queue = [0; 2];
    for (i, word) in frame.into_iter().map(Some).chain([None; 4]).enumerate() {
        let (a, dir) = if let Some((delta, v)) = word {
            let a = oracle::access(sp.wrapping_sub(delta), true, false, AccessKind::Stack);
            p.events.push(Event::EntryWrite(a, v, p.end));
            p.words.insert(a.bus_address, v);
            (a, Direction::Write)
        } else {
            if i == 10 {
                p.events.push(Event::Idle(p.end, 2));
                p.end += 2;
            }
            let address = match i {
                7 => u32::from(vector) * 4,
                8 => u32::from(vector) * 4 + 2,
                9 => target,
                _ => target.wrapping_add(2),
            };
            let a = oracle::access(
                address,
                true,
                i >= 9,
                if i < 9 {
                    AccessKind::Vector
                } else {
                    AccessKind::Prefetch
                },
            );
            p.events.push(Event::EntryRead(a, p.end));
            let value = oracle::sample(&p.words, address);
            match i {
                7 => vector_high = u32::from(value) << 16,
                8 => target = vector_high | u32::from(value),
                9 => queue[0] = value,
                _ => queue[1] = value,
            };
            (a, Direction::Read)
        };
        p.count += 1;
        p.end += 4;
        if let Some((_, failure)) = secondary.filter(|(at, _)| *at == i) {
            p.terminal(a, dir, failure, false);
            return p;
        }
    }
    p.registers.ssp = sp.wrapping_sub(14);
    p.registers.pc = target;
    p.registers.set_status_register(c.phase_sr | SUPERVISOR);
    p.queue = Some(queue);
    p.result = Ok(PhysicalInstructionStep::Group0 {
        opcode: case.opcode(),
        capture: c,
        clocks: p.end - case.start,
    });
    p
}
impl Prediction {
    fn terminal(
        &mut self,
        a: BusAccess,
        direction: Direction,
        failure: Failure,
        address_error: bool,
    ) {
        let cause = if address_error {
            CriticalFault::AddressError {
                access: a,
                direction,
            }
        } else {
            CriticalFault::BusAbort {
                access: a,
                direction,
            }
        };
        match failure {
            Failure::Host => {
                self.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: a,
                    direction,
                    source: "host",
                }))
            }
            Failure::Boundary | Failure::Complete
                if address_error || failure == Failure::Boundary =>
            {
                self.result = Err(InstructionFault::Entry(EntryFault::DoubleFaultHalted(
                    cause,
                )));
                self.state = PhysicalEntryState::DoubleFaultHalted {
                    cause,
                    timing: TimingQualification::Qualified,
                };
                self.inspection = PhysicalInspection::Unavailable;
            }
            _ => {
                self.result = Err(InstructionFault::Entry(EntryFault::TimingUnavailable(
                    cause,
                )));
                self.state = PhysicalEntryState::DoubleFaultHalted {
                    cause,
                    timing: TimingQualification::Unavailable,
                };
                self.inspection = PhysicalInspection::Unavailable;
            }
        }
    }
}

pub(super) struct Probe<'a> {
    pub bus: &'a mut BusDevice,
    pub primary_default: bool,
    pub secondary: Option<(Failure, u32)>,
}
impl EntryBus for Probe<'_> {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.bus.read_entry(a, c)
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.bus.write_entry(a, v, c)
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.bus.idle_entry(c, n)
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        if let Some((kind, elapsed)) = self.secondary {
            assert_eq!(self.bus.clock, c.0);
            self.bus.events.push(Event::EntryDetect(a, d, c.0));
            self.bus.clock += u64::from(elapsed);
            EntryReport {
                elapsed,
                outcome: match kind {
                    Failure::Host => EntryOutcome::HostFailure("host"),
                    Failure::Boundary => EntryOutcome::CriticalAbortReleased,
                    Failure::Complete => EntryOutcome::Complete(()),
                    Failure::Unavailable => EntryOutcome::TimingUnavailable,
                },
            }
        } else {
            self.bus.detect_address_error(a, d, c)
        }
    }
}
impl InstructionBus for Probe<'_> {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.bus.read_instruction(a, c)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.bus.write_instruction(a, v, c)
    }
    fn detect_instruction_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        if self.primary_default {
            InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::TimingUnavailable,
            }
        } else {
            self.bus.detect_instruction_address_error(a, d, c)
        }
    }
}
