use super::*;
use zeff_m68k::bus::FunctionCode;

fn provenance(access: BusAccess) -> bool {
    access.bus_address & 1 == 0
        && access.function_code != FunctionCode::CpuSpace
        && access.kind != AccessKind::InterruptAcknowledge
        && access.logical_address & BUS_ADDRESS_MASK
            == access.bus_address + u32::from(access.lanes == ByteLanes::Lower)
}

fn domain(address: u32) -> bool {
    address <= 0x40ffff
}

impl Bus {
    pub(super) fn preflight_realtec_read(&self, access: BusAccess) -> Result<(), BusError> {
        if self.cartridge.realtec.is_some() && access.bus_address == 0x100 {
            if !provenance(access) {
                return Err(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Read,
                });
            }
            return Err(BusError::UnqualifiedCartridgeReset {
                address: access.bus_address,
            });
        }
        Ok(())
    }

    pub(super) fn preflight_realtec_write(&self, access: BusAccess) -> Result<(), BusError> {
        if self.cartridge.realtec.is_none() || !domain(access.bus_address) {
            return Ok(());
        }
        let control = access.bus_address >= 0x400000;
        if provenance(access)
            && (!control
                || access.kind == AccessKind::Data
                    && matches!(
                        access.function_code,
                        FunctionCode::UserData | FunctionCode::SupervisorData
                    ))
        {
            Ok(())
        } else {
            Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            })
        }
    }

    pub(super) fn write_realtec(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        if self.cartridge.realtec.is_none() || !domain(access.bus_address) {
            return None;
        }
        if let Err(error) = self.preflight_realtec_write(access) {
            return Some(Err(error));
        }
        if access.bus_address >= 0x400000 && access.lanes != ByteLanes::Lower {
            let selected = if access.lanes == ByteLanes::Upper {
                (value >> 8) as u8
            } else {
                value as u8
            };
            self.cartridge
                .realtec
                .as_mut()
                .unwrap()
                .write_control(access.bus_address, selected);
        }
        Some(Ok(()))
    }

    pub(super) fn read_realtec(
        &self,
        access: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        let board = self.cartridge.realtec.as_ref()?;
        if !domain(access.bus_address) {
            return None;
        }
        let failure = || BusError::UnsupportedAccess {
            address: access.bus_address,
            direction: Direction::Read,
        };
        if !provenance(access) {
            return Some(Err(failure()));
        }
        if let Err(error) = self.preflight_realtec_read(access) {
            return Some(Err(error));
        }
        let byte = |address| {
            board
                .source_offset(address)
                .and_then(|offset| self.cartridge.source_sample(offset))
                .ok_or_else(failure)
        };
        Some(instruction_input::lanes(access, byte))
    }
}
