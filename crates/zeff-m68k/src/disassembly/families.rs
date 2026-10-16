use super::ea::{alterable, data, memory};
use super::{Decoder, Error, Result, Size, condition, require};

impl<F: Fn(u32) -> Option<u8>> Decoder<F> {
    pub(super) fn move_operand(&mut self, op: u16) -> Result<String> {
        let size = match op >> 12 {
            1 => Size::Byte,
            2 => Size::Long,
            _ => Size::Word,
        };
        let (sm, sr, dm, dr) = ((op >> 3) & 7, op & 7, (op >> 6) & 7, (op >> 9) & 7);
        require(
            (data(sm, sr) || sm == 1 && size != Size::Byte)
                && (alterable(dm, dr) || dm == 1 && size != Size::Byte),
        )?;
        let source = self.ea(sm, sr, size)?;
        let destination = self.ea(dm, dr, size)?;
        Ok(format!(
            "{}.{} {source},{destination}",
            if dm == 1 { "MOVEA" } else { "MOVE" },
            size.suffix()
        ))
    }

    pub(super) fn immediate_bit(&mut self, op: u16) -> Result<String> {
        let (mode, reg, dn) = ((op >> 3) & 7, op & 7, (op >> 9) & 7);
        if op & 0xf138 == 0x0108 {
            let size = if op & 0x40 == 0 { "W" } else { "L" };
            let memory = format!("({},A{reg})", self.word()? as i16);
            return Ok(if op & 0x80 == 0 {
                format!("MOVEP.{size} {memory},D{dn}")
            } else {
                format!("MOVEP.{size} D{dn},{memory}")
            });
        }
        if matches!(op, 0x003c | 0x007c | 0x023c | 0x027c | 0x0a3c | 0x0a7c) {
            let full = op & 0x40 != 0;
            let value = self.immediate(if full { Size::Word } else { Size::Byte })?;
            let name = match op & 0xf00 {
                0 => "ORI",
                0x200 => "ANDI",
                _ => "EORI",
            };
            return Ok(format!(
                "{name} {value},{}",
                if full { "SR" } else { "CCR" }
            ));
        }
        if op & 0x100 != 0 || op & 0xff00 == 0x0800 {
            let dynamic = op & 0x100 != 0;
            let kind = (op >> 6) & 3;
            require(if kind == 0 {
                data(mode, reg) && (dynamic || !(mode == 7 && reg == 4))
            } else {
                alterable(mode, reg)
            })?;
            let source = if dynamic {
                format!("D{dn}")
            } else {
                format!("#${:04X}", self.word()?)
            };
            let size = if mode == 0 { Size::Long } else { Size::Byte };
            let destination = self.ea(mode, reg, size)?;
            let name = ["BTST", "BCHG", "BCLR", "BSET"][usize::from(kind)];
            return Ok(format!("{name}.{} {source},{destination}", size.suffix()));
        }
        let name = match (op >> 8) & 15 {
            0 => "ORI",
            2 => "ANDI",
            4 => "SUBI",
            6 => "ADDI",
            10 => "EORI",
            12 => "CMPI",
            _ => return Err(Error::Invalid),
        };
        let size = Size::field(op)?;
        require(alterable(mode, reg))?;
        let immediate = self.immediate(size)?;
        let destination = self.ea(mode, reg, size)?;
        Ok(format!(
            "{name}.{} {immediate},{destination}",
            size.suffix()
        ))
    }

    pub(super) fn quick_condition(&mut self, op: u16) -> Result<String> {
        let (mode, reg) = ((op >> 3) & 7, op & 7);
        if op & 0xc0 == 0xc0 {
            let condition = condition((op >> 8) & 15);
            if mode == 1 {
                let displacement = i32::from(self.word()? as i16);
                let target = self
                    .address
                    .wrapping_add(2)
                    .wrapping_add_signed(displacement);
                self.target = Some(target);
                return Ok(format!("DB{condition} D{reg},${target:08X}"));
            }
            require(alterable(mode, reg))?;
            let destination = self.ea(mode, reg, Size::Byte)?;
            return Ok(format!("S{condition} {destination}"));
        }
        let size = Size::field(op)?;
        require(alterable(mode, reg) || mode == 1 && size != Size::Byte)?;
        let value = (op >> 9) & 7;
        let destination = self.ea(mode, reg, size)?;
        Ok(format!(
            "{}Q.{} #{},{destination}",
            if op & 0x100 == 0 { "ADD" } else { "SUB" },
            size.suffix(),
            if value == 0 { 8 } else { value }
        ))
    }

    pub(super) fn binary(&mut self, op: u16) -> Result<String> {
        let (family, mode, reg, dn, opmode) = (
            op >> 12,
            (op >> 3) & 7,
            op & 7,
            (op >> 9) & 7,
            (op >> 6) & 7,
        );
        if matches!(op & 0xf1f0, 0x8100 | 0xc100) {
            let name = if family == 8 { "SBCD" } else { "ABCD" };
            return Ok(if op & 8 == 0 {
                format!("{name} D{reg},D{dn}")
            } else {
                format!("{name} -(A{reg}),-(A{dn})")
            });
        }
        if matches!(op & 0xf1f8, 0xc140 | 0xc148 | 0xc188) {
            return Ok(match op & 0xf1f8 {
                0xc140 => format!("EXG D{dn},D{reg}"),
                0xc148 => format!("EXG A{dn},A{reg}"),
                _ => format!("EXG D{dn},A{reg}"),
            });
        }
        if matches!(family, 9 | 13) && op & 0x130 == 0x100 && opmode != 7 {
            let size = Size::field(op)?;
            let name = if family == 9 { "SUBX" } else { "ADDX" };
            return Ok(if op & 8 == 0 {
                format!("{name}.{} D{reg},D{dn}", size.suffix())
            } else {
                format!("{name}.{} -(A{reg}),-(A{dn})", size.suffix())
            });
        }
        if family == 11 && op & 0x138 == 0x108 && opmode != 7 {
            let size = Size::field(op)?;
            return Ok(format!("CMPM.{} (A{reg})+,(A{dn})+", size.suffix()));
        }
        if matches!(family, 8 | 12) && matches!(opmode, 3 | 7) {
            require(data(mode, reg))?;
            let source = self.ea(mode, reg, Size::Word)?;
            let name = match (family, opmode) {
                (8, 3) => "DIVU",
                (8, _) => "DIVS",
                (12, 3) => "MULU",
                _ => "MULS",
            };
            return Ok(format!("{name}.W {source},D{dn}"));
        }
        if matches!(family, 9 | 11 | 13) && matches!(opmode, 3 | 7) {
            require(data(mode, reg) || mode == 1)?;
            let size = if opmode == 3 { Size::Word } else { Size::Long };
            let source = self.ea(mode, reg, size)?;
            let name = match family {
                9 => "SUBA",
                11 => "CMPA",
                _ => "ADDA",
            };
            return Ok(format!("{name}.{} {source},A{dn}", size.suffix()));
        }
        let size = Size::field(op)?;
        let reverse = opmode >= 4;
        require(if reverse {
            memory(mode, reg) || family == 11 && mode == 0
        } else {
            data(mode, reg) || matches!(family, 9 | 11 | 13) && mode == 1 && size != Size::Byte
        })?;
        let operand = self.ea(mode, reg, size)?;
        let name = match family {
            8 => "OR",
            9 => "SUB",
            11 if reverse => "EOR",
            11 => "CMP",
            12 => "AND",
            _ => "ADD",
        };
        Ok(if reverse {
            format!("{name}.{} D{dn},{operand}", size.suffix())
        } else {
            format!("{name}.{} {operand},D{dn}", size.suffix())
        })
    }

    pub(super) fn shift(&mut self, op: u16) -> Result<String> {
        let left = op & 0x100 != 0;
        if op & 0xc0 == 0xc0 {
            let (mode, reg) = ((op >> 3) & 7, op & 7);
            require(op & 0x800 == 0 && memory(mode, reg))?;
            let name = ["AS", "LS", "ROX", "RO"][usize::from((op >> 9) & 3)];
            let operand = self.ea(mode, reg, Size::Word)?;
            return Ok(format!(
                "{name}{}.W {operand}",
                if left { "L" } else { "R" }
            ));
        }
        let size = Size::field(op)?;
        let count = (op >> 9) & 7;
        let count = if op & 0x20 == 0 {
            format!("#{}", if count == 0 { 8 } else { count })
        } else {
            format!("D{count}")
        };
        let name = ["AS", "LS", "ROX", "RO"][usize::from((op >> 3) & 3)];
        Ok(format!(
            "{name}{}.{} {count},D{}",
            if left { "L" } else { "R" },
            size.suffix(),
            op & 7
        ))
    }
}
