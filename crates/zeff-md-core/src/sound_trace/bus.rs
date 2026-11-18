use super::AttemptCapture;
use zeff_z80::{IoWriteCycle, Z80Bus};

pub(crate) struct CapturedBus<'a, B, A> {
    pub(crate) inner: &'a mut B,
    pub(crate) attempt: &'a mut A,
}

impl<B: Z80Bus, A: AttemptCapture> Z80Bus for CapturedBus<'_, B, A> {
    fn cpu_read(&self, address: u16) -> u8 {
        let value = self.inner.cpu_read(address);
        self.attempt.read(address, value, false);
        value
    }
    fn cpu_read_instruction(&self, address: u16) -> u8 {
        let value = self.inner.cpu_read_instruction(address);
        self.attempt.fetch(address, value);
        value
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.attempt.write(address, value, false);
        self.inner.cpu_write(address, value);
    }
    fn io_read(&mut self, port: u8) -> u8 {
        let value = self.inner.io_read(port);
        self.attempt.read(u16::from(port), value, true);
        value
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.attempt.write(u16::from(port), value, true);
        self.inner.io_write(port, value);
    }
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.attempt.write(u16::from(cycle.port), cycle.value, true);
        self.inner.io_write_cycle(cycle);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.inner.maskable_interrupt_pending()
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.inner.non_maskable_interrupt_pending()
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.inner.acknowledge_non_maskable_interrupt()
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.inner.acknowledge_maskable_interrupt()
    }
    fn read_interrupt_instruction(&mut self, pc: u16) -> u8 {
        self.inner.read_interrupt_instruction(pc)
    }
}
