use super::{CPU_CYCLES_PER_FRAME, Emulator};
use crate::hardware::cpu::CpuState;

impl Emulator {

    pub fn step_instruction(&mut self) -> (u16, u8, u64) {
        if self.cpu.state == CpuState::Suspended {
            return (self.cpu.pc, self.bus.cpu_read(self.cpu.pc), 0);
        }

        let pc_before = self.cpu.pc;
        let opcode = self.bus.cpu_read(pc_before);

        let cycles = self.cpu.step(&mut self.bus);
        
        self.cpu.irq_line = self.bus.apu.irq_pending();

        let nmi = self.bus.tick_peripherals(cycles);

        if nmi {
            self.cpu.nmi_pending = true;
        }

        (pc_before, opcode, cycles)
    }

    pub fn step_frame(&mut self) {
        if self.cpu.state == CpuState::Suspended {
            return;
        }

        let target = self.cpu.cycles.wrapping_add(CPU_CYCLES_PER_FRAME);

        while self.cpu.cycles < target && self.cpu.state == CpuState::Running {
            self.step_instruction();
        }
    }
}

