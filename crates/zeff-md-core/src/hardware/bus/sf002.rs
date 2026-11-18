use super::*;

impl Bus {
    pub(super) fn preflight_sf002_write(
        &self,
        access: BusAccess,
        value: u16,
    ) -> Result<(), BusError> {
        if self.cartridge.preflight_sf002_write(access, value) == Some(false) {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn read_sf002(
        &self,
        access: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        if self.cartridge.sf002.is_none() || access.bus_address > CARTRIDGE_END {
            return None;
        }
        let byte = |address| {
            self.cartridge
                .read_sampled(address)
                .ok_or(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                })
        };
        Some(instruction_input::lanes(access, byte))
    }
}
