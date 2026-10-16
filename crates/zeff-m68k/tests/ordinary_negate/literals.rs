use super::*;
use AccessKind::{Data, Prefetch};
use Direction::{Read, Write};
use device::Event;
use fixture::{Case, Expected, Outcome, Stage, access, state};

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
            profile: self.text(),
            route: self.text(),
            opcode: self.value(),
            size: self.value(),
            source: self.value(),
            result: self.value(),
            low_result: self.value(),
            partial_sr: self.value(),
            full_sr: self.value(),
            original_x: self.value(),
            published_low_x: self.value(),
            start: self.value(),
            waits: (0..self.number()).map(|_| self.value()).collect(),
            fail: match self.number() {
                0 => None,
                1 => Some(self.value()),
                _ => panic!("literal tag"),
            },
            failed_write_deposits: match self.number() {
                0 => None,
                1 => Some(self.flag()),
                _ => panic!("literal tag"),
            },
            blocked: match self.number() {
                0 => Some(self.value()),
                1 => None,
                _ => panic!("literal tag"),
            },
            initial: state(
                self.value(),
                self.value(),
                self.value(),
                self.value(),
                self.value(),
            ),
            queue: std::array::from_fn(|_| self.value()),
            queue_address: self.value(),
            ram: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            events: (0..self.number())
                .map(|_| Event {
                    access: access(
                        self.value(),
                        self.value(),
                        self.value(),
                        match self.number() {
                            0 => Data,
                            1 => Prefetch,
                            _ => panic!("literal tag"),
                        },
                    ),
                    at: self.value(),
                    value: self.value(),
                    direction: match self.number() {
                        0 => Read,
                        1 => Write,
                        _ => panic!("literal tag"),
                    },
                    wait: self.value(),
                    elapsed: self.value(),
                    host: self.flag(),
                    deposited: self.flag(),
                })
                .collect(),
            stages: (0..self.number())
                .map(|_| Stage {
                    phase: self.text(),
                    sr: self.value(),
                    source_an: self.value(),
                    observation: self.text(),
                })
                .collect(),
            expected: Expected {
                r: state(
                    self.value(),
                    self.value(),
                    self.value(),
                    self.value(),
                    self.value(),
                ),
                queue: std::array::from_fn(|_| self.value()),
                queue_address: self.value(),
                ram: (0..self.number())
                    .map(|_| (self.value(), self.value()))
                    .collect(),
                clock: self.value(),
                duration: self.value(),
                outcome: match self.number() {
                    0 => Outcome::Overflow,
                    1 => Outcome::Host(
                        access(
                            self.value(),
                            self.value(),
                            self.value(),
                            match self.number() {
                                0 => Prefetch,
                                1 => Data,
                                _ => panic!("literal tag"),
                            },
                        ),
                        match self.number() {
                            0 => Read,
                            1 => Write,
                            _ => panic!("literal tag"),
                        },
                        self.value(),
                    ),
                    2 => Outcome::Complete,
                    _ => panic!("literal tag"),
                },
                frozen: self.flag(),
                trace: self.flag(),
                stopped: self.flag(),
                state: PhysicalEntryState::Idle,
                inspection: PhysicalInspection::DiagnosticCompleteValues,
                interrupt_state: self.text(),
                retry: match self.number() {
                    0 => Some(self.text()),
                    1 => None,
                    _ => panic!("literal tag"),
                },
            },
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut reader = Reader {
        bytes: include_bytes!("literals.bin"),
        at: 4,
    };
    assert_eq!(&reader.bytes[..4], b"ONG1");
    let count = reader.number();
    assert_eq!(count, 432);
    let cases = (0..count).map(|_| reader.case()).collect();
    assert_eq!(reader.at, reader.bytes.len());
    cases
}
