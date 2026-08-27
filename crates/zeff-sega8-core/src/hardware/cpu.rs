use super::bus::Bus;

pub use zeff_z80::*;

pub const SEGA8_RESET_STATE: ResetState = ResetState::new(Z80_RESET_PC, 0xDFF0);

impl Z80Bus for Bus {
    #[inline]
    fn cpu_read(&self, addr: u16) -> u8 {
        Bus::cpu_read(self, addr)
    }

    #[inline]
    fn cpu_write(&mut self, addr: u16, value: u8) {
        Bus::cpu_write(self, addr, value);
    }

    #[inline]
    fn io_read(&mut self, port: u8) -> u8 {
        Bus::io_read(self, port)
    }

    #[inline]
    fn io_write(&mut self, port: u8, value: u8) {
        Bus::io_write(self, port, value);
    }

    #[inline]
    fn maskable_interrupt_pending(&self) -> bool {
        Bus::maskable_interrupt_pending(self)
    }

    #[inline]
    fn non_maskable_interrupt_pending(&self) -> bool {
        Bus::non_maskable_interrupt_pending(self)
    }

    #[inline]
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        Bus::acknowledge_non_maskable_interrupt(self)
    }
}
