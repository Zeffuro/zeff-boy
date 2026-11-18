use super::*;

impl Bus {
    pub(super) fn preflight_save_latch(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        if let Some(accepted) = self.cartridge.save_latch_access(access, direction)
            && (!accepted || self.rmw.is_some())
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction,
            });
        }
        Ok(())
    }

    pub(super) fn write_latched_save(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        self.cartridge
            .write_save_latch(access, value, self.now.get(), self.region.master_hz())
            .map(|accepted| {
                if accepted {
                    Ok(())
                } else {
                    Err(BusError::UnsupportedAccess {
                        address: access.bus_address,
                        direction: Direction::Write,
                    })
                }
            })
    }
}
