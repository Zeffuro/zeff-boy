use super::*;
use device::Kind;
use fixture::{Case, ResultKind, capture, entry, idle, image, instruction};

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
            index: self.value(),
            label: self.text(),
            source: self.value(),
            initial: image(
                std::array::from_fn(|_| self.value()),
                self.value(),
                self.flag(),
            ),
            expected: image(
                std::array::from_fn(|_| self.value()),
                self.value(),
                self.flag(),
            ),
            memory: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            final_memory: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            transfers: (0..self.number())
                .map(|_| match self.number() {
                    0 => instruction(
                        match self.number() {
                            0 => InstructionPhase::OperandReadHigh,
                            1 => InstructionPhase::OperandRefill,
                            2 => InstructionPhase::OperandWriteHigh,
                            3 => InstructionPhase::OperandReadLow,
                            4 => InstructionPhase::OperandWriteLow,
                            _ => panic!("literal tag"),
                        },
                        (self.value(), self.value(), self.value()),
                        (
                            self.value(),
                            self.value(),
                            self.value(),
                            match self.number() {
                                0 => Kind::Complete,
                                1 => Kind::Boundary,
                                2 => Kind::Host,
                                3 => Kind::Unavailable,
                                _ => panic!("literal tag"),
                            },
                            self.flag(),
                        ),
                        std::array::from_fn(|_| self.value()),
                    ),
                    1 => idle(self.value(), self.value()),
                    2 => entry(
                        (self.value(), self.value(), self.value()),
                        (self.value(), self.value(), self.value()),
                        match self.number() {
                            0 => AccessKind::Stack,
                            1 => AccessKind::Vector,
                            2 => AccessKind::Prefetch,
                            _ => panic!("literal tag"),
                        },
                        match self.number() {
                            0 => Direction::Write,
                            1 => Direction::Read,
                            _ => panic!("literal tag"),
                        },
                    ),
                    _ => panic!("literal tag"),
                })
                .collect(),
            kind: match self.number() {
                0 => ResultKind::Instruction,
                1 => ResultKind::Group0,
                2 => ResultKind::Host,
                3 => ResultKind::Unavailable,
                4 => ResultKind::Blocked,
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
            duration: self.value(),
            writes: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
            frame: (0..self.number())
                .map(|_| (self.value(), self.value()))
                .collect(),
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut reader = Reader {
        bytes: include_bytes!("literals.bin"),
        at: 4,
    };
    assert_eq!(&reader.bytes[..4], b"PRW1");
    let count = reader.number();
    assert_eq!(count, 70);
    let cases = (0..count).map(|_| reader.case()).collect();
    assert_eq!(reader.at, reader.bytes.len());
    cases
}
