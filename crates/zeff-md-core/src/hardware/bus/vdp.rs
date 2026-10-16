use super::*;

impl Bus {
    pub(super) fn vdp_data_wait(&self, access: BusAccess) -> Result<Option<u32>, BusError> {
        self.vdp_wait(access, false)
    }

    pub(super) fn vdp_read_wait(&mut self, access: BusAccess) -> Result<Option<u32>, BusError> {
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && access.bus_address & VDP_PORT_MASK <= VDP_DATA_END
        {
            self.prefetch_upper = None;
        }
        self.vdp_wait(access, true)
    }

    fn vdp_wait(&self, access: BusAccess, read: bool) -> Result<Option<u32>, BusError> {
        if access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || access.bus_address & VDP_PORT_MASK > VDP_DATA_END
        {
            return Ok(None);
        }
        let wait = if read {
            self.vdp.data_read_wait(self.region)
        } else {
            self.vdp.data_write_wait(self.region)
        };
        let Some(deadline) = wait.map_err(BusError::VdpPort)? else {
            return Ok(None);
        };
        let clocks = u32::try_from((deadline.get() - self.now.get()).div_ceil(MAIN_CPU_DIVISOR))
            .map_err(|_| BusError::ClockOverflow)?;
        Ok(Some(clocks))
    }

    pub(super) fn read_vdp(&mut self, access: BusAccess) -> Option<Result<u16, BusError>> {
        if access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || access.bus_address & VDP_PORT_MASK > VDP_HV_END
        {
            return None;
        }
        if access.bus_address & VDP_PORT_MASK >= VDP_HV_START {
            return Some(self.vdp.read_hv(self.region).map_err(BusError::Vdp));
        }
        if access.bus_address & VDP_PORT_MASK >= VDP_CONTROL_START {
            let upper = if access.lanes == ByteLanes::Lower {
                0
            } else {
                let upper = self.prefetch_upper.unwrap_or(0);
                (u16::from(upper) << 8) & VDP_STATUS_FLOAT_MASK
            };
            return Some(
                self.vdp
                    .read_cpu_status(self.region)
                    .map(|word| word | upper)
                    .map_err(BusError::VdpPort),
            );
        }
        Some(self.vdp.read_data(self.region).map_err(BusError::VdpPort))
    }

    pub(super) fn write_vdp(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        let address = access.bus_address;
        if address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || address & VDP_PORT_MASK > VDP_CONTROL_END
        {
            return None;
        }
        let [high, low] = value.to_be_bytes();
        let word = match access.lanes {
            ByteLanes::Both => value,
            ByteLanes::Upper => u16::from_be_bytes([high; 2]),
            ByteLanes::Lower => u16::from_be_bytes([low; 2]),
        };
        let result = if address & VDP_PORT_MASK < VDP_CONTROL_START {
            self.vdp.write_data(word, self.region)
        } else {
            self.vdp
                .write_control_from(
                    word,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
                )
                .map(|_| ())
        };
        Some(result.map_err(BusError::VdpPort))
    }
}
