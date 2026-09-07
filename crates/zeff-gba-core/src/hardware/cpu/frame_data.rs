use super::super::constants::{EWRAM_SIZE, IWRAM_SIZE};
use super::super::timing::{self, BusRegion};
use super::transfer::SingleTransfer;
use super::{Bus, CpuBusOperation};

#[derive(Clone, Copy)]
pub(super) enum ResolvedData {
    Ewram(u32),
    Iwram(u32),
    GamePak(u32),
}

impl ResolvedData {
    pub(super) fn new(bus: &Bus, transfer: SingleTransfer) -> Option<Self> {
        let address = transfer.address & !(u32::from(transfer.width) - 1);
        match timing::region_for_addr(address) {
            BusRegion::Ewram => Some(Self::Ewram(address & (EWRAM_SIZE as u32 - 1))),
            BusRegion::Iwram => Some(Self::Iwram(address & (IWRAM_SIZE as u32 - 1))),
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2
                if transfer.operation == CpuBusOperation::Read
                    && !bus.cartridge.is_eeprom_access_addr(address)
                    && !bus.cartridge.has_rtc()
                    && (address & 0x01FF_FFFF) as usize + usize::from(transfer.width)
                        <= bus.cartridge.rom().len() =>
            {
                Some(Self::GamePak(address & 0x01FF_FFFF))
            }
            _ => None,
        }
    }

    pub(super) fn read(self, bus: &Bus, width: u8) -> u32 {
        let (memory, offset) = match self {
            Self::Ewram(offset) => (bus.ewram.as_slice(), offset),
            Self::Iwram(offset) => (bus.iwram.as_slice(), offset),
            Self::GamePak(offset) => (bus.cartridge.rom(), offset),
        };
        let offset = offset as usize;
        match width {
            1 => u32::from(memory[offset]),
            2 => u32::from(u16::from_le_bytes(
                memory[offset..offset + 2]
                    .try_into()
                    .expect("validated direct halfword read"),
            )),
            4 => u32::from_le_bytes(
                memory[offset..offset + 4]
                    .try_into()
                    .expect("validated direct word read"),
            ),
            _ => unreachable!("invalid direct read width"),
        }
    }

    pub(super) fn write(self, bus: &mut Bus, width: u8, value: u32) {
        let (memory, offset) = match self {
            Self::Ewram(offset) => (bus.ewram.as_mut_slice(), offset),
            Self::Iwram(offset) => (bus.iwram.as_mut_slice(), offset),
            Self::GamePak(_) => unreachable!("direct ROM write rejected during planning"),
        };
        let offset = offset as usize;
        let width = usize::from(width);
        memory[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
}
