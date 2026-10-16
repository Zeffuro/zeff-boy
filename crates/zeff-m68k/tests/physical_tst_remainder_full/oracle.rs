use super::*;
use device::{Device, Event, Kind, Stage};
use model::{Spec, access, status};
#[derive(Clone)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
    pub words: Ram,
    pub default_word: u16,
    pub events: Events,
    pub count: usize,
    pub entries: usize,
    pub level: u8,
    pub edge: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub guard: Option<TimingFault<&'static str>>,
    pub result: Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
    pub mutation_count: usize,
    pub scripts: Scripts,
    pub omitted_detectors: usize,
}
impl Expected {
    pub fn new(r: Registers, q: [u16; 2], b: &Device) -> Self {
        Self {
            r,
            queue: Some(q),
            clock: b.clock,
            words: b.words.clone(),
            default_word: 0,
            events: Events::default(),
            count: 0,
            entries: 0,
            level: 0,
            edge: false,
            state: PhysicalEntryState::Idle,
            inspection: PhysicalInspection::DiagnosticCompleteValues,
            guard: None,
            result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
            mutation_count: 0,
            scripts: Scripts::default(),
            omitted_detectors: 0,
        }
    }
    pub fn overflow(&mut self) {
        self.guard = Some(TimingFault::Frozen);
        self.result = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
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
}
pub(super) fn predict(s: Spec, b: &Device) -> Expected {
    let (r, q, _, ext, mut ea) = s.seed();
    let mut e = Expected::new(r, q, b);
    let mut retained = q[1];
    let mut value = if s.mode() == 0 {
        r.d[s.reg()]
            & if s.bits() == 32 {
                u32::MAX
            } else {
                (1 << s.bits()) - 1
            }
    } else {
        0
    };
    if s.idle() != 0 && !e.idle(2) {
        return e;
    }
    for at in 0..s.count() {
        if !e.reserve() {
            return e;
        }
        let is_f = at == s.count() - 1;
        let is_rh = s.mode() != 0 && at == s.extensions();
        if is_rh && matches!(s.mode(), 3 | 4) {
            model::set_address(
                &mut e.r,
                s.reg(),
                if s.mode() == 3 {
                    ea.wrapping_add(if s.reg() == 7 { 2 } else { 1 })
                } else {
                    ea
                },
            );
        }
        if is_f {
            e.r = status(
                e.r,
                (e.r.status_register() & 0xfff0)
                    | if value & (1 << (s.bits() - 1)) != 0 {
                        8
                    } else {
                        0
                    }
                    | if value == 0 { 4 } else { 0 },
            );
        }
        if let Some((i, a, v)) = b.mutation
            && i == at
        {
            e.words.insert(a & BUS_ADDRESS_MASK & !1, v);
            e.mutation_count += 1;
        }
        let address = if is_f {
            s.anchor().wrapping_add(2)
        } else if is_rh {
            ea
        } else {
            s.pc.wrapping_add(4 + at as u32 * 2)
        };
        let mut a = access(
            address,
            if is_rh {
                if s.bank { 5 } else { 1 }
            } else if s.bank {
                6
            } else {
                2
            },
            if is_rh {
                AccessKind::Data
            } else {
                AccessKind::Prefetch
            },
        );
        if is_rh {
            a.lanes = if address & 1 != 0 {
                ByteLanes::Lower
            } else {
                ByteLanes::Upper
            };
        }
        let v = e.words.get(&a.bus_address).copied().unwrap_or(0);
        let (k, n) = model::report(b, at);
        e.events.push(Event::Transfer(
            Stage::Instruction,
            a,
            Direction::Read,
            v,
            e.clock,
            n,
            k,
            false,
        ));
        e.scripts.push((Stage::Instruction, k, n));
        e.count += 1;
        e.clock += u64::from(n);
        let phase = if is_f {
            InstructionPhase::OperandRefill
        } else if is_rh {
            InstructionPhase::OperandReadHigh
        } else if at == 1 {
            InstructionPhase::ExtensionSecond
        } else {
            InstructionPhase::ExtensionFirst
        };
        let internal_pc = if is_f {
            s.anchor().wrapping_add(2)
        } else if at == 1 && !is_rh || is_rh && s.opcode == 0x4a39 {
            s.pc.wrapping_add(6)
        } else if s.opcode == 0x4a38 || is_rh && s.mode() == 4 {
            s.pc.wrapping_add(4)
        } else {
            s.pc.wrapping_add(2)
        };
        let ird = if is_f { retained } else { s.opcode };
        match k {
            Kind::Complete if n >= 4 => {
                if is_f {
                    e.r.pc = s.anchor();
                    e.queue = Some([retained, v]);
                    e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
                        opcode: s.opcode,
                        clocks: e.clock - s.start,
                        exception: None,
                    }));
                } else if is_rh {
                    value = u32::from(if address & 1 != 0 { v & 255 } else { v >> 8 });
                } else if s.opcode == 0x4a39 && at == 0 {
                    ea = u32::from(ext[0]) << 16 | u32::from(v);
                } else {
                    retained = v;
                }
            }
            Kind::Complete => {
                e.guard = Some(TimingFault::Frozen);
                e.result = Err(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration { elapsed: n },
                ));
                return e;
            }
            Kind::Host => {
                e.queue = None;
                e.guard = Some(TimingFault::Frozen);
                e.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: a,
                    direction: Direction::Read,
                    source: "host",
                }));
                return e;
            }
            Kind::Unavailable | Kind::Boundary => {
                e.queue = None;
                let cause = CriticalFault::BusAbort {
                    access: a,
                    direction: Direction::Read,
                };
                if k == Kind::Unavailable || !classifier::known(ird) {
                    e.state = PhysicalEntryState::FaultUnqualified { cause };
                    e.inspection = PhysicalInspection::Unavailable;
                    e.guard = Some(TimingFault::PhysicalFaultUnqualified);
                    e.result = Err(if k == Kind::Unavailable {
                        InstructionFault::TransitionTimingUnavailable { phase, cause }
                    } else {
                        InstructionFault::CaptureUnavailable { phase, cause }
                    });
                } else {
                    let cap = Group0Capture {
                        vector: 2,
                        internal_pc,
                        phase_sr: e.r.status_register(),
                        ird,
                        logical_address: address,
                        function_code: a.function_code,
                        direction: Direction::Read,
                        processing_class: ProcessingClass::InstructionOrGroup2,
                        ssw_profile: SswProfile::SelectedIrdUpperBits,
                    };
                    e.state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
                    e.edge = false;
                    e.result = match entry_oracle::group0(&mut e, b, cap) {
                        Ok(()) => Ok(PhysicalInstructionStep::Group0 {
                            opcode: s.opcode,
                            capture: cap,
                            clocks: e.clock - s.start,
                        }),
                        Err(error) => Err(InstructionFault::Entry(error)),
                    };
                }
                return e;
            }
        }
    }
    e
}
