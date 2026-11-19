use super::*;

pub(super) struct LocalBus<'a> {
    pub memory: &'a SoundMemory,
    pub rejected: Cell<bool>,
}

impl Z80Bus for LocalBus<'_> {
    fn cpu_read(&self, address: u16) -> u8 {
        if address <= SOUND_RAM_END {
            self.memory.ram[usize::from(address) & SOUND_RAM_MASK]
        } else {
            self.rejected.set(true);
            SOUND_DISCONNECTED_BYTE
        }
    }

    fn cpu_write(&mut self, _: u16, _: u8) {
        self.rejected.set(true);
    }

    fn io_read(&mut self, _: u8) -> u8 {
        self.rejected.set(true);
        SOUND_DISCONNECTED_BYTE
    }

    fn io_write(&mut self, _: u8, _: u8) {
        self.rejected.set(true);
    }

    fn maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.rejected.set(true);
        false
    }

    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.rejected.set(true);
        zeff_z80::Z80_INTERRUPT_ACK_OPCODE
    }
}
