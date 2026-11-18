use super::*;

impl SaveMemory {
    pub(crate) fn fixed_latched_odd32(&self) -> bool {
        self.layout == OddSaveLayout::KiB32
            && self.start == SAVE_START
            && !self.switched
            && self.initially_enabled
            && !self.time_latch
            && self.serial.is_none()
            && self.bytes.len() == 32768
    }

    pub(crate) fn validate_state_with_fixed_latch(&self) -> Result<()> {
        ensure!(
            self.fixed_latched_odd32(),
            "invalid MD fixed save latch geometry"
        );
        self.validate_state_inner(true)
    }

    pub(crate) fn write_with_fixed_latch(
        &mut self,
        address: u32,
        value: u8,
        now: u64,
        master_hz: u64,
    ) {
        if self.fixed_latched_odd32() && self.enabled && self.contains(address) {
            self.write(address, value, now, master_hz);
        }
    }
}
