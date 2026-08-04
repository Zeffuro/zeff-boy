use super::*;

impl Cpu {
    pub(super) fn movs8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read8(cpu.string_src_addr(segment_override));
            bus.write8(cpu.string_dst_addr(), value);
            cpu.adjust_string_index(REG_SI, 1);
            cpu.adjust_string_index(REG_DI, 1);
        });
    }

    pub(super) fn movs16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read16(cpu.string_src_addr(segment_override));
            bus.write16(cpu.string_dst_addr(), value);
            cpu.adjust_string_index(REG_SI, 2);
            cpu.adjust_string_index(REG_DI, 2);
        });
    }

    pub(super) fn ins8(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.io_read8(cpu.get_reg16(REG_DX));
            bus.write8(cpu.string_dst_addr(), value);
            cpu.adjust_string_index(REG_DI, 1);
        });
    }

    pub(super) fn ins16(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.io_read16(cpu.get_reg16(REG_DX));
            bus.write16(cpu.string_dst_addr(), value);
            cpu.adjust_string_index(REG_DI, 2);
        });
    }

    pub(super) fn outs8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read8(cpu.string_src_addr(segment_override));
            bus.io_write8(cpu.get_reg16(REG_DX), value);
            cpu.adjust_string_index(REG_SI, 1);
        });
    }

    pub(super) fn outs16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read16(cpu.string_src_addr(segment_override));
            bus.io_write16(cpu.get_reg16(REG_DX), value);
            cpu.adjust_string_index(REG_SI, 2);
        });
    }

    pub(super) fn stos8(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            bus.write8(cpu.string_dst_addr(), cpu.get_reg8(REG_AX));
            cpu.adjust_string_index(REG_DI, 1);
        });
    }

    pub(super) fn stos16(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            bus.write16(cpu.string_dst_addr(), cpu.get_reg16(REG_AX));
            cpu.adjust_string_index(REG_DI, 2);
        });
    }

    pub(super) fn lods8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read8(cpu.string_src_addr(segment_override));
            cpu.set_reg8(REG_AX, value);
            cpu.adjust_string_index(REG_SI, 1);
        });
    }

    pub(super) fn lods16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_simple(repeat_prefix, bus, |cpu, bus| {
            let value = bus.read16(cpu.string_src_addr(segment_override));
            cpu.set_reg16(REG_AX, value);
            cpu.adjust_string_index(REG_SI, 2);
        });
    }

    pub(super) fn cmps8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_compare(repeat_prefix, bus, |cpu, bus| {
            let lhs = bus.read8(cpu.string_src_addr(segment_override));
            let rhs = bus.read8(cpu.string_dst_addr());
            cpu.alu8(AluOp::Cmp, lhs, rhs);
            cpu.adjust_string_index(REG_SI, 1);
            cpu.adjust_string_index(REG_DI, 1);
        });
    }

    pub(super) fn cmps16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        self.repeat_compare(repeat_prefix, bus, |cpu, bus| {
            let lhs = bus.read16(cpu.string_src_addr(segment_override));
            let rhs = bus.read16(cpu.string_dst_addr());
            cpu.alu16(AluOp::Cmp, lhs, rhs);
            cpu.adjust_string_index(REG_SI, 2);
            cpu.adjust_string_index(REG_DI, 2);
        });
    }

    pub(super) fn scas8(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_compare(repeat_prefix, bus, |cpu, bus| {
            let rhs = bus.read8(cpu.string_dst_addr());
            cpu.alu8(AluOp::Cmp, cpu.get_reg8(REG_AX), rhs);
            cpu.adjust_string_index(REG_DI, 1);
        });
    }

    pub(super) fn scas16(&mut self, repeat_prefix: Option<RepeatPrefix>, bus: &mut Bus) {
        self.repeat_compare(repeat_prefix, bus, |cpu, bus| {
            let rhs = bus.read16(cpu.string_dst_addr());
            cpu.alu16(AluOp::Cmp, cpu.get_reg16(REG_AX), rhs);
            cpu.adjust_string_index(REG_DI, 2);
        });
    }

    pub(super) fn repeat_simple(
        &mut self,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
        mut body: impl FnMut(&mut Cpu, &mut Bus),
    ) {
        let iterations = self.repeat_iterations(repeat_prefix);
        for _ in 0..iterations {
            body(self, bus);
            self.add_cycles(bus, 4);
            self.decrement_repeat_counter(repeat_prefix);
        }
        if iterations == 0 {
            self.add_cycles(bus, 2);
        }
    }

    pub(super) fn repeat_compare(
        &mut self,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
        mut body: impl FnMut(&mut Cpu, &mut Bus),
    ) {
        let iterations = self.repeat_iterations(repeat_prefix);
        for _ in 0..iterations {
            body(self, bus);
            self.add_cycles(bus, 4);
            self.decrement_repeat_counter(repeat_prefix);
            let zf = self.flags & FLAG_ZF != 0;
            match repeat_prefix {
                Some(RepeatPrefix::Repe) if !zf => break,
                Some(RepeatPrefix::Repne) if zf => break,
                _ => {}
            }
        }
        if iterations == 0 {
            self.add_cycles(bus, 2);
        }
    }

    pub(super) fn repeat_iterations(&self, repeat_prefix: Option<RepeatPrefix>) -> u16 {
        if repeat_prefix.is_none() || self.get_reg16(REG_CX) != 0 {
            1
        } else {
            0
        }
    }

    pub(super) fn decrement_repeat_counter(&mut self, repeat_prefix: Option<RepeatPrefix>) {
        if repeat_prefix.is_some() {
            self.set_reg16(REG_CX, self.get_reg16(REG_CX).wrapping_sub(1));
        }
    }

    pub(super) fn string_src_addr(&self, segment_override: Option<SegmentRegister>) -> u32 {
        self.overridden_address(segment_override, self.get_reg16(REG_SI))
    }

    pub(super) fn string_dst_addr(&self) -> u32 {
        self.physical_address(SegmentRegister::Es, self.get_reg16(REG_DI))
    }

    pub(super) fn adjust_string_index(&mut self, reg: u8, size: u16) {
        let value = if self.flags & FLAG_DF != 0 {
            self.get_reg16(reg).wrapping_sub(size)
        } else {
            self.get_reg16(reg).wrapping_add(size)
        };
        self.set_reg16(reg, value);
    }

    pub(super) fn daa(&mut self, bus: &mut Bus) {
        let old_al = self.get_reg8(REG_AX);
        let old_cf = self.carry_set();
        let old_af = self.flags & FLAG_AF != 0;
        let mut adjust = 0u8;
        let carry = old_cf || old_al >= 0x9A;
        let auxiliary = old_af || (old_al & 0x0F) >= 0x0A;
        if auxiliary {
            adjust |= 0x06;
        }
        if carry {
            adjust |= 0x60;
        }
        let result = old_al.wrapping_add(adjust);
        self.set_reg8(REG_AX, result);
        self.set_add_flags8(old_al, adjust, result);
        self.set_adjust_carry_flags(carry, auxiliary);
        self.add_cycles(bus, 4);
    }

    pub(super) fn das(&mut self, bus: &mut Bus) {
        let old_al = self.get_reg8(REG_AX);
        let old_cf = self.carry_set();
        let old_af = self.flags & FLAG_AF != 0;
        let mut adjust = 0u8;
        let carry = old_cf || old_al >= 0x9A;
        let auxiliary = old_af || (old_al & 0x0F) >= 0x0A;
        if auxiliary {
            adjust |= 0x06;
        }
        if carry {
            adjust |= 0x60;
        }
        let result = old_al.wrapping_sub(adjust);
        self.set_reg8(REG_AX, result);
        self.set_sub_flags8(old_al, adjust, result);
        self.set_adjust_carry_flags(carry, auxiliary);
        self.add_cycles(bus, 4);
    }

    pub(super) fn aaa(&mut self, bus: &mut Bus) {
        let old_ax = self.get_reg16(REG_AX);
        let old_al = old_ax as u8;
        let adjust = self.flags & FLAG_AF != 0 || (old_al & 0x0F) >= 0x0A;
        let result = if adjust {
            let al = old_al.wrapping_add(0x06) & 0x0F;
            let ah = (old_ax >> 8).wrapping_add(1) as u8;
            (u16::from(ah) << 8) | u16::from(al)
        } else {
            (old_ax & 0xFF00) | u16::from(old_al & 0x0F)
        };
        self.set_reg16(REG_AX, result);
        self.set_ascii_adjust_flags(adjust);
        self.add_cycles(bus, 4);
    }

    pub(super) fn aas(&mut self, bus: &mut Bus) {
        let old_ax = self.get_reg16(REG_AX);
        let old_al = old_ax as u8;
        let adjust = self.flags & FLAG_AF != 0 || (old_al & 0x0F) >= 0x0A;
        let result = if adjust {
            let al = old_al.wrapping_sub(0x06) & 0x0F;
            let ah = (old_ax >> 8).wrapping_sub(1) as u8;
            (u16::from(ah) << 8) | u16::from(al)
        } else {
            (old_ax & 0xFF00) | u16::from(old_al & 0x0F)
        };
        self.set_reg16(REG_AX, result);
        self.set_ascii_adjust_flags(adjust);
        self.add_cycles(bus, 4);
    }

    fn set_adjust_carry_flags(&mut self, carry: bool, auxiliary: bool) {
        self.flags &= !(FLAG_CF | FLAG_AF);
        if carry {
            self.flags |= FLAG_CF;
        }
        if auxiliary {
            self.flags |= FLAG_AF;
        }
        self.flags |= FLAG_FIXED;
    }

    fn set_ascii_adjust_flags(&mut self, adjusted: bool) {
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        self.flags |= FLAG_PF | FLAG_FIXED;
        if adjusted {
            self.flags |= FLAG_CF | FLAG_AF | FLAG_ZF;
        } else {
            self.flags |= FLAG_SF;
        }
    }
}
