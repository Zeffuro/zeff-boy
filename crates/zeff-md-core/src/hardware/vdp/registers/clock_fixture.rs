use super::*;

impl VdpRegisterPipeline {
    pub(in crate::hardware::vdp) fn clock_fixture(
        epoch: MasterTicks,
        now: MasterTicks,
        image: [u8; REGISTER_COUNT as usize],
    ) -> Self {
        assert!(now >= epoch);
        Self {
            epoch,
            now,
            pending: None,
            registers: image.map(Some),
        }
    }

    pub(in crate::hardware::vdp) fn queue_clock_fixture(
        &mut self,
        index: u8,
        value: u8,
    ) -> Result<VdpRegisterWrite, VdpRegisterError> {
        let queued = if index == DISPLAY_SIZE_REGISTER && value & H40_CLOCK_MASK == 1 {
            value | 0x80
        } else {
            value
        };
        let mut write = self.queue_control_register(index, queued)?;
        write.value = value;
        self.pending = Some(write);
        Ok(write)
    }
}
