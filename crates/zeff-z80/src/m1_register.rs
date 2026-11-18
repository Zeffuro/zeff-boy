use super::*;

pub(super) fn admitted(opcode: u8) -> bool {
    matches!(
        opcode,
        0x03 | 0x13 | 0x23 | 0x33 | 0x0b | 0x1b | 0x2b | 0x3b | 0xf9
    )
}

impl Cpu {
    pub(super) fn execute_m1_register(&mut self, opcode: u8, index: Option<u8>) -> Option<u32> {
        if !admitted(opcode) {
            return None;
        }
        let selected = index.map(|prefix| prefix == Z80_PREFIX_FD);
        let pair = if opcode == 0xf9 { 2 } else { (opcode >> 4) & 3 };
        let value = match (pair, selected) {
            (2, Some(false)) => self.regs.ix,
            (2, Some(true)) => self.regs.iy,
            _ => self.read_reg16(pair),
        };
        if opcode == 0xf9 {
            self.regs.sp = value;
            return Some(CYCLES_LD_SP_HL);
        }
        let result = if opcode & 8 == 0 {
            value.wrapping_add(1)
        } else {
            value.wrapping_sub(1)
        };
        match (pair, selected) {
            (2, Some(false)) => self.regs.ix = result,
            (2, Some(true)) => self.regs.iy = result,
            _ => self.write_reg16(pair, result),
        }
        Some(CYCLES_INC_DEC_RR)
    }
}
