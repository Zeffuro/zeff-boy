use super::*;

impl Bus {
    pub(crate) fn pending_interrupt_vector(&self) -> Option<u8> {
        highest_interrupt_id(self.io[usize::from(IRQ_STATUS_PORT)])
            .map(|id| (self.io[usize::from(IRQ_VECTOR_BASE_PORT)] & 0xF8) | id)
    }

    pub(crate) fn has_pending_interrupt_signal(&self) -> bool {
        self.io[usize::from(IRQ_STATUS_PORT)] != 0
    }

    pub(super) fn interrupt_base_read(&self) -> u8 {
        (self.io[usize::from(IRQ_VECTOR_BASE_PORT)] & 0xF8)
            | highest_interrupt_id(self.io[usize::from(IRQ_STATUS_PORT)]).unwrap_or(0)
    }

    pub(crate) fn raise_keypad_interrupt(&mut self) {
        self.raise_interrupt(IRQ_KEYPAD);
    }

    pub(super) fn raise_interrupt(&mut self, mask: u8) {
        if self.io[usize::from(IRQ_ENABLE_PORT)] & mask != 0 {
            self.io[usize::from(IRQ_STATUS_PORT)] |= mask;
        }
    }

    pub(super) fn refresh_level_interrupts(&mut self) {
        let serial_status = self.serial_control_read();
        let serial_enabled = serial_status & SERIAL_CONTROL_ENABLE != 0;
        let irq_enable = self.io[usize::from(IRQ_ENABLE_PORT)];
        if serial_enabled
            && serial_status & SERIAL_STATUS_TX_EMPTY != 0
            && irq_enable & IRQ_SERIAL_TX != 0
        {
            self.io[usize::from(IRQ_STATUS_PORT)] |= IRQ_SERIAL_TX;
        }
        if serial_enabled
            && serial_status & SERIAL_STATUS_RX_READY != 0
            && irq_enable & IRQ_SERIAL_RX != 0
        {
            self.io[usize::from(IRQ_STATUS_PORT)] |= IRQ_SERIAL_RX;
        }
    }
}

fn highest_interrupt_id(pending: u8) -> Option<u8> {
    (pending != 0).then(|| 7 - pending.leading_zeros() as u8)
}
