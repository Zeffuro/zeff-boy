use super::*;
use crate::cartridge::sf001::Location;

impl Bus {
    pub(super) fn read_sf001(
        &self,
        access: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        let board = self.cartridge.sf001.as_ref()?;
        let address = access.bus_address;
        if board.decode(address) == Location::Unclaimed {
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

    pub(super) fn write_sf001(&mut self, access: BusAccess, value: u16) -> bool {
        let Some(board) = &mut self.cartridge.sf001 else {
            return false;
        };
        let address = access.bus_address + u32::from(access.lanes == ByteLanes::Lower);
        let byte = if access.lanes == ByteLanes::Upper {
            (value >> 8) as u8
        } else {
            value as u8
        };
        if board.control(address, byte) {
            return true;
        }
        match access.lanes {
            ByteLanes::Upper => board.write(address, byte),
            ByteLanes::Lower => board.write(address, byte),
            ByteLanes::Both => {
                if board.decode(address) == Location::Unclaimed
                    && !(0xa130f0..=0xa130ff).contains(&address)
                {
                    return false;
                }
                board.write(address, (value >> 8) as u8) && board.write(address + 1, value as u8)
            }
        }
    }
}

#[cfg(test)]
mod tests;
