use super::ea::{alterable, control, data};
use super::{Decoder, Error, Result, Size, require};

impl<F: Fn(u32) -> Option<u8>> Decoder<F> {
    pub(super) fn system(&mut self, op: u16) -> Result<String> {
        let (mode, reg, dn) = ((op >> 3) & 7, op & 7, (op >> 9) & 7);
        match op {
            0x4e70 => return Ok("RESET".to_owned()),
            0x4e71 => return Ok("NOP".to_owned()),
            0x4e72 => return Ok(format!("STOP #${:04X}", self.word()?)),
            0x4e73 => return Ok("RTE".to_owned()),
            0x4e75 => return Ok("RTS".to_owned()),
            0x4e76 => return Ok("TRAPV".to_owned()),
            0x4e77 => return Ok("RTR".to_owned()),
            0x4afc => return Ok("ILLEGAL".to_owned()),
            _ => {}
        }
        if op & 0xfff0 == 0x4e40 {
            return Ok(format!("TRAP #{}", op & 15));
        }
        if op & 0xfff8 == 0x4e50 {
            return Ok(format!("LINK A{reg},#{}", self.word()? as i16));
        }
        if op & 0xfff8 == 0x4e58 {
            return Ok(format!("UNLK A{reg}"));
        }
        if op & 0xfff0 == 0x4e60 {
            return Ok(if op & 8 == 0 {
                format!("MOVE A{reg},USP")
            } else {
                format!("MOVE USP,A{reg}")
            });
        }
        if op & 0xf1c0 == 0x41c0 {
            require(control(mode, reg))?;
            let source = self.ea(mode, reg, Size::Long)?;
            return Ok(format!("LEA {source},A{dn}"));
        }
        if op & 0xf1c0 == 0x4180 {
            require(data(mode, reg))?;
            let source = self.ea(mode, reg, Size::Word)?;
            return Ok(format!("CHK.W {source},D{dn}"));
        }
        if matches!(op & 0xffc0, 0x4e80 | 0x4ec0) {
            require(control(mode, reg))?;
            let operand = self.control_ea(mode, reg)?;
            return Ok(format!(
                "{} {operand}",
                if op & 0x40 == 0 { "JSR" } else { "JMP" }
            ));
        }
        if op & 0xfff8 == 0x4840 {
            return Ok(format!("SWAP D{reg}"));
        }
        if matches!(op & 0xfff8, 0x4880 | 0x48c0) {
            return Ok(format!(
                "EXT.{} D{reg}",
                if op & 0x40 == 0 { "W" } else { "L" }
            ));
        }
        if op & 0xffc0 == 0x4840 {
            require(control(mode, reg))?;
            let operand = self.ea(mode, reg, Size::Long)?;
            return Ok(format!("PEA {operand}"));
        }
        if op & 0xfb80 == 0x4880 {
            let load = op & 0x400 != 0;
            require(if load {
                control(mode, reg) || mode == 3
            } else {
                control(mode, reg) && !(mode == 7 && reg >= 2) || mode == 4
            })?;
            let mask = self.word()?;
            let registers = register_list(mask, mode == 4);
            let size = if op & 0x40 == 0 {
                Size::Word
            } else {
                Size::Long
            };
            let operand = self.ea(mode, reg, size)?;
            return Ok(if load {
                format!("MOVEM.{} {operand},{registers}", size.suffix())
            } else {
                format!("MOVEM.{} {registers},{operand}", size.suffix())
            });
        }
        if matches!(op & 0xffc0, 0x40c0 | 0x44c0 | 0x46c0) {
            let from = op & 0xffc0 == 0x40c0;
            require(if from {
                alterable(mode, reg)
            } else {
                data(mode, reg)
            })?;
            let operand = self.ea(mode, reg, Size::Word)?;
            return Ok(if from {
                format!("MOVE.W SR,{operand}")
            } else {
                format!(
                    "MOVE.W {operand},{}",
                    if op & 0x200 == 0 { "CCR" } else { "SR" }
                )
            });
        }
        if matches!(op & 0xffc0, 0x4800 | 0x4ac0) {
            require(alterable(mode, reg))?;
            let operand = self.ea(mode, reg, Size::Byte)?;
            return Ok(format!(
                "{} {operand}",
                if op & 0xffc0 == 0x4800 { "NBCD" } else { "TAS" }
            ));
        }
        let name = match op & 0xff00 {
            0x4000 => "NEGX",
            0x4200 => "CLR",
            0x4400 => "NEG",
            0x4600 => "NOT",
            0x4a00 => "TST",
            _ => return Err(Error::Invalid),
        };
        let size = Size::field(op)?;
        require(alterable(mode, reg))?;
        let operand = self.ea(mode, reg, size)?;
        Ok(format!("{name}.{} {operand}", size.suffix()))
    }
}

fn register_list(mask: u16, reverse: bool) -> String {
    let mask = if reverse { mask.reverse_bits() } else { mask };
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < 16 {
        if mask & (1 << index) == 0 {
            index += 1;
            continue;
        }
        let start = index;
        while index < 15 && index != 7 && mask & (1 << (index + 1)) != 0 {
            index += 1;
        }
        let register = |i: u16| format!("{}{}", if i < 8 { "D" } else { "A" }, i & 7);
        ranges.push(if start == index {
            register(start)
        } else {
            format!("{}-{}", register(start), register(index))
        });
        index += 1;
    }
    if ranges.is_empty() {
        "#0".to_owned()
    } else {
        ranges.join("/")
    }
}
