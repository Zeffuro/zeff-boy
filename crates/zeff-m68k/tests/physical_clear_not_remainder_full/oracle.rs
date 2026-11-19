use super::device::{Device, access};
use super::fixed::{Event, Events, Ram, Scripts};
use super::model::{Case, Failure};
use super::{
    AccessKind, BusAccess, ByteLanes, CriticalFault, Direction, EntryFault, Group0Capture,
    InstructionFault, InstructionPhase, PhysicalEntryState, PhysicalInspection,
    PhysicalInstructionStep, ProcessingClass, Registers, SswProfile, TimedStep, TimingFault,
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
    pub result_value: Option<u32>,
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
    fn transfer(
        &mut self,
        c: Case,
        b: &Device,
        a: BusAccess,
        write: Option<u16>,
        capture: (u32, u16, InstructionPhase),
    ) -> Result<u16, ()> {
        let (pc, ird, phase) = capture;
        if !self.reserve() {
            return Err(());
        }
        self.mutate(b, false);
        let direction = if write.is_some() {
            Direction::Write
        } else {
            Direction::Read
        };
        let value = write.unwrap_or_else(|| self.words.sample(a.bus_address));
        let report = b.primary[self.count];
        let deposited =
            write.is_some() && (report.kind == Failure::Complete || b.primary_deposit[self.count]);
        if deposited {
            let old = self.words.sample(a.bus_address);
            let word = match a.lanes {
                ByteLanes::Upper => old & 0x00ff | value & 0xff00,
                ByteLanes::Lower => old & 0xff00 | value & 0x00ff,
                ByteLanes::Both => value,
            };
            self.words.insert(a.bus_address, word);
        }
        self.events.push(Event::Transfer {
            stage: 0,
            phase: Some(phase),
            access: a,
            direction,
            value: Some(value),
            start: self.clock,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited,
        });
        self.scripts.push(0, report.kind, report.elapsed);
        self.mutate(b, true);
        self.count += 1;
        self.counts[0] += 1;
        self.clock += u64::from(report.elapsed);
        let cause = CriticalFault::BusAbort {
            access: a,
            direction,
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
                    direction,
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
                    direction,
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
        source: if c.p.memory() {
            None
        } else {
            Some(c.r.d[c.p.register] & c.p.mask())
        },
        result_value: None,
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
        let capture_pc = if at == 1 {
            pc.wrapping_add(6)
        } else if p.mode == 7 && p.register == 0 {
            pc.wrapping_add(4)
        } else {
            pc.wrapping_add(2)
        };
        match e.transfer(
            c,
            b,
            access(address, bank, true, AccessKind::Prefetch, false),
            None,
            (capture_pc, c.word, p.phase(at)),
        ) {
            Ok(value) => stream = value,
            Err(()) => return e,
        }
    }
    let saved_address = if p.memory() {
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
        let value = match e.transfer(
            c,
            b,
            access(address, bank, false, AccessKind::Data, true),
            None,
            (capture_pc, c.word, InstructionPhase::OperandReadHigh),
        ) {
            Ok(value) => value,
            Err(()) => return e,
        };
        e.source = Some(u32::from(if address & 1 == 0 {
            value >> 8
        } else {
            value & 255
        }));
        Some(address)
    } else {
        None
    };
    if !e.reserve() {
        return e;
    }
    let result = p.result(e.source.unwrap());
    e.result_value = Some(result);
    e.r.set_status_register(super::model::flags(e.r.status_register(), result, p.bits));
    if !p.memory() {
        let mask = if p.tail() { 65535 } else { p.mask() };
        e.r.d[p.register] = e.r.d[p.register] & !mask | result & mask;
    }
    let address = pc.wrapping_add(p.length() + 2);
    let last = match e.transfer(
        c,
        b,
        access(address, bank, true, AccessKind::Prefetch, false),
        None,
        (
            address,
            if p.memory() || p.tail() {
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
    if let Some(saved_address) = saved_address {
        let value = if saved_address & 1 == 0 {
            (result as u16) << 8
        } else {
            result as u16
        };
        if e.transfer(
            c,
            b,
            access(saved_address, bank, false, AccessKind::Data, true),
            Some(value),
            (address, stream, InstructionPhase::OperandWriteHigh),
        )
        .is_err()
        {
            return e;
        }
    }
    if p.tail() {
        e.r.d[p.register] = result;
        if !e.idle(2) {
            return e;
        }
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
