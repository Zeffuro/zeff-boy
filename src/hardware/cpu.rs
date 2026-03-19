use crate::hardware::mmu::MMU;
use crate::hardware::opcodes::{control, load, arithmetic};

pub struct CPU<'a> {
    pub pc: u16,
    pub mmu: &'a mut MMU,
}

impl<'a> CPU<'a> {
    pub fn new(mmu: &'a mut MMU) -> Self {
        Self { mmu, pc: 0x100 }
    }

    pub fn step(&mut self) {
        let opcode = self.mmu.read_byte(self.pc);
        match opcode {
            0x00 => control::nop(self),
            //0xC3 => control::jp_a16(self),
            _ => log::warn!("Unimplemented opcode {:02X} at PC={:04X}", opcode, self.pc),
        }
    }
}