use super::*;
use AccessKind::{Data, Prefetch};
use Direction::{Read, Write};
use device::Event;
use fixture::{Case, Outcome, access, state};

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
            opcode: self.value(),
            start: self.value(),
            initial: state(
                self.value(),
                self.value(),
                self.value(),
                self.value(),
                self.value(),
            ),
            queue: std::array::from_fn(|_| self.value()),
            ram: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            waits: (0..self.number()).map(|_| self.value()).collect(),
            fail: match self.number() {
                0 => None,
                1 => Some(self.value()),
                _ => panic!("literal tag"),
            },
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
            final_registers: state(
                self.value(),
                self.value(),
                self.value(),
                self.value(),
                self.value(),
            ),
            final_queue: std::array::from_fn(|_| self.value()),
            final_ram: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            clock: self.value(),
            duration: self.value(),
            events: (0..self.number())
                .map(|_| match self.number() {
                    0 => Event::Read(
                        access(
                            self.value(),
                            self.value(),
                            self.value(),
                            match self.number() {
                                0 => Data,
                                1 => Prefetch,
                                _ => panic!("literal tag"),
                            },
                        ),
                        self.value(),
                        self.value(),
                        self.value(),
                    ),
                    1 => Event::Write(
                        access(self.value(), self.value(), self.value(), Data),
                        self.value(),
                        self.value(),
                        self.value(),
                    ),
                    _ => panic!("literal tag"),
                })
                .collect(),
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut reader = Reader {
        bytes: include_bytes!("literals.bin"),
        at: 4,
    };
    assert_eq!(&reader.bytes[..4], b"ORW1");
    let count = reader.number();
    assert_eq!(count, 112);
    let cases = (0..count).map(|_| reader.case()).collect();
    assert_eq!(reader.at, reader.bytes.len());
    cases
}
