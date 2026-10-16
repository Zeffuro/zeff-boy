use super::device::{Device, access};
use super::fixed::{Event, Events, Ram, Scripts};
use super::model::{Case, Failure};
use super::{
    AccessKind, BusAccess, CriticalFault, Direction, EntryFault, Group0Capture, InstructionFault,
    InstructionPhase, PhysicalEntryState, PhysicalInspection, PhysicalInstructionStep,
    ProcessingClass, Registers, SswProfile, TimedStep, TimingFault,
};

#[derive(Clone)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub pipeline_address: Option<u32>,
    pub clock: u64,
    pub words: Ram,
    pub events: Events,
    pub scripts: Scripts,
    pub count: usize,
    pub entries: usize,
    pub counts: [usize; 5],
    pub mutated: usize,
    pub before_entry: Option<(u64, Ram)>,
    pub level: u8,
    pub edge: bool,
    pub imported: Option<zeff_m68k::timing::physical::ImportedGroup0Boundary>,
    pub ordinary: [usize; 2],
    pub frozen: bool,
    pub stopped: bool,
    pub pending_trace: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub guard: Option<TimingFault<&'static str>>,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
    pub capture: Option<Group0Capture>,
    pub source: Option<u32>,
    pub compared: Option<u32>,
}
impl Expected {
    pub fn overflow(&mut self) {
        self.frozen = true;
        self.guard = Some(TimingFault::Frozen);
        self.result = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
    }
    pub fn reserve(&mut self) -> bool {
        if self.clock.checked_add(u64::from(u32::MAX)).is_none() {
            self.overflow();
            false
        } else {
            true
        }
    }
    pub fn idle(&mut self, clocks: u32) -> bool {
        if let Some(end) = self.clock.checked_add(u64::from(clocks)) {
            self.events.push(Event::Idle(self.clock, clocks));
            self.counts[4] += 1;
            self.clock = end;
            true
        } else {
            self.overflow();
            false
        }
    }
    fn mutate(&mut self, b: &Device, after: bool) {
        for m in b
            .mutations
            .into_iter()
            .flatten()
            .filter(|m| m.at == self.count && m.after == after)
        {
            self.words.insert(m.address, m.word);
            self.mutated += 1;
        }
    }
    fn read(
        &mut self,
        c: Case,
        b: &Device,
        a: BusAccess,
        capture: (u32, u16, InstructionPhase),
    ) -> Result<u16, ()> {
        let (pc, ird, phase) = capture;
        if !self.reserve() {
            return Err(());
        }
        self.mutate(b, false);
        let value = self.words.sample(a.bus_address);
        let report = b.primary[self.count];
        self.events.push(Event::Transfer {
            stage: 0,
            phase: Some(phase),
            access: a,
            direction: Direction::Read,
            value: Some(value),
            start: self.clock,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited: false,
        });
        self.scripts.push(0, report.kind, report.elapsed);
        self.mutate(b, true);
        self.count += 1;
        self.counts[0] += 1;
        self.clock += u64::from(report.elapsed);
        let cause = CriticalFault::BusAbort {
            access: a,
            direction: Direction::Read,
        };
        match report.kind {
            Failure::Complete if report.elapsed >= 4 => return Ok(value),
            Failure::Complete => {
                self.frozen = true;
                self.guard = Some(TimingFault::Frozen);
                self.result = Err(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration {
                        elapsed: report.elapsed,
                    },
                ));
            }
            Failure::Host => {
                self.frozen = true;
                self.queue = None;
                self.pipeline_address = None;
                self.guard = Some(TimingFault::Frozen);
                self.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: a,
                    direction: Direction::Read,
                    source: "host",
                }));
            }
            Failure::Unavailable | Failure::Boundary
                if report.kind == Failure::Unavailable || !super::classifier::known(ird) =>
            {
                self.queue = None;
                self.pipeline_address = None;
                self.state = PhysicalEntryState::FaultUnqualified { cause };
                self.inspection = PhysicalInspection::Unavailable;
                self.guard = Some(TimingFault::PhysicalFaultUnqualified);
                self.result = Err(if report.kind == Failure::Unavailable {
                    InstructionFault::TransitionTimingUnavailable { phase, cause }
                } else {
                    InstructionFault::CaptureUnavailable { phase, cause }
                });
            }
            Failure::Boundary => {
                let capture = Group0Capture {
                    vector: 2,
                    internal_pc: pc,
                    phase_sr: self.r.status_register(),
                    ird,
                    logical_address: a.logical_address,
                    function_code: a.function_code,
                    direction: Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                };
                self.capture = Some(capture);
                self.result = super::entry::group0(self, b, capture)
                    .map(|()| PhysicalInstructionStep::Group0 {
                        opcode: c.word,
                        capture,
                        clocks: self.clock - c.start,
                    })
                    .map_err(InstructionFault::Entry);
            }
            Failure::Unavailable => unreachable!(),
        }
        Err(())
    }
}
pub(super) fn initial(c: Case, b: &Device) -> Expected {
    Expected {
        r: c.r,
        queue: Some([c.word, c.retained]),
        pipeline_address: Some(c.r.pc),
        clock: c.start,
        words: b.words.clone(),
        events: b.events.clone(),
        scripts: b.scripts.clone(),
        count: b.count,
        entries: b.entry_count,
        counts: b.counts,
        mutated: b.mutated,
        before_entry: b.before_entry.clone(),
        level: 0,
        edge: false,
        imported: None,
        ordinary: b.ordinary,
        frozen: false,
        stopped: false,
        pending_trace: false,
        state: PhysicalEntryState::Idle,
        inspection: PhysicalInspection::DiagnosticCompleteValues,
        guard: None,
        result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
        capture: None,
        source: None,
        compared: None,
    }
}
pub(super) fn predict(c: Case, b: &Device) -> Expected {
    let mut e = initial(c, b);
    let p = c.p;
    let pc = c.r.pc;
    let bank = c.r.supervisor();
    if p.idle() && !e.idle(2) {
        return e;
    }
    let mut stream = c.retained;
    let mut extensions = [0u16; 2];
    for (at, slot) in extensions.iter_mut().enumerate().take(p.extensions()) {
        *slot = stream;
        let address = pc.wrapping_add(4 + at as u32 * 2);
        let capture_pc = if p.immediate() {
            if p.bits == 32 && at == 0 {
                pc.wrapping_add(2)
            } else {
                pc.wrapping_add(4 + at as u32 * 2)
            }
        } else if at == 1 {
            pc.wrapping_add(6)
        } else if p.mode == 7 && p.register == 0 {
            pc.wrapping_add(4)
        } else {
            pc.wrapping_add(2)
        };
        match e.read(
            c,
            b,
            access(address, bank, true, AccessKind::Prefetch, false),
            (capture_pc, c.word, p.phase(at)),
        ) {
            Ok(value) => stream = value,
            Err(()) => return e,
        }
    }
    let raw = if p.memory() {
        let mut source_case = c;
        source_case.retained = extensions[0];
        if p.extensions() == 2 {
            source_case.low = extensions[1];
        }
        let address = super::model::address(source_case, &e.r);
        if matches!(p.mode, 3 | 4) {
            if !e.reserve() {
                return e;
            }
            e.r.set_address_register(
                p.register,
                if p.mode == 3 {
                    address.wrapping_add(p.step())
                } else {
                    address
                },
            );
        }
        let capture_pc = if p.mode == 7 && p.register == 1 {
            pc.wrapping_add(6)
        } else if p.mode == 4 || p.mode == 7 && p.register == 0 {
            pc.wrapping_add(4)
        } else {
            pc.wrapping_add(2)
        };
        match e.read(
            c,
            b,
            access(
                address,
                bank,
                p.mode == 7 && matches!(p.register, 2 | 3),
                AccessKind::Data,
                true,
            ),
            (capture_pc, c.word, InstructionPhase::OperandReadHigh),
        ) {
            Ok(value) => {
                if address & 1 == 0 {
                    u32::from(value >> 8)
                } else {
                    u32::from(value & 255)
                }
            }
            Err(()) => return e,
        }
    } else if p.immediate() {
        if p.bits == 32 {
            u32::from(extensions[0]) << 16 | u32::from(extensions[1])
        } else {
            u32::from(extensions[0])
        }
    } else if p.mode == 0 {
        e.r.d[p.register]
    } else {
        e.r.address_register(p.register).unwrap()
    };
    let source = if p.cmpa && p.bits == 16 {
        i32::from(raw as i16) as u32
    } else if p.bits == 32 {
        raw
    } else {
        raw & ((1u32 << p.bits) - 1)
    };
    let destination = if p.cmpa {
        e.r.address_register(p.destination).unwrap()
    } else {
        e.r.d[p.destination]
    };
    if !e.reserve() {
        return e;
    }
    e.source = Some(source);
    e.compared = Some(destination);
    e.r.set_status_register(super::model::flags(
        e.r.status_register(),
        source,
        destination,
        if p.cmpa { 32 } else { p.bits },
    ));
    let address = pc.wrapping_add(p.length() + 2);
    let last = match e.read(
        c,
        b,
        access(address, bank, true, AccessKind::Prefetch, false),
        (
            address,
            if p.cmpa || p.bits == 32 {
                c.word
            } else {
                stream
            },
            InstructionPhase::OperandRefill,
        ),
    ) {
        Ok(value) => value,
        Err(()) => return e,
    };
    if p.tail() && !e.idle(2) {
        return e;
    }
    e.r.pc = pc.wrapping_add(p.length());
    e.queue = Some([stream, last]);
    e.pipeline_address = Some(e.r.pc);
    e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
        opcode: c.word,
        clocks: e.clock - c.start,
        exception: None,
    }));
    e
}
