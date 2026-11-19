use super::*;
use crate::cartridge::parallel_adapter::Model;
use zeff_m68k::bus::FunctionCode;

impl Bus {
    pub(super) fn discarded_parallel_adapter_read(&self, access: BusAccess) -> bool {
        self.cartridge
            .parallel_adapter
            .as_ref()
            .is_some_and(|board| match board.model() {
                Model::First => (0x430000..=0x43fffe).contains(&access.bus_address),
                Model::Second => (0x610000..=0x61fffe).contains(&access.bus_address),
            })
            && access.bus_address & 1 == 0
            && access.logical_address & BUS_ADDRESS_MASK == access.bus_address
            && access.kind == AccessKind::Data
            && access.lanes == ByteLanes::Both
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
    }

    pub(super) fn preflight_parallel_adapter_write(
        &self,
        access: BusAccess,
        value: u16,
    ) -> Result<(), BusError> {
        if self
            .cartridge
            .parallel_adapter
            .as_ref()
            .and_then(|board| board.check_write(access, value))
            .is_some_and(|result| result.is_err())
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn read_parallel_adapter(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        let board = self.cartridge.parallel_adapter.as_ref()?;
        let control = board.guarded(access.bus_address);
        if !control && !board.contains(access.bus_address) {
            return None;
        }
        let failure = || BusError::UnsupportedAccess {
            address: access.bus_address,
            direction: Direction::Read,
        };
        if control || access.lanes == ByteLanes::Lower || access.bus_address & 1 != 0 {
            return Some(Err(failure()));
        }
        let high = board.read(access.bus_address).ok_or_else(failure);
        Some(high.and_then(|high| {
            if access.lanes == ByteLanes::Upper {
                Ok(u16::from(high) << 8)
            } else {
                board
                    .read(access.bus_address + 1)
                    .map(|low| u16::from_be_bytes([high, low]))
                    .ok_or_else(failure)
            }
        }))
    }
}
