use super::*;

mod clock;

fn domain(address: u32) -> bool {
    address <= CARTRIDGE_UNUSED_END
        || (0xa13000..0xa13100).contains(&address)
        || (0xa15000..0xa15100).contains(&address)
}

fn failure(access: BusAccess, direction: Direction, cause: crate::svp::Error) -> BusError {
    BusError::Svp {
        address: access.bus_address,
        direction,
        cause,
    }
}

impl Bus {
    pub(super) fn preflight_svp(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        let Some(svp) = &self.cartridge.svp else {
            return Ok(());
        };
        if !domain(access.bus_address) {
            return Ok(());
        }
        let mutable = match (access.bus_address, direction, access.lanes) {
            (0..=0x1ffffe, Direction::Read, _)
                if access.bus_address as usize + 1 < self.cartridge.bytes.len() =>
            {
                false
            }
            (0x300000..=0x31fffe, Direction::Read, _) => false,
            (0xa15006, Direction::Write, ByteLanes::Both) if svp.boundary_control_enabled() => true,
            (0xa15000 | 0xa15002, Direction::Read, ByteLanes::Both) => false,
            (0x300000..=0x31fffe, Direction::Write, _)
            | (0xa15000 | 0xa15002, Direction::Write, ByteLanes::Both)
            | (0xa15004, Direction::Read, ByteLanes::Both | ByteLanes::Lower) => true,
            _ => {
                return Err(failure(
                    access,
                    direction,
                    crate::svp::Error::Image("unqualified MAIN cartridge access"),
                ));
            }
        };
        if mutable && svp.functional_timing().is_none() {
            svp.main_boundary()
                .map_err(|cause| failure(access, direction, cause))?;
        }
        Ok(())
    }

    pub(super) fn preflight_svp_write(
        &self,
        access: BusAccess,
        value: u16,
    ) -> Result<(), BusError> {
        self.preflight_svp(access, Direction::Write)?;
        if access.bus_address == 0xa15006
            && let Some(svp) = &self.cartridge.svp
        {
            (if svp.functional_timing().is_some() {
                svp.preflight_control_value(value)
            } else {
                svp.preflight_control(value)
            })
            .map_err(|cause| failure(access, Direction::Write, cause))?;
        }
        Ok(())
    }

    pub(super) fn peek_svp(&self, address: u32) -> Option<Option<u8>> {
        let svp = self.cartridge.svp.as_ref()?;
        if !domain(address) {
            return None;
        }
        Some(match address {
            0xa15000..=0xa15003 => Some(svp.peek_result().to_be_bytes()[address as usize & 1]),
            0xa15004..=0xa15005 => Some(svp.peek_status().to_be_bytes()[address as usize & 1]),
            _ => self.cartridge.read(address),
        })
    }

    pub(super) fn read_svp(&mut self, access: BusAccess) -> Option<Result<u16, BusError>> {
        self.cartridge.svp.as_ref()?;
        if !domain(access.bus_address) {
            return None;
        }
        Some((|| {
            self.preflight_svp(access, Direction::Read)?;
            let svp = self.cartridge.svp.as_mut().unwrap();
            match access.bus_address {
                0x300000..=0x31fffe => {
                    Ok(svp.peek_dram(((access.bus_address - 0x300000) >> 1) as u16))
                }
                0xa15000 | 0xa15002 => Ok(svp.peek_result()),
                0xa15004 => svp
                    .consume_main_status()
                    .map_err(|cause| failure(access, Direction::Read, cause)),
                _ => {
                    let address = access.bus_address;
                    let high = self.cartridge.read(address);
                    let low = self.cartridge.read(address + 1);
                    high.zip(low)
                        .map(|(high, low)| u16::from_be_bytes([high, low]))
                        .ok_or_else(|| {
                            failure(
                                access,
                                Direction::Read,
                                crate::svp::Error::Image("MAIN ROM read outside supplied image"),
                            )
                        })
                }
            }
        })())
    }

    pub(super) fn write_svp(
        &mut self,
        access: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        self.cartridge.svp.as_ref()?;
        if !domain(access.bus_address) {
            return None;
        }
        Some((|| {
            self.preflight_svp_write(access, value)?;
            let svp = self.cartridge.svp.as_mut().unwrap();
            let result = match access.bus_address {
                0x300000..=0x31fffe => {
                    let mask = match access.lanes {
                        ByteLanes::Both => 0xffff,
                        ByteLanes::Upper => 0xff00,
                        ByteLanes::Lower => 0x00ff,
                    };
                    svp.main_dram_write(((access.bus_address - 0x300000) >> 1) as u16, value, mask)
                }
                0xa15000 | 0xa15002 => svp.publish_main_command(value),
                0xa15006 => svp.main_control(value),
                _ => unreachable!(),
            };
            result.map_err(|cause| failure(access, Direction::Write, cause))
        })())
    }
}
