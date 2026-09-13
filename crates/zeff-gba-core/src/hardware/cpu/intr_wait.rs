use super::{Bus, Cpu};

const INTR_WAIT_RETURN_INSTRUCTION_CYCLES: [u8; 19] =
    [3, 1, 2, 3, 1, 1, 2, 2, 3, 1, 4, 3, 4, 1, 1, 3, 1, 5, 3];
const INTR_WAIT_FLAG_READ_INSTRUCTION: usize = 3;
const INTR_WAIT_FLAG_CLEAR_INSTRUCTION: usize = 6;
const INTR_WAIT_RESULT_BRANCH_INSTRUCTION: usize = 9;

impl Cpu {
    pub(super) fn swi_intr_wait(&mut self, bus: &mut Bus, discard_old_flags: bool, mask: u16) {
        let mask = mask & 0x3FFF;
        if discard_old_flags {
            bus.clear_bios_irq_flags(mask);
        }

        let ready = bus.bios_irq_flags() & mask;
        bus.enable_master_interrupts();
        if ready != 0 {
            bus.clear_bios_irq_flags(ready);
            self.swi_wait_return_pc = None;
            self.swi_wait_mask = 0;
            return;
        }

        self.swi_wait_return_pc = Some(self.pc());
        self.swi_wait_mask = mask;
        self.state = super::CpuState::Halted;
    }

    pub(super) fn complete_swi_wait(&mut self, bus: &mut Bus) -> bool {
        let mut ready = 0;
        for (instruction, cycles) in INTR_WAIT_RETURN_INSTRUCTION_CYCLES.into_iter().enumerate() {
            let cycles = match (instruction, ready) {
                (INTR_WAIT_FLAG_CLEAR_INSTRUCTION, 0) => 1,
                (INTR_WAIT_RESULT_BRANCH_INSTRUCTION, 0) => 3,
                _ => u32::from(cycles),
            };
            self.cycles = self.cycles.wrapping_add(u64::from(cycles));
            bus.step_cycles(cycles);
            if instruction == INTR_WAIT_FLAG_READ_INSTRUCTION {
                ready = bus.bios_irq_flags() & self.swi_wait_mask;
            } else if instruction == INTR_WAIT_FLAG_CLEAR_INSTRUCTION && ready != 0 {
                bus.clear_bios_irq_flags(ready);
            } else if instruction == INTR_WAIT_RESULT_BRANCH_INSTRUCTION && ready == 0 {
                // An unrelated IRQ wakes HALT but does not complete the BIOS wait.
                self.cycles = self.cycles.wrapping_add(2);
                bus.step_cycles(2);
                self.state = super::CpuState::Halted;
                return false;
            }
        }
        self.swi_wait_mask = 0;
        true
    }
}
