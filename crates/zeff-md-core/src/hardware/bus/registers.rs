use super::*;
use crate::cartridge::registers::CartridgeRegisters;

impl Bus {
    pub(super) fn preflight_banked_register_write(
        &self,
        access: BusAccess,
    ) -> Result<(), BusError> {
        if self.cartridge.registers.as_ref().is_some_and(|board| {
            board.is_banked()
                && board.contains(access.bus_address)
                && !board.accepts_bank_cycle(access.bus_address, access.lanes)
        }) {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn preflight_protection_write(&self, access: BusAccess) -> Result<(), BusError> {
        if self
            .cartridge
            .protection
            .is_some_and(|board| board.rejects_write(access.bus_address))
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn read_lock_on_save(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        let save = self.cartridge.lock_on_save.as_ref()?;
        if !save.enabled || !save.contains(access.bus_address) {
            return None;
        }
        Some(
            if access.lanes == ByteLanes::Lower {
                save.read(access.bus_address + 1).map(u16::from)
            } else {
                None
            }
            .ok_or(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Read,
            }),
        )
    }

    pub(super) fn write_lock_on_save(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        let save = self.cartridge.lock_on_save.as_mut()?;
        if !save.enabled || !save.contains(access.bus_address) {
            return None;
        }
        Some(
            if access.lanes == ByteLanes::Lower && save.write(access.bus_address + 1, value as u8) {
                Ok(())
            } else {
                Err(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Write,
                })
            },
        )
    }

    pub(super) fn read_registers(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        if let Some(protection) = self.cartridge.protection
            && protection.contains(access.bus_address)
        {
            let address = access.bus_address + u32::from(access.lanes == ByteLanes::Lower);
            return Some(protection.read_cycle(address, access.lanes).ok_or(
                BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                },
            ));
        }
        let registers = self.cartridge.registers.as_ref()?;
        if !registers.contains(access.bus_address) {
            return None;
        }
        let address = access.bus_address + u32::from(access.lanes == ByteLanes::Lower);
        Some(
            registers
                .read(address)
                .map(|value| match access.lanes {
                    ByteLanes::Upper => u16::from(value) << 8,
                    _ => u16::from(value),
                })
                .ok_or(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                }),
        )
    }

    pub(super) fn write_registers(&mut self, access: BusAccess, value: u16) -> bool {
        if access.logical_address == crate::cartridge::ram::SAVE_CONTROL + 1
            && access.bus_address == crate::cartridge::ram::SAVE_CONTROL
            && access.lanes == ByteLanes::Lower
            && self.cartridge.save.is_none()
            && self.cartridge.registers.is_none()
            && self.cartridge.protection.is_none()
            && self.cartridge.sf001.is_none()
            && if let Some(save) = &mut self.cartridge.lock_on_save {
                let selected = value as u8;
                if selected <= 1 {
                    save.enabled = selected == 1;
                    true
                } else {
                    false
                }
            } else {
                self.cartridge
                    .lock_on
                    .as_mut()
                    .is_some_and(|board| board.write_control(value as u8))
            }
        {
            return true;
        }
        if self
            .cartridge
            .protection
            .is_some_and(|protection| protection.contains(access.bus_address))
        {
            return true;
        }
        let Some(registers) = &mut self.cartridge.registers else {
            return false;
        };
        if !registers.contains(access.bus_address) {
            return false;
        }
        if registers.is_banked() {
            if !registers.accepts_bank_cycle(access.bus_address, access.lanes) {
                return false;
            }
            registers.write_bank(value as u8);
            return true;
        }
        let address = access.bus_address + u32::from(access.lanes == ByteLanes::Lower);
        if let Some(index) = CartridgeRegisters::index(address) {
            let value = if access.lanes == ByteLanes::Upper {
                (value >> 8) as u8
            } else {
                value as u8
            };
            registers.write(index, value);
        }
        true
    }
}

#[cfg(test)]
mod tests;
