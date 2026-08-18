use super::*;

impl Cpu {
    pub(super) fn try_service_non_maskable_interrupt<B: SegaCpuBus>(
        &mut self,
        bus: &mut B,
    ) -> Option<FetchedInstruction> {
        if !bus.non_maskable_interrupt_pending() {
            return None;
        }

        let pc = self.regs.pc;
        bus.acknowledge_non_maskable_interrupt();
        self.state = CpuState::Running;
        self.interrupt_flip_flop_1 = false;
        self.enable_interrupts_delay = 0;
        self.push_u16(bus, self.regs.pc);
        self.regs.pc = Z80_INTERRUPT_VECTOR_NMI;
        self.last_opcode_pc = pc;
        self.last_opcode = Z80_INTERRUPT_ACK_OPCODE;

        let cycles = CYCLES_NMI_ACK;
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        Some(FetchedInstruction {
            pc,
            opcode: Z80_INTERRUPT_ACK_OPCODE,
            cycles,
        })
    }

    pub(super) fn try_service_maskable_interrupt<B: SegaCpuBus>(
        &mut self,
        bus: &mut B,
    ) -> Option<FetchedInstruction> {
        if !self.interrupt_flip_flop_1
            || self.enable_interrupts_delay != 0
            || !bus.maskable_interrupt_pending()
        {
            return None;
        }

        let pc = self.regs.pc;
        self.state = CpuState::Running;
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
        self.push_u16(bus, self.regs.pc);
        self.regs.pc = self.maskable_interrupt_vector();
        self.last_opcode_pc = pc;
        self.last_opcode = Z80_INTERRUPT_ACK_OPCODE;

        let cycles = CYCLES_INTERRUPT_ACK;
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        Some(FetchedInstruction {
            pc,
            opcode: Z80_INTERRUPT_ACK_OPCODE,
            cycles,
        })
    }

    fn maskable_interrupt_vector(&self) -> u16 {
        match self.interrupt_mode {
            InterruptMode::Im0 | InterruptMode::Im1 | InterruptMode::Im2 => {
                Z80_INTERRUPT_VECTOR_IM1
            }
        }
    }

    pub(super) fn finish_instruction(&mut self, cycles: u32) {
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        if self.enable_interrupts_delay > 0 {
            self.enable_interrupts_delay -= 1;
            if self.enable_interrupts_delay == 0 {
                self.interrupt_flip_flop_1 = true;
                self.interrupt_flip_flop_2 = true;
            }
        }
    }

    pub(super) fn disable_interrupts(&mut self) {
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
    }

    pub(super) fn schedule_enable_interrupts(&mut self) {
        self.enable_interrupts_delay = 2;
    }
}
