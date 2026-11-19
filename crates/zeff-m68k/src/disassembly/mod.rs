mod ea;
mod families;
mod system;

use ea::Size;

pub const MAX_INSTRUCTION_BYTES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeStatus {
    Complete,
    Invalid,
    Unavailable,
    Unaligned,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub address: u32,
    pub bytes: Vec<u8>,
    pub text: String,
    pub control_target: Option<u32>,
    pub status: DecodeStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmappedInstruction {
    pub bytes: Vec<u8>,
    pub text: String,
    pub status: DecodeStatus,
}

#[derive(Clone, Copy)]
enum Error {
    Invalid,
    Unavailable,
}

type Result<T> = std::result::Result<T, Error>;

struct Decoder<F> {
    reader: F,
    logical_pc: Option<u32>,
    bytes: Vec<u8>,
    target: Option<u32>,
}

impl<F: Fn(usize) -> Option<u8>> Decoder<F> {
    fn cursor(&self) -> Option<u32> {
        self.logical_pc
            .map(|address| address.wrapping_add(self.bytes.len() as u32))
    }

    fn byte(&mut self) -> Result<u8> {
        if self.bytes.len() >= MAX_INSTRUCTION_BYTES {
            return Err(Error::Invalid);
        }
        let byte = (self.reader)(self.bytes.len()).ok_or(Error::Unavailable)?;
        self.bytes.push(byte);
        Ok(byte)
    }

    fn word(&mut self) -> Result<u16> {
        let high = self.byte()?;
        Ok(u16::from_be_bytes([high, self.byte()?]))
    }

    fn long(&mut self) -> Result<u32> {
        let high = self.word()?;
        Ok(u32::from(high) << 16 | u32::from(self.word()?))
    }

    fn branch_target(&mut self, displacement: i32) -> String {
        self.target = self
            .logical_pc
            .map(|address| address.wrapping_add(2).wrapping_add_signed(displacement));
        self.target.map_or_else(
            || format!("(PC+2{displacement:+})"),
            |target| format!("${target:08X}"),
        )
    }

    fn instruction(&mut self, op: u16) -> Result<String> {
        match op >> 12 {
            0 => self.immediate_bit(op),
            1..=3 => self.move_operand(op),
            4 => self.system(op),
            5 => self.quick_condition(op),
            6 => {
                let displacement = if op as u8 == 0 {
                    i32::from(self.word()? as i16)
                } else {
                    i32::from(op as u8 as i8)
                };
                let target = self.branch_target(displacement);
                let name = match (op >> 8) & 15 {
                    0 => "BRA".to_owned(),
                    1 => "BSR".to_owned(),
                    code => format!("B{}", condition(code)),
                };
                Ok(format!(
                    "{name}.{} {target}",
                    if op as u8 == 0 { "W" } else { "S" }
                ))
            }
            7 if op & 0x100 == 0 => Ok(format!("MOVEQ #{},D{}", op as u8 as i8, (op >> 9) & 7)),
            8 | 9 | 11 | 12 | 13 => self.binary(op),
            14 => self.shift(op),
            _ => Err(Error::Invalid),
        }
    }

    fn parse(&mut self) -> (DecodeStatus, String) {
        if self.logical_pc.is_some_and(|address| address & 1 != 0) {
            return (DecodeStatus::Unaligned, "<unaligned>".to_owned());
        }
        match self.word() {
            Err(_) => (DecodeStatus::Unavailable, "<unavailable>".to_owned()),
            Ok(op) => match self.instruction(op) {
                Ok(text) => (DecodeStatus::Complete, text),
                Err(Error::Invalid) => {
                    self.bytes.truncate(2);
                    self.target = None;
                    (DecodeStatus::Invalid, format!("DC.W ${op:04X}"))
                }
                Err(Error::Unavailable) => {
                    self.target = None;
                    (DecodeStatus::Unavailable, "<unavailable>".to_owned())
                }
            },
        }
    }
}

fn condition(code: u16) -> &'static str {
    [
        "T", "F", "HI", "LS", "CC", "CS", "NE", "EQ", "VC", "VS", "PL", "MI", "GE", "LT", "GT",
        "LE",
    ][usize::from(code & 15)]
}

fn require(valid: bool) -> Result<()> {
    if valid { Ok(()) } else { Err(Error::Invalid) }
}

/// The reader owns address masking; missing bytes end decoding.
pub fn decode(address: u32, reader: impl Fn(u32) -> Option<u8>) -> DecodedInstruction {
    let mut decoder = Decoder {
        reader: |index: usize| reader(address.wrapping_add(index as u32)),
        logical_pc: Some(address),
        bytes: Vec::with_capacity(MAX_INSTRUCTION_BYTES),
        target: None,
    };
    let (status, text) = decoder.parse();
    DecodedInstruction {
        address,
        bytes: decoder.bytes,
        text,
        control_target: decoder.target,
        status,
    }
}

pub fn decode_unmapped(bytes: &[u8]) -> UnmappedInstruction {
    let mut decoder = Decoder {
        reader: |index: usize| bytes.get(index).copied(),
        logical_pc: None,
        bytes: Vec::with_capacity(MAX_INSTRUCTION_BYTES),
        target: None,
    };
    let (status, text) = decoder.parse();
    UnmappedInstruction {
        bytes: decoder.bytes,
        text,
        status,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unmapped_tests;
