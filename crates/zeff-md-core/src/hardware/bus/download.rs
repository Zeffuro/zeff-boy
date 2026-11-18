use super::*;
use crate::cartridge::download;

impl Bus {
    pub(super) fn preflight_download(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        if let Some(accepted) = self.cartridge.download_access(access, direction)
            && (!accepted || self.rmw.is_some())
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction,
            });
        }
        Ok(())
    }

    pub(super) fn download_observation(&self, access: BusAccess) -> bool {
        self.cartridge.download.is_some()
            && download::time(access.bus_address)
            && self.rmw.is_none()
            && self.cartridge.download_access(access, Direction::Read) == Some(true)
    }

    pub(super) fn read_download(
        &self,
        access: BusAccess,
        main_data: bool,
    ) -> Option<Result<u16, BusError>> {
        self.cartridge.download.as_ref()?;
        if !download::time(access.bus_address) {
            return None;
        }
        Some(if main_data && self.download_observation(access) {
            self.prefetch
                .sample(access.lanes)
                .ok_or(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Read,
                })
        } else {
            Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Read,
            })
        })
    }

    pub(super) fn write_download(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        self.cartridge
            .write_download(access, value)
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
