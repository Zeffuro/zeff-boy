use super::*;

impl Rmw {
    pub(crate) fn indexed_cb_cycle(
        self,
        cpu: &Cpu,
        address: u16,
        opcode: u8,
    ) -> Option<IndexedCbCycleKind> {
        if self.address != address || self.register != Some(opcode & 7) {
            return None;
        }
        match self.phase {
            Phase::Read(Operation::Cb(recorded)) if recorded == opcode => {
                Some(if opcode >> 6 == 1 {
                    IndexedCbCycleKind::Read { address }
                } else {
                    IndexedCbCycleKind::ReadModify { address }
                })
            }
            Phase::Write {
                value,
                flags,
                a: None,
            } if write_matches(opcode, value, flags, cpu.regs.f) => {
                Some(IndexedCbCycleKind::Write { address, value })
            }
            _ => None,
        }
    }
}

fn write_matches(opcode: u8, value: u8, flags: u8, current_flags: u8) -> bool {
    let bit = (opcode >> 3) & 7;
    match opcode >> 6 {
        0 => {
            let carry = flags & Z80_FLAG_CARRY;
            flags == flags::szp_flags(value) | carry
                && match bit {
                    0 => carry == value & 1,
                    1 => carry == value >> 7,
                    2 => value & 1 == current_flags & Z80_FLAG_CARRY,
                    3 => value >> 7 == current_flags & Z80_FLAG_CARRY,
                    4 => value & 1 == 0,
                    5 => (value >> 7) == ((value >> 6) & 1),
                    6 => value & 1 == 1,
                    7 => value & 0x80 == 0,
                    _ => unreachable!(),
                }
        }
        2 => flags == current_flags && value & (1 << bit) == 0,
        3 => flags == current_flags && value & (1 << bit) != 0,
        _ => false,
    }
}
