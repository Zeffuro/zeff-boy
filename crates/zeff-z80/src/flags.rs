use super::*;

impl Cpu {
    pub(super) fn set_szp_flags_preserving_carry(&mut self, value: u8) {
        self.regs.f = (self.regs.f & Z80_FLAG_CARRY) | szp_flags(value);
    }
}

pub(super) fn szp_flags(value: u8) -> u8 {
    let mut flags = sz53_flags(value);
    if value.count_ones().is_multiple_of(2) {
        flags |= Z80_FLAG_PARITY_OVERFLOW;
    }
    flags
}

pub(super) fn sz53_flags(value: u8) -> u8 {
    let mut flags = value & (Z80_FLAG_SIGN | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3);
    if value == 0 {
        flags |= Z80_FLAG_ZERO;
    }
    flags
}
