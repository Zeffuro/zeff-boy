use super::*;
use crate::cartridge::sf004::Sf004;
use zeff_m68k::bus::FunctionCode;

fn data_lower(access: BusAccess) -> bool {
    access.kind == AccessKind::Data
        && access.lanes == ByteLanes::Lower
        && matches!(
            access.function_code,
            FunctionCode::UserData | FunctionCode::SupervisorData
        )
        && access.bus_address & 1 == 0
        && access.logical_address & BUS_ADDRESS_MASK == access.bus_address + 1
}

impl Bus {
    pub(super) fn preflight_sf004_write(
        &self,
        access: BusAccess,
        value: u16,
    ) -> Result<(), BusError> {
        let Some(board) = &self.cartridge.sf004 else {
            return Ok(());
        };
        if !Sf004::domain(access.bus_address) {
            return Ok(());
        }
        if !data_lower(access) || !board.accepts_write(access.bus_address + 1, value as u8) {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn read_sf004(
        &self,
        access: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        let board = self.cartridge.sf004.as_ref()?;
        let address = access.bus_address;
        if !Sf004::domain(address) {
            return None;
        }
        let failure = || BusError::UnsupportedAccess {
            address,
            direction: Direction::Read,
        };
        let logical = access.logical_address & BUS_ADDRESS_MASK;
        if access.function_code == FunctionCode::CpuSpace
            || access.kind == AccessKind::InterruptAcknowledge
            || address & 1 != 0
            || logical != address + u32::from(access.lanes == ByteLanes::Lower)
        {
            return Some(Err(failure()));
        }
        if (0xa13000..=0xa130fe).contains(&address) {
            return Some(if data_lower(access) {
                Ok(InstructionInput::unknown(u16::from(board.time_read())))
            } else {
                Err(failure())
            });
        }
        if address >= 0x200000 && !data_lower(access) {
            return Some(Err(failure()));
        }
        let byte = |at| self.cartridge.read_sampled(at).ok_or_else(failure);
        Some(instruction_input::lanes(access, byte))
    }
}
