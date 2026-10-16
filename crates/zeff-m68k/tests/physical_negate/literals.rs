use super::*;
use model::{Case, Event, access, capture, event, image};

struct Reader {
    bytes: &'static [u8],
    at: usize,
}

impl Reader {
    fn number(&mut self) -> u64 {
        let mut value = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.bytes[self.at];
            self.at += 1;
            assert!(shift < 63 || byte <= 1);
            value |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return value;
            }
        }
        panic!("literal integer")
    }
    fn flag(&mut self) -> bool {
        match self.number() {
            0 => false,
            1 => true,
            _ => panic!("literal bool"),
        }
    }
    fn value<T: TryFrom<u64>>(&mut self) -> T {
        self.number().try_into().ok().unwrap()
    }
    fn text(&mut self) -> &'static str {
        let len = usize::try_from(self.number()).unwrap();
        let end = self.at.checked_add(len).unwrap();
        let text = std::str::from_utf8(self.bytes.get(self.at..end).unwrap()).unwrap();
        self.at = end;
        text
    }
    fn case(&mut self) -> Case {
        Case {
            label: self.text(),
            initial: image(
                std::array::from_fn(|_| self.value()),
                Some(std::array::from_fn(|_| self.value())),
                self.value(),
            ),
            expected: image(
                std::array::from_fn(|_| self.value()),
                match self.number() {
                    0 => Some(std::array::from_fn(|_| self.value())),
                    1 => None,
                    _ => panic!("literal tag"),
                },
                self.value(),
            ),
            memory: memory::get(self.value()),
            final_memory: memory::get(self.value()),
            events: (0..self.number())
                .map(|_| match self.number() {
                    0 => event(std::array::from_fn(|_| self.value())),
                    1 => Event::Idle(self.value(), self.value()),
                    _ => panic!("literal tag"),
                })
                .collect(),
            result: match self.number() {
                0 => Ok(match self.number() {
                    0 => PhysicalInstructionStep::Instruction(TimedStep {
                        opcode: self.value(),
                        clocks: self.value(),
                        exception: None,
                    }),
                    1 => PhysicalInstructionStep::Group0 {
                        opcode: self.value(),
                        capture: (capture(
                            std::array::from_fn(|_| self.value()),
                            match self.number() {
                                0 => Direction::Read,
                                1 => Direction::Write,
                                _ => panic!("literal tag"),
                            },
                        ))
                        .0,
                        clocks: self.value(),
                    },
                    _ => panic!("literal tag"),
                }),
                1 => Err(match self.number() {
                    0 => InstructionFault::Entry(match self.number() {
                        0 => EntryFault::ClockOverflow,
                        1 => EntryFault::HostFailure {
                            access: access(std::array::from_fn(|_| self.value())),
                            direction: match self.number() {
                                0 => Direction::Read,
                                1 => Direction::Write,
                                _ => panic!("literal tag"),
                            },
                            source: self.text(),
                        },
                        2 => EntryFault::DoubleFaultHalted(CriticalFault::AddressError {
                            access: access(std::array::from_fn(|_| self.value())),
                            direction: Direction::Write,
                        }),
                        _ => panic!("literal tag"),
                    }),
                    1 => InstructionFault::TransitionTimingUnavailable {
                        phase: match self.number() {
                            0 => InstructionPhase::OperandRefill,
                            1 => InstructionPhase::OperandWriteHigh,
                            2 => InstructionPhase::OperandWriteLow,
                            3 => InstructionPhase::OperandReadHigh,
                            _ => panic!("literal tag"),
                        },
                        cause: match self.number() {
                            0 => CriticalFault::BusAbort {
                                access: access(std::array::from_fn(|_| self.value())),
                                direction: match self.number() {
                                    0 => Direction::Read,
                                    1 => Direction::Write,
                                    _ => panic!("literal tag"),
                                },
                            },
                            1 => CriticalFault::AddressError {
                                access: access(std::array::from_fn(|_| self.value())),
                                direction: Direction::Read,
                            },
                            _ => panic!("literal tag"),
                        },
                    },
                    _ => panic!("literal tag"),
                }),
                _ => panic!("literal tag"),
            },
            state: match self.number() {
                0 => PhysicalEntryState::Idle,
                1 => PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
                2 => PhysicalEntryState::FaultUnqualified {
                    cause: match self.number() {
                        0 => CriticalFault::BusAbort {
                            access: access(std::array::from_fn(|_| self.value())),
                            direction: match self.number() {
                                0 => Direction::Read,
                                1 => Direction::Write,
                                _ => panic!("literal tag"),
                            },
                        },
                        1 => CriticalFault::AddressError {
                            access: access(std::array::from_fn(|_| self.value())),
                            direction: Direction::Read,
                        },
                        _ => panic!("literal tag"),
                    },
                },
                3 => PhysicalEntryState::DoubleFaultHalted {
                    cause: CriticalFault::AddressError {
                        access: access(std::array::from_fn(|_| self.value())),
                        direction: Direction::Write,
                    },
                    timing: TimingQualification::Qualified,
                },
                _ => panic!("literal tag"),
            },
            inspection: match self.number() {
                0 => PhysicalInspection::DiagnosticCompleteValues,
                1 => PhysicalInspection::Unavailable,
                _ => panic!("literal tag"),
            },
            retry: match self.number() {
                0 => None,
                1 => Some(match self.number() {
                    0 => TimingFault::Frozen,
                    1 => TimingFault::PhysicalFaultUnqualified,
                    2 => TimingFault::DoubleFaultHalted,
                    _ => panic!("literal tag"),
                }),
                _ => panic!("literal tag"),
            },
            frozen: self.flag(),
            queue_address: match self.number() {
                0 => Some(self.value()),
                1 => None,
                _ => panic!("literal tag"),
            },
            capture: match self.number() {
                0 => None,
                1 => Some(capture(
                    std::array::from_fn(|_| self.value()),
                    match self.number() {
                        0 => Direction::Read,
                        1 => Direction::Write,
                        _ => panic!("literal tag"),
                    },
                )),
                _ => panic!("literal tag"),
            },
            capture_observable: self.flag(),
            writes: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            frame: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            before_entry: match self.number() {
                0 => None,
                1 => Some((self.value(), memory::get(self.value()))),
                _ => panic!("literal tag"),
            },
            counts: std::array::from_fn(|_| self.value()),
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut reader = Reader {
        bytes: include_bytes!("literals.bin"),
        at: 4,
    };
    assert_eq!(&reader.bytes[..4], b"PNG1");
    let count = reader.number();
    assert_eq!(count, 536);
    let cases = (0..count).map(|_| reader.case()).collect();
    assert_eq!(reader.at, reader.bytes.len());
    cases
}
