use super::*;
use crate::IndexedMemoryKind;

impl Rmw {
    pub(crate) fn indexed_memory_cycle(
        self,
        cpu: &Cpu,
        address: u16,
        opcode: u8,
    ) -> Option<IndexedMemoryKind> {
        if self.address != address || self.register.is_some() {
            return None;
        }
        match (opcode, self.phase) {
            (0x34, Phase::Read(Operation::Increment))
            | (0x35, Phase::Read(Operation::Decrement)) => {
                Some(IndexedMemoryKind::ReadModify { address })
            }
            (
                0x34 | 0x35,
                Phase::Write {
                    value,
                    flags,
                    a: None,
                },
            ) => {
                let expected = if opcode == 0x34 {
                    alu::inc_flags(value.wrapping_sub(1), value, cpu.regs.f)
                } else {
                    alu::dec_flags(value.wrapping_add(1), value, cpu.regs.f)
                };
                (flags == expected).then_some(IndexedMemoryKind::Write { address, value })
            }
            _ => None,
        }
    }
}
