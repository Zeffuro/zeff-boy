use super::VdpDmaError;
use crate::constants::{CARTRIDGE_END, UNPOPULATED_ROM_BYTE, WORK_RAM_MASK, WORK_RAM_START};
use zeff_m68k::bus::BUS_ADDRESS_MASK;

#[derive(Clone, Copy)]
pub struct VdpDmaSource<'a> {
    rom: &'a [u8],
    ram: &'a [u8],
    cartridge: Option<&'a crate::cartridge::Cartridge>,
}

impl<'a> VdpDmaSource<'a> {
    pub fn new(rom: &'a [u8], ram: &'a [u8]) -> Self {
        Self {
            rom,
            ram,
            cartridge: None,
        }
    }

    pub(crate) fn from_cartridge(
        cartridge: &'a crate::cartridge::Cartridge,
        ram: &'a [u8],
    ) -> Self {
        Self {
            rom: &cartridge.bytes,
            ram,
            cartridge: Some(cartridge),
        }
    }

    pub(super) fn read(self, address: u32) -> Result<u16, VdpDmaError> {
        let byte = |address| {
            if address <= CARTRIDGE_END {
                if let Some(cartridge) = self.cartridge {
                    return cartridge
                        .read(address)
                        .ok_or(VdpDmaError::UnsupportedSource(address));
                }
                Ok(self
                    .rom
                    .get(address as usize)
                    .copied()
                    .unwrap_or(UNPOPULATED_ROM_BYTE))
            } else if (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&address) {
                self.ram
                    .get(address as usize & WORK_RAM_MASK)
                    .copied()
                    .ok_or(VdpDmaError::UnsupportedSource(address))
            } else {
                Err(VdpDmaError::UnsupportedSource(address))
            }
        };
        Ok(u16::from_be_bytes([byte(address)?, byte(address + 1)?]))
    }
}
