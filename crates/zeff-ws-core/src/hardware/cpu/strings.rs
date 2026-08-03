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
        let mut adjust = 0u8;
        let mut carry = old_cf;
        let mut auxiliary = false;
        if (old_al & 0x0F) > 9 || self.flags & FLAG_AF != 0 {
            adjust |= 0x06;
            auxiliary = true;
        }
        if old_al > 0x99 || old_cf {
            adjust |= 0x60;
            carry = true;
        }
        let result = old_al.wrapping_add(adjust);
        self.set_reg8(REG_AX, result);
        self.set_logic_flags8(result);
        self.set_carry(carry);
        self.set_auxiliary_carry(auxiliary);
        self.add_cycles(bus, 4);
    }

    pub(super) fn das(&mut self, bus: &mut Bus) {
        let old_al = self.get_reg8(REG_AX);
        let old_cf = self.carry_set();
        let mut adjust = 0u8;
        let mut carry = false;
        let mut auxiliary = false;
        if (old_al & 0x0F) > 9 || self.flags & FLAG_AF != 0 {
            adjust |= 0x06;
            auxiliary = true;
        }
        if old_al > 0x99 || old_cf {
            adjust |= 0x60;
            carry = true;
        }
        let result = old_al.wrapping_sub(adjust);
        self.set_reg8(REG_AX, result);
        self.set_logic_flags8(result);
        self.set_carry(carry);
        self.set_auxiliary_carry(auxiliary);
        self.add_cycles(bus, 4);
    }
}
