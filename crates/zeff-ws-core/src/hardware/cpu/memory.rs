use super::*;

impl Cpu {
    pub(super) fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let value = bus.read8(self.pc());
        self.ip = self.ip.wrapping_add(1);
        value
    }

    pub(super) fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch8(bus);
        let hi = self.fetch8(bus);
        u16::from_le_bytes([lo, hi])
    }

    pub(super) fn fetch_modrm(&mut self, bus: &mut Bus) -> ModRm {
        let byte = self.fetch8(bus);
        ModRm {
            byte,
            mode: byte >> 6,
            reg: (byte >> 3) & 0x07,
            rm: byte & 0x07,
        }
    }

    pub(super) fn decode_rm_operand(
        &mut self,
        modrm: ModRm,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) -> Operand {
        if modrm.mode == 0b11 {
            return Operand::Register(modrm.rm);
        }

        let (offset, default_segment) = self.decode_rm_effective_offset(modrm, bus);
        Operand::Memory(self.overridden_address(segment_override.or(Some(default_segment)), offset))
    }

    pub(super) fn decode_rm_effective_offset(
        &mut self,
        modrm: ModRm,
        bus: &mut Bus,
    ) -> (u16, SegmentRegister) {
        let (base, uses_bp) = match modrm.rm {
            0 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_SI)),
                false,
            ),
            1 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_DI)),
                false,
            ),
            2 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_SI)),
                true,
            ),
            3 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_DI)),
                true,
            ),
            4 => (self.get_reg16(REG_SI), false),
            5 => (self.get_reg16(REG_DI), false),
            6 if modrm.mode == 0 => (self.fetch16(bus), false),
            6 => (self.get_reg16(REG_BP), true),
            _ => (self.get_reg16(REG_BX), false),
        };

        let offset = match modrm.mode {
            0 => base,
            1 => base.wrapping_add_signed(i16::from(self.fetch8(bus) as i8)),
            2 => base.wrapping_add(self.fetch16(bus)),
            _ => unreachable!("register operands returned above"),
        };
        let default_segment = if uses_bp {
            SegmentRegister::Ss
        } else {
            SegmentRegister::Ds
        };
        (offset, default_segment)
    }

    pub(super) fn decode_v30mz_register_mode_offset(&self, rm: u8) -> (u16, SegmentRegister) {
        match rm & 0x07 {
            0 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_AX)),
                SegmentRegister::Ds,
            ),
            1 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_CX)),
                SegmentRegister::Ds,
            ),
            2 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_DX)),
                SegmentRegister::Ss,
            ),
            3 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_BX)),
                SegmentRegister::Ss,
            ),
            4 => (
                self.get_reg16(REG_SI).wrapping_add(self.get_reg16(REG_SP)),
                SegmentRegister::Ds,
            ),
            5 => (
                self.get_reg16(REG_DI).wrapping_add(self.get_reg16(REG_BP)),
                SegmentRegister::Ds,
            ),
            6 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_SI)),
                SegmentRegister::Ss,
            ),
            _ => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_DI)),
                SegmentRegister::Ds,
            ),
        }
    }

    pub(super) fn read_operand8(&mut self, operand: Operand, bus: &mut Bus) -> u8 {
        match operand {
            Operand::Register(reg) => self.get_reg8(reg),
            Operand::Memory(addr) => bus.read8(addr),
        }
    }

    pub(super) fn read_operand16(&mut self, operand: Operand, bus: &mut Bus) -> u16 {
        match operand {
            Operand::Register(reg) => self.get_reg16(reg),
            Operand::Memory(addr) => bus.read16(addr),
        }
    }

    pub(super) fn write_operand8(&mut self, operand: Operand, value: u8, bus: &mut Bus) {
        match operand {
            Operand::Register(reg) => self.set_reg8(reg, value),
            Operand::Memory(addr) => bus.write8(addr, value),
        }
    }

    pub(super) fn write_operand16(&mut self, operand: Operand, value: u16, bus: &mut Bus) {
        match operand {
            Operand::Register(reg) => self.set_reg16(reg, value),
            Operand::Memory(addr) => bus.write16(addr, value),
        }
    }

    pub(super) fn get_reg8(&self, reg: u8) -> u8 {
        let word = self.regs[usize::from(reg & 0x03)];
        if reg & 0x04 == 0 {
            word as u8
        } else {
            (word >> 8) as u8
        }
    }

    pub(super) fn set_reg8(&mut self, reg: u8, value: u8) {
        let slot = &mut self.regs[usize::from(reg & 0x03)];
        if reg & 0x04 == 0 {
            *slot = (*slot & 0xFF00) | u16::from(value);
        } else {
            *slot = (*slot & 0x00FF) | (u16::from(value) << 8);
        }
    }

    pub(super) fn get_reg16(&self, reg: u8) -> u16 {
        self.regs[usize::from(reg & 0x07)]
    }

    pub(super) fn set_reg16(&mut self, reg: u8, value: u16) {
        self.regs[usize::from(reg & 0x07)] = value;
    }

    pub(super) fn overridden_address(
        &self,
        segment_override: Option<SegmentRegister>,
        offset: u16,
    ) -> u32 {
        self.physical_address(segment_override.unwrap_or(SegmentRegister::Ds), offset)
    }

    pub(super) fn physical_address(&self, segment: SegmentRegister, offset: u16) -> u32 {
        ((u32::from(self.segments[segment.index()]) << 4).wrapping_add(u32::from(offset)))
            & ADDRESS_MASK
    }

    pub(super) fn push16(&mut self, value: u16, bus: &mut Bus) {
        let sp = self.get_reg16(REG_SP).wrapping_sub(2);
        self.set_reg16(REG_SP, sp);
        let addr = self.physical_address(SegmentRegister::Ss, sp);
        bus.write16(addr, value);
    }

    pub(super) fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let sp = self.get_reg16(REG_SP);
        let addr = self.physical_address(SegmentRegister::Ss, sp);
        let value = bus.read16(addr);
        self.set_reg16(REG_SP, sp.wrapping_add(2));
        value
    }
}
