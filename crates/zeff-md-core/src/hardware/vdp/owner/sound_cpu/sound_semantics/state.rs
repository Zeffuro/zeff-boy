use super::*;

impl Plan {
    pub fn write_state(self, w: &mut StateWriter) {
        w.write_u8(match self.shape {
            Shape::Fetch => 0,
            Shape::Read => 1,
            Shape::Write => 2,
            Shape::Internal => 3,
            Shape::InterruptAck => 4,
        });
        w.write_u16(self.address);
        w.write_u8(self.value);
        w.write_u16(self.refresh);
        w.write_u32(self.cycles);
        w.write_u8(self.steps);
        w.write_bool(self.external);
        w.write_u16(self.bank);
        w.write_bool(self.opaque_tail);
        w.write_bool(self.irq);
        w.write_bool(self.finishes);
        w.write_bool(self.im1.is_some());
        if let Some(binding) = self.im1 {
            w.write_u16(binding.sp);
            w.write_bool(binding.halted);
        }
        w.write_u8(match self.jr {
            None => 0,
            Some(JrPhase::Read) => 1,
            Some(JrPhase::Internal) => 2,
        });
        w.write_u8(match self.copy {
            None => 0,
            Some(CopyPhase::Read) => 1,
            Some(CopyPhase::Write) => 2,
            Some(CopyPhase::Repeat) => 3,
        });
        w.write_u8(match self.word {
            None => 0,
            Some(WordPhase::M1) => 1,
            Some(WordPhase::Internal4) => 2,
            Some(WordPhase::Internal3) => 3,
        });
        w.write_u8(match self.djnz {
            None => 0,
            Some(DjnzPhase::M1) => 1,
            Some(DjnzPhase::Operand) => 2,
            Some(DjnzPhase::Internal) => 3,
        });
        w.write_u8(match self.indexed {
            None => 0,
            Some(IndexedPhase::Displacement) => 1,
            Some(IndexedPhase::Address) => 2,
            Some(IndexedPhase::Read) => 3,
            Some(IndexedPhase::Write) => 4,
        });
        w.write_u8(match self.ed_word {
            None => 0,
            Some(EdWordPhase::Internal4) => 1,
            Some(EdWordPhase::Internal3) => 2,
        });
        w.write_u16(self.instruction.pc);
        w.write_u8(self.instruction.opcode);
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let shape = match r.read_u8()? {
            0 => Shape::Fetch,
            1 => Shape::Read,
            2 => Shape::Write,
            3 => Shape::Internal,
            4 => Shape::InterruptAck,
            _ => anyhow::bail!("invalid SOUND shape"),
        };
        let address = r.read_u16()?;
        let value = r.read_u8()?;
        let refresh = r.read_u16()?;
        let cycles = r.read_u32()?;
        let steps = r.read_u8()?;
        let external = r.read_bool()?;
        let bank = r.read_u16()?;
        let opaque_tail = r.read_bool()?;
        let irq = r.read_bool()?;
        let finishes = r.read_bool()?;
        let im1 = if r.read_bool()? {
            Some(Im1Binding {
                sp: r.read_u16()?,
                halted: r.read_bool()?,
            })
        } else {
            None
        };
        let jr = match r.read_u8()? {
            0 => None,
            1 => Some(JrPhase::Read),
            2 => Some(JrPhase::Internal),
            _ => anyhow::bail!("invalid JR plan phase"),
        };
        let copy = match r.read_u8()? {
            0 => None,
            1 => Some(CopyPhase::Read),
            2 => Some(CopyPhase::Write),
            3 => Some(CopyPhase::Repeat),
            _ => anyhow::bail!("invalid copy phase"),
        };
        let word = match r.read_u8()? {
            0 => None,
            1 => Some(WordPhase::M1),
            2 => Some(WordPhase::Internal4),
            3 => Some(WordPhase::Internal3),
            _ => anyhow::bail!("invalid ADD16 phase"),
        };
        let djnz = match r.read_u8()? {
            0 => None,
            1 => Some(DjnzPhase::M1),
            2 => Some(DjnzPhase::Operand),
            3 => Some(DjnzPhase::Internal),
            _ => anyhow::bail!("invalid DJNZ phase"),
        };
        let indexed = match r.read_u8()? {
            0 => None,
            1 => Some(IndexedPhase::Displacement),
            2 => Some(IndexedPhase::Address),
            3 => Some(IndexedPhase::Read),
            4 => Some(IndexedPhase::Write),
            _ => anyhow::bail!("invalid indexed phase"),
        };
        let ed_word = match r.read_u8()? {
            0 => None,
            1 => Some(EdWordPhase::Internal4),
            2 => Some(EdWordPhase::Internal3),
            _ => anyhow::bail!("invalid ED arithmetic phase"),
        };
        let instruction = FetchedInstruction {
            pc: r.read_u16()?,
            opcode: r.read_u8()?,
            cycles,
        };
        let p = Self {
            shape,
            address,
            value,
            refresh,
            cycles,
            steps,
            external,
            bank,
            opaque_tail,
            irq,
            finishes,
            im1,
            jr,
            copy,
            word,
            djnz,
            indexed,
            ed_word,
            instruction,
        };
        p.validate()?;
        Ok(p)
    }
}
