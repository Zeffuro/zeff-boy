use super::{Decoder, Error, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub(super) fn field(op: u16) -> Result<Self> {
        match (op >> 6) & 3 {
            0 => Ok(Self::Byte),
            1 => Ok(Self::Word),
            2 => Ok(Self::Long),
            _ => Err(Error::Invalid),
        }
    }

    pub(super) fn suffix(self) -> &'static str {
        match self {
            Self::Byte => "B",
            Self::Word => "W",
            Self::Long => "L",
        }
    }
}

pub(super) fn data(mode: u16, reg: u16) -> bool {
    mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 4
}

pub(super) fn alterable(mode: u16, reg: u16) -> bool {
    mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 1
}

pub(super) fn memory(mode: u16, reg: u16) -> bool {
    mode != 0 && alterable(mode, reg)
}

pub(super) fn control(mode: u16, reg: u16) -> bool {
    matches!(mode, 2 | 5 | 6) || mode == 7 && reg <= 3
}

impl<F: Fn(u32) -> Option<u8>> Decoder<F> {
    pub(super) fn immediate(&mut self, size: Size) -> Result<String> {
        Ok(match size {
            Size::Byte => format!("#${:02X}", self.word()? as u8),
            Size::Word => format!("#${:04X}", self.word()?),
            Size::Long => format!("#${:08X}", self.long()?),
        })
    }

    pub(super) fn ea(&mut self, mode: u16, reg: u16, size: Size) -> Result<String> {
        Ok(match mode {
            0 => format!("D{reg}"),
            1 => format!("A{reg}"),
            2 => format!("(A{reg})"),
            3 => format!("(A{reg})+"),
            4 => format!("-(A{reg})"),
            5 => format!("({},A{reg})", self.word()? as i16),
            6 => {
                let ext = self.word()?;
                indexed(&format!("A{reg}"), ext)
            }
            7 => match reg {
                0 => format!("${:08X}.W", self.word()? as i16 as i32 as u32),
                1 => format!("${:08X}.L", self.long()?),
                2 => {
                    let base = self.cursor();
                    let displacement = i32::from(self.word()? as i16);
                    format!("(${:08X},PC)", base.wrapping_add_signed(displacement))
                }
                3 => {
                    let ext = self.word()?;
                    indexed("PC", ext)
                }
                4 => self.immediate(size)?,
                _ => return Err(Error::Invalid),
            },
            _ => return Err(Error::Invalid),
        })
    }

    pub(super) fn control_ea(&mut self, mode: u16, reg: u16) -> Result<String> {
        let base = self.cursor();
        let text = self.ea(mode, reg, Size::Long)?;
        self.target = match (mode, reg) {
            (7, 0) => Some(i16::from_be_bytes([
                self.bytes[self.bytes.len() - 2],
                self.bytes[self.bytes.len() - 1],
            ]) as i32 as u32),
            (7, 1) => {
                let end = self.bytes.len();
                Some(u32::from_be_bytes(
                    self.bytes[end - 4..end]
                        .try_into()
                        .map_err(|_| Error::Invalid)?,
                ))
            }
            (7, 2) => {
                let end = self.bytes.len();
                Some(base.wrapping_add_signed(i32::from(i16::from_be_bytes([
                    self.bytes[end - 2],
                    self.bytes[end - 1],
                ]))))
            }
            _ => None,
        };
        Ok(text)
    }
}

fn indexed(base: &str, ext: u16) -> String {
    format!(
        "({},{base},{}{}.{})",
        ext as u8 as i8,
        if ext & 0x8000 == 0 { "D" } else { "A" },
        (ext >> 12) & 7,
        if ext & 0x800 == 0 { "W" } else { "L" }
    )
}
