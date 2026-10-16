use super::*;
use model::{Case, Event, Image, Metadata, Outcome};

struct Reader {
    bytes: &'static [u8],
    at: usize,
}

impl Reader {
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let end = self.at.checked_add(N).unwrap();
        let value = self.bytes.get(self.at..end).unwrap().try_into().unwrap();
        self.at = end;
        value
    }
    fn u8(&mut self) -> u8 {
        self.bytes::<1>()[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.bytes())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes())
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.bytes())
    }
    fn flag(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            _ => panic!("literal bool"),
        }
    }
    fn text(&mut self) -> String {
        let len = usize::from(self.u16());
        let end = self.at.checked_add(len).unwrap();
        let value = std::str::from_utf8(self.bytes.get(self.at..end).unwrap())
            .unwrap()
            .to_owned();
        self.at = end;
        value
    }
    fn optional_u32(&mut self) -> Option<u32> {
        self.flag().then(|| self.u32())
    }
    fn registers(&mut self) -> Registers {
        let mut registers = Registers::new();
        for value in &mut registers.d {
            *value = self.u32();
        }
        for value in &mut registers.a {
            *value = self.u32();
        }
        registers.usp = self.u32();
        registers.ssp = self.u32();
        let sr = self.u16();
        registers.set_status_register(sr);
        assert_eq!(registers.status_register(), sr);
        registers.pc = self.u32();
        registers
    }
    fn image(&mut self) -> Image {
        let registers = self.registers();
        let queue = self.flag().then(|| [self.u16(), self.u16()]);
        let clock = self.u64();
        Image {
            registers,
            queue,
            clock,
        }
    }
    fn words(&mut self) -> Vec<(u32, u16)> {
        let count = self.u16();
        (0..count).map(|_| (self.u32(), self.u16())).collect()
    }
    fn access(&mut self) -> BusAccess {
        BusAccess {
            logical_address: self.u32(),
            bus_address: self.u32(),
            function_code: match self.u8() {
                1 => FunctionCode::UserData,
                2 => FunctionCode::UserProgram,
                5 => FunctionCode::SupervisorData,
                6 => FunctionCode::SupervisorProgram,
                _ => panic!("literal FC"),
            },
            kind: match self.u8() {
                0 => AccessKind::Data,
                1 => AccessKind::Prefetch,
                2 => AccessKind::Stack,
                3 => AccessKind::Vector,
                _ => panic!("literal access"),
            },
            lanes: ByteLanes::Both,
        }
    }
    fn direction(&mut self) -> Direction {
        match self.u8() {
            0 => Direction::Read,
            1 => Direction::Write,
            _ => panic!("literal direction"),
        }
    }
    fn outcome(&mut self) -> Outcome {
        match self.u8() {
            0 => Outcome::Complete,
            1 => Outcome::Boundary,
            2 => Outcome::Host,
            3 => Outcome::Unavailable,
            4 => Outcome::Critical,
            _ => panic!("literal outcome"),
        }
    }
    fn events(&mut self) -> Vec<Event> {
        let count = self.u16();
        (0..count)
            .map(|_| {
                let stage = self.u8();
                if stage == 4 {
                    return Event::Idle(self.u64(), self.u32());
                }
                assert!(stage < 4);
                let access = self.access();
                let direction = self.direction();
                let value = self.flag().then(|| self.u16());
                Event::Transfer {
                    stage,
                    access,
                    direction,
                    value,
                    start: self.u64(),
                    elapsed: self.u32(),
                    outcome: self.outcome(),
                    deposited: self.flag(),
                }
            })
            .collect()
    }
    fn cause(&mut self) -> CriticalFault {
        let address_error = self.flag();
        let access = self.access();
        let direction = self.direction();
        if address_error {
            CriticalFault::AddressError { access, direction }
        } else {
            CriticalFault::BusAbort { access, direction }
        }
    }
    fn capture(&mut self) -> (Group0Capture, u16) {
        let vector = self.u8();
        let internal_pc = self.u32();
        let phase_sr = self.u16();
        let ird = self.u16();
        let logical_address = self.u32();
        let function_code = match self.u8() {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("literal capture FC"),
        };
        let direction = self.direction();
        let ssw = self.u16();
        (
            Group0Capture {
                vector,
                internal_pc,
                phase_sr,
                ird,
                logical_address,
                function_code,
                direction,
                processing_class: ProcessingClass::InstructionOrGroup2,
                ssw_profile: SswProfile::SelectedIrdUpperBits,
            },
            ssw,
        )
    }
    fn phase(&mut self) -> InstructionPhase {
        match self.u8() {
            1 => InstructionPhase::ExtensionFirst,
            2 => InstructionPhase::ExtensionSecond,
            3 => InstructionPhase::OperandReadHigh,
            4 => InstructionPhase::OperandRefill,
            5 => InstructionPhase::OperandWriteHigh,
            _ => panic!("literal phase"),
        }
    }
    fn result(&mut self) -> model::Step {
        match self.u8() {
            0 => Ok(PhysicalInstructionStep::Instruction(TimedStep {
                opcode: self.u16(),
                clocks: self.u64(),
                exception: None,
            })),
            1 => Ok(PhysicalInstructionStep::Group0 {
                opcode: self.u16(),
                capture: self.capture().0,
                clocks: self.u64(),
            }),
            2 => Err(InstructionFault::Entry(EntryFault::ClockOverflow)),
            3 => Err(InstructionFault::Entry(EntryFault::HostFailure {
                access: self.access(),
                direction: self.direction(),
                source: "host",
            })),
            4 => Err(InstructionFault::TransitionTimingUnavailable {
                phase: self.phase(),
                cause: self.cause(),
            }),
            5 => Err(InstructionFault::Entry(EntryFault::DoubleFaultHalted(
                self.cause(),
            ))),
            _ => panic!("literal result"),
        }
    }
    fn state(&mut self) -> PhysicalEntryState {
        match self.u8() {
            0 => PhysicalEntryState::Idle,
            1 => PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
            2 => PhysicalEntryState::DoubleFaultHalted {
                cause: self.cause(),
                timing: TimingQualification::Qualified,
            },
            3 => PhysicalEntryState::FaultUnqualified {
                cause: self.cause(),
            },
            _ => panic!("literal state"),
        }
    }
    fn retry(&mut self) -> Option<TimingFault<&'static str>> {
        match self.u8() {
            0 => None,
            1 => Some(TimingFault::Frozen),
            2 => Some(TimingFault::PhysicalFaultUnqualified),
            3 => Some(TimingFault::DoubleFaultHalted),
            _ => panic!("literal guard"),
        }
    }
    fn case(&mut self) -> Case {
        let label = self.text();
        let metadata = Metadata {
            opcode: self.u16(),
            mode: self.u8(),
            register: self.u8(),
            supervisor: self.flag(),
            phase: self.u8(),
            disposition: self.u8(),
            failed_write_deposited: self.flag(),
            detector_only: self.flag(),
            terminal_phase: self.u8(),
        };
        let initial = self.image();
        let expected = self.image();
        let memory = self.words();
        let final_memory = self.words();
        let events = self.events();
        let result = self.result();
        let state = self.state();
        let inspection = if self.flag() {
            PhysicalInspection::Unavailable
        } else {
            PhysicalInspection::DiagnosticCompleteValues
        };
        let retry = self.retry();
        let frozen = self.flag();
        let queue_address = self.optional_u32();
        let capture = self.flag().then(|| self.capture());
        let capture_observable = self.flag();
        let writes = self.words();
        let frame = self.words();
        let before_entry = self.flag().then(|| (self.image(), self.words()));
        let entry_event = self.optional_u32().map(|v| v as usize);
        let counts = std::array::from_fn(|_| self.u32() as usize);
        let mutation_count = self.u16();
        let mutations = (0..mutation_count)
            .map(|_| (self.u32() as usize, self.u32(), self.u16()))
            .collect();
        Case {
            label,
            metadata,
            initial,
            expected,
            memory,
            final_memory,
            events,
            result,
            state,
            inspection,
            retry,
            frozen,
            queue_address,
            capture,
            capture_observable,
            writes,
            frame,
            before_entry,
            entry_event,
            counts,
            mutations,
            elapsed: self.u64(),
            pending_trace: self.flag(),
            stopped: self.flag(),
            interrupt_level: self.u8(),
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut reader = Reader {
        bytes: include_bytes!("first-literals.bin"),
        at: 0,
    };
    assert_eq!(reader.bytes::<4>(), *b"FSR1");
    let count = reader.u32();
    assert_eq!(count, 261);
    let cases = (0..count).map(|_| reader.case()).collect();
    assert_eq!(reader.at, reader.bytes.len());
    cases
}
