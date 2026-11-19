use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};

pub(crate) fn time(address: u32) -> bool {
    (0xa13000..=0xa130ff).contains(&address)
}

pub(crate) fn memory(address: u32) -> bool {
    (address as usize) < VIEW_BYTES
}

impl Cartridge {
    pub(crate) fn download_access(&self, access: BusAccess, direction: Direction) -> Option<bool> {
        self.download.as_ref()?;
        if !memory(access.bus_address & BUS_ADDRESS_MASK)
            && !time(access.bus_address & BUS_ADDRESS_MASK)
            && !memory(access.logical_address & BUS_ADDRESS_MASK)
            && !time(access.logical_address & BUS_ADDRESS_MASK)
        {
            return None;
        }
        let lane_address = access.bus_address | u32::from(access.lanes == ByteLanes::Lower);
        if access.bus_address & 1 != 0
            || access.logical_address != lane_address
            || access.logical_address > BUS_ADDRESS_MASK
        {
            return Some(false);
        }
        let data = matches!(
            access.function_code,
            FunctionCode::UserData | FunctionCode::SupervisorData
        );
        if time(access.bus_address) {
            return Some(data && access.kind == AccessKind::Data);
        }
        let accepted = data && matches!(access.kind, AccessKind::Data | AccessKind::Stack)
            || data && direction == Direction::Read && access.kind == AccessKind::Discard
            || direction == Direction::Read
                && access.function_code == FunctionCode::SupervisorData
                && access.kind == AccessKind::Vector
            || direction == Direction::Read
                && matches!(
                    access.function_code,
                    FunctionCode::UserProgram | FunctionCode::SupervisorProgram
                )
                && matches!(
                    access.kind,
                    AccessKind::Opcode
                        | AccessKind::Data
                        | AccessKind::Extension
                        | AccessKind::Prefetch
                        | AccessKind::Discard
                );
        Some(accepted)
    }

    pub(crate) fn write_download(&mut self, access: BusAccess, value: u16) -> Option<bool> {
        if !self.download_access(access, Direction::Write)? {
            return Some(false);
        }
        let board = self.download.as_mut()?;
        if time(access.bus_address) {
            let byte = if access.lanes == ByteLanes::Upper {
                (value >> 8) as u8
            } else {
                value as u8
            };
            board.control(access.logical_address, byte);
        } else {
            if access.lanes != ByteLanes::Lower {
                board.write(access.bus_address, (value >> 8) as u8);
            }
            if access.lanes != ByteLanes::Upper {
                board.write(access.bus_address + 1, value as u8);
            }
        }
        Some(true)
    }
}
