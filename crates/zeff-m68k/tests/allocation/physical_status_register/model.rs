use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Report {
    Complete,
    Host,
    Boundary,
    Unavailable,
    Short,
}

impl Report {
    pub(super) fn elapsed(self) -> u32 {
        match self {
            Self::Complete => 7,
            Self::Host => 6,
            Self::Boundary => 8,
            Self::Unavailable => 5,
            Self::Short => 3,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Path {
    Complete,
    Report(usize, Report),
    Blocked(usize),
    Idle(usize, u64),
}

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub(super) context: usize,
    pub(super) form: usize,
    pub(super) old_sr: u16,
    pub(super) source: u16,
}

impl Spec {
    pub(super) fn new(context: usize, form: usize, ccr: u16, mask: u16, source: u16) -> Self {
        Self {
            context,
            form,
            old_sr: (mask << 8) | ccr | if context == 0 { 0 } else { 0x2000 },
            source,
        }
    }

    pub(super) fn transfers(self) -> usize {
        if self.form == 8 { 3 } else { 2 }
    }

    pub(super) fn opcode(self) -> u16 {
        (if self.context == 2 { 0x46c0 } else { 0x44c0 })
            | if self.form == 8 { 60 } else { self.form as u16 }
    }

    pub(super) fn published(self) -> u16 {
        if self.context == 2 {
            self.source & 0xa71f
        } else {
            (self.old_sr & !31) | (self.source & 31)
        }
    }

    pub(super) fn input(self, path: Path) -> (Registers, [u16; 2], Ram, u64) {
        let mut r = Registers::new();
        r.pc = 0x2000;
        for (index, value) in r.d.iter_mut().enumerate() {
            *value = 0x1111_1111 * (index as u32 + 1);
        }
        for (index, value) in r.a.iter_mut().enumerate() {
            *value = 0x3000 + index as u32 * 0x1000;
        }
        let default_source = if self.context == 2 { 0x071f } else { 0x0312 };
        r.d[if self.form == 8 { 0 } else { self.form }] = 0x89ab_0000
            | u32::from(if self.form == 8 {
                default_source
            } else {
                self.source
            });
        r.usp = 0xb000;
        r.ssp = 0xa000;
        r.set_status_register(self.old_sr);
        let opcode = self.opcode();
        let mut ram = Ram([None; 32]);
        for (address, value) in [
            (8, 0),
            (10, 0x4000),
            (0x2000, opcode),
            (
                0x2002,
                if self.form == 8 {
                    default_source
                } else {
                    0x4e71
                },
            ),
            (0x2004, if self.form == 8 { 0x4e71 } else { 0x7201 }),
            (0x2006, 0x7201),
            (0x4000, 0x4e71),
            (0x4002, 0x7002),
        ] {
            ram.write(address, value);
        }
        let clock = match path {
            Path::Blocked(at) => {
                let prefix = if self.form == 8 {
                    if at == 0 {
                        0
                    } else {
                        4 + (at - 1) as u64 * 7 + 7
                    }
                } else {
                    4 + at as u64 * 7
                };
                u64::MAX - (BUDGET - 1) - prefix
            }
            Path::Idle(at, room) => {
                u64::MAX - room - at as u64 * 2 - if self.form == 8 { BUDGET } else { 0 }
            }
            Path::Complete | Path::Report(_, _) => 0,
        };
        (
            r,
            [opcode, if self.form == 8 { self.source } else { 0x7001 }],
            ram,
            clock,
        )
    }
}

pub(super) fn access(address: u32, sr: u16, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: if sr & 0x2000 == 0 {
            FunctionCode::UserProgram
        } else {
            FunctionCode::SupervisorProgram
        },
        kind,
        lanes: ByteLanes::Both,
    }
}

pub(super) struct Expected {
    pub(super) registers: Registers,
    pub(super) queue: Option<[u16; 2]>,
    pub(super) device: Device,
    pub(super) result: Result<PhysicalInstructionStep, InstructionFault<usize>>,
    pub(super) state: PhysicalEntryState,
    pub(super) inspection: PhysicalInspection,
}

impl Expected {
    fn idle(&mut self, clocks: u32) -> bool {
        let start = self.device.clock;
        if let Some(end) = start.checked_add(u64::from(clocks)) {
            self.device.events[self.device.count] = Some(Event::Idle(start, clocks));
            self.device.count += 1;
            self.device.clock = end;
            true
        } else {
            self.result = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
            false
        }
    }

    fn entry(&mut self, capture: Group0Capture) {
        self.queue = None;
        self.state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
        assert!(self.idle(4));
        let ssw = (capture.ird & 0xffe0) | 0x10 | capture.function_code as u16;
        for (offset, value) in [
            (2, capture.internal_pc as u16),
            (6, capture.phase_sr),
            (4, (capture.internal_pc >> 16) as u16),
            (8, capture.ird),
            (10, capture.logical_address as u16),
            (14, ssw),
            (12, (capture.logical_address >> 16) as u16),
        ] {
            let address = self.registers.ssp - offset;
            let mut a = access(address, 0x2000, AccessKind::Stack);
            a.function_code = FunctionCode::SupervisorData;
            self.device.ram.write(address, value);
            self.device.events[self.device.count] =
                Some(Event::Write(a, value, self.device.clock, 4));
            self.device.count += 1;
            self.device.entry_transfers += 1;
            self.device.clock += 4;
        }
        let high = self.entry_read(8, AccessKind::Vector);
        let low = self.entry_read(10, AccessKind::Vector);
        let target = (u32::from(high) << 16) | u32::from(low);
        let first = self.entry_read(target, AccessKind::Prefetch);
        assert!(self.idle(2));
        let second = self.entry_read(target + 2, AccessKind::Prefetch);
        self.registers.ssp -= 14;
        self.registers
            .set_status_register((capture.phase_sr | 0x2000) & !0x8000);
        self.registers.pc = target;
        self.queue = Some([first, second]);
    }

    fn entry_read(&mut self, address: u32, kind: AccessKind) -> u16 {
        let mut a = access(address, 0x2000, kind);
        if kind == AccessKind::Vector {
            a.function_code = FunctionCode::SupervisorData;
        }
        let value = self.device.ram.read(address);
        self.device.events[self.device.count] = Some(Event::Read(
            a,
            value,
            self.device.clock,
            4,
            Report::Complete,
        ));
        self.device.count += 1;
        self.device.entry_transfers += 1;
        self.device.clock += 4;
        value
    }
}

pub(super) fn predict(spec: Spec, path: Path) -> Expected {
    let (r, queue, ram, start) = spec.input(path);
    let opcode = spec.opcode();
    let anchor = 0x2000 + if spec.form == 8 { 4 } else { 2 };
    let mut expected = Expected {
        registers: r,
        queue: Some(queue),
        device: Device::new(ram, start, path, spec.form == 8),
        result: Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
        state: PhysicalEntryState::Idle,
        inspection: PhysicalInspection::DiagnosticCompleteValues,
    };
    let mut first = 0;
    for action in 0usize..if spec.form == 8 { 5 } else { 4 } {
        let relative = action.saturating_sub(usize::from(spec.form == 8));
        if (spec.form != 8 || action != 0) && relative < 2 {
            if !expected.idle(2) {
                return expected;
            }
            if relative == 0 {
                expected.registers.set_status_register(spec.published());
            }
            continue;
        }
        let at = expected.device.instruction_transfers;
        if expected.device.clock.checked_add(BUDGET).is_none() {
            return expected;
        }
        let phase = if spec.form == 8 && action == 0 {
            InstructionPhase::ImmediateFirst
        } else if relative == 2 {
            InstructionPhase::StatusFirst
        } else {
            InstructionPhase::StatusSecond
        };
        let address = if phase == InstructionPhase::StatusSecond {
            anchor + 2
        } else {
            anchor
        };
        let a = access(
            address,
            expected.registers.status_register(),
            AccessKind::Prefetch,
        );
        let value = expected.device.ram.read(address);
        let report = if let Path::Report(target, kind) = path {
            if target == at { kind } else { Report::Complete }
        } else {
            Report::Complete
        };
        let elapsed = if matches!(path, Path::Idle(_, _)) && spec.form == 8 && at == 0 {
            u32::MAX
        } else {
            report.elapsed()
        };
        expected.device.events[expected.device.count] = Some(Event::Read(
            a,
            value,
            expected.device.clock,
            elapsed,
            report,
        ));
        expected.device.count += 1;
        expected.device.instruction_transfers += 1;
        expected.device.clock += u64::from(elapsed);
        let cause = CriticalFault::BusAbort {
            access: a,
            direction: Direction::Read,
        };
        match report {
            Report::Host => {
                expected.queue = None;
                expected.result = Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: a,
                    direction: Direction::Read,
                    source: at,
                }));
                return expected;
            }
            Report::Short => {
                expected.result = Err(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration { elapsed: 3 },
                ));
                return expected;
            }
            Report::Unavailable => {
                expected.queue = None;
                expected.state = PhysicalEntryState::FaultUnqualified { cause };
                expected.inspection = PhysicalInspection::Unavailable;
                expected.result =
                    Err(InstructionFault::TransitionTimingUnavailable { phase, cause });
                return expected;
            }
            Report::Boundary => {
                let capture = Group0Capture {
                    vector: 2,
                    internal_pc: address,
                    phase_sr: expected.registers.status_register(),
                    ird: if phase == InstructionPhase::StatusSecond {
                        first
                    } else {
                        opcode
                    },
                    logical_address: address,
                    function_code: a.function_code,
                    direction: Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                };
                expected.entry(capture);
                expected.result = Ok(PhysicalInstructionStep::Group0 {
                    opcode,
                    capture,
                    clocks: expected.device.clock - start,
                });
                return expected;
            }
            Report::Complete => {}
        }
        if phase == InstructionPhase::StatusFirst {
            first = value;
        } else if phase == InstructionPhase::StatusSecond {
            expected.registers.pc = anchor;
            expected.queue = Some([first, value]);
        }
    }
    expected.result = Ok(PhysicalInstructionStep::Instruction(
        zeff_m68k::timing::TimedStep {
            opcode,
            clocks: expected.device.clock - start,
            exception: None,
        },
    ));
    expected
}
