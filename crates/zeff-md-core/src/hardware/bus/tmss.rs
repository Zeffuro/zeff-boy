use super::*;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) fn contract() {
    tests::contract();
}

impl Bus {
    pub(crate) fn initial_registers(&self) -> zeff_m68k::Registers {
        self.tmss
            .as_ref()
            .filter(|tmss| tmss.boot_mapped())
            .map_or_else(|| self.cartridge.effective_initial(), |tmss| tmss.initial())
    }

    pub(super) fn read_tmss(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        let tmss = self.tmss.as_ref()?;
        let address = access.bus_address;
        if !matches!(address, 0xa14000 | 0xa14002 | 0xa14100) {
            return None;
        }
        Some(
            if matches!(address, 0xa14000 | 0xa14002) && access.lanes == ByteLanes::Both {
                Ok(tmss.lock_word(((address >> 1) & 1) as usize))
            } else if address == 0xa14100 && access.lanes != ByteLanes::Upper {
                self.prefetch
                    .sample(access.lanes)
                    .ok_or(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    })
            } else {
                Err(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                })
            },
        )
    }

    pub(super) fn write_tmss(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        let tmss = self.tmss.as_mut()?;
        let address = access.bus_address;
        if !matches!(address, 0xa14000 | 0xa14002 | 0xa14100) {
            return None;
        }
        if matches!(address, 0xa14000 | 0xa14002) && access.lanes == ByteLanes::Both {
            tmss.write_lock(((address >> 1) & 1) as usize, value);
            return Some(Ok(()));
        }
        if address == 0xa14100 && access.lanes != ByteLanes::Upper {
            if value & 1 == 0 && !self.sound.reset_asserted {
                return Some(Err(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                }));
            }
            tmss.select_cartridge(value & 1 != 0);
            return Some(Ok(()));
        }
        Some(Err(BusError::UnsupportedAccess {
            address,
            direction: Direction::Write,
        }))
    }

    pub(super) fn preflight_tmss_vdp_lock(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && self.tmss.as_ref().is_some_and(|tmss| !tmss.unlocked())
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction,
            });
        }
        Ok(())
    }

    pub(super) fn preflight_tmss(
        &self,
        access: BusAccess,
        value: Option<u16>,
    ) -> Result<(), BusError> {
        let direction = if value.is_some() {
            Direction::Write
        } else {
            Direction::Read
        };
        self.preflight_tmss_vdp_lock(access, direction)?;
        let Some(tmss) = &self.tmss else {
            return Ok(());
        };
        let address = access.bus_address;
        let sound_boot = tmss.boot_mapped()
            && address & SOUND_CONTROL_ADDRESS_MASK == SOUND_RESET
            && access.lanes != ByteLanes::Lower
            && value.is_some_and(|word| word & SOUND_CONTROL_MASK != 0);
        if sound_boot {
            return Err(BusError::UnsupportedAccess { address, direction });
        }
        Ok(())
    }
}
