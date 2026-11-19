use super::{Cartridge, sf002::Location};
use zeff_m68k::bus::{BusAccess, ByteLanes};

impl Cartridge {
    pub(crate) fn preflight_sf002_write(&self, access: BusAccess, value: u16) -> Option<bool> {
        let board = self.sf002.as_ref()?;
        if access.bus_address > 0x3fffff
            && !super::sf002::Sf002::rejects_time_write(access.bus_address)
        {
            return None;
        }
        Some(if access.bus_address < 0x3c0000 {
            board.accepts_control(access, value)
        } else if (0x3c0000..=0x3c3ffe).contains(&access.bus_address) {
            matches!(board.decode(access.bus_address), Location::Unclaimed)
                && self
                    .save
                    .as_ref()
                    .is_some_and(super::SaveMemory::sf002_compatible)
        } else {
            false
        })
    }

    pub(crate) fn write_sf002(
        &mut self,
        access: BusAccess,
        value: u16,
        now: u64,
        hz: u64,
    ) -> Option<bool> {
        if !self.preflight_sf002_write(access, value)? {
            return Some(false);
        }
        if access.bus_address < 0x3c0000 {
            return Some(self.sf002.as_mut().unwrap().control(access, value));
        }
        let save = self.save.as_mut().unwrap();
        if access.lanes != ByteLanes::Lower {
            save.write(access.bus_address, (value >> 8) as u8, now, hz);
        }
        if access.lanes != ByteLanes::Upper {
            save.write(access.bus_address + 1, value as u8, now, hz);
        }
        Some(true)
    }
}
