use super::*;

impl PceMachine {
    #[inline]
    pub fn rom_offset_for_cpu_address(&self, logical_addr: u16) -> Option<u32> {
        self.bus
            .hucard_rom_offset(self.cpu.cpu().logical_to_physical(logical_addr))
    }

    pub fn rom_mapping_token(&self) -> u64 {
        self.cpu
            .cpu()
            .mapping_registers()
            .into_iter()
            .chain([self.bus.hucard_mapping_token()])
            .fold(0xCBF2_9CE4_8422_2325, |token, byte| {
                (token ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
            })
    }

    #[inline]
    pub fn hucard_rom(&self) -> &[u8] {
        self.bus.hucard_rom()
    }

    #[inline]
    pub fn hucard_board(&self) -> PceHuCardBoard {
        self.bus.hucard_board()
    }

    #[inline]
    pub fn hucard_ram(&self) -> Option<&[u8; POPULOUS_HUCARD_RAM_LEN]> {
        self.bus.hucard_ram()
    }

    #[cfg(test)]
    pub(in super::super) fn system_card_ram_mut_for_test(
        &mut self,
    ) -> Option<&mut [u8; super::super::cartridge::SUPER_SYSTEM_CARD_RAM_LEN]> {
        self.bus.system_card_ram_mut()
    }

    #[inline]
    pub fn work_ram(&self) -> &[u8; super::super::bus::WORK_RAM_LEN] {
        self.bus.work_ram()
    }

    #[inline]
    pub fn mapped_work_ram(&self) -> &[u8] {
        self.bus.mapped_work_ram()
    }

    #[inline]
    pub fn mapped_work_ram_mut(&mut self) -> &mut [u8] {
        self.bus.mapped_work_ram_mut()
    }

    pub(crate) fn cheat_peek_physical_ram(&self, address: u32) -> Option<u8> {
        match address {
            super::super::constants::PCE_PHYSICAL_WORK_RAM_START
                ..=super::super::constants::PCE_PHYSICAL_WORK_RAM_END => {
                let offset = (address - super::super::constants::PCE_PHYSICAL_WORK_RAM_START)
                    as usize
                    & (self.bus.mapped_work_ram().len() - 1);
                Some(self.bus.mapped_work_ram()[offset])
            }
            super::super::cartridge::SUPER_SYSTEM_CARD_RAM_START
                ..=super::super::cartridge::SUPER_SYSTEM_CARD_RAM_END => {
                self.bus.system_card_ram().map(|ram| {
                    ram[(address - super::super::cartridge::SUPER_SYSTEM_CARD_RAM_START) as usize]
                })
            }
            super::super::cdrom2::CDROM2_WORK_RAM_START
                ..=super::super::cdrom2::CDROM2_WORK_RAM_END => self
                .bus
                .devices()
                .cdrom2()
                .and_then(|cdrom2| cdrom2.peek_physical(address)),
            _ => None,
        }
    }

    pub(crate) fn cheat_write_physical_ram(&mut self, address: u32, value: u8) {
        match address {
            super::super::constants::PCE_PHYSICAL_WORK_RAM_START
                ..=super::super::constants::PCE_PHYSICAL_WORK_RAM_END => {
                let offset = (address - super::super::constants::PCE_PHYSICAL_WORK_RAM_START)
                    as usize
                    & (self.bus.mapped_work_ram().len() - 1);
                self.bus.mapped_work_ram_mut()[offset] = value;
            }
            super::super::cartridge::SUPER_SYSTEM_CARD_RAM_START
                ..=super::super::cartridge::SUPER_SYSTEM_CARD_RAM_END => {
                if let Some(ram) = self.bus.system_card_ram_mut() {
                    ram[(address - super::super::cartridge::SUPER_SYSTEM_CARD_RAM_START)
                        as usize] = value;
                }
            }
            super::super::cdrom2::CDROM2_WORK_RAM_START
                ..=super::super::cdrom2::CDROM2_WORK_RAM_END => {
                if let Some(cdrom2) = self.bus.devices_mut().cdrom2_mut() {
                    cdrom2.write_physical(address, value);
                }
            }
            _ => {}
        }
    }

    #[inline]
    pub const fn hardware_topology(&self) -> PceHardwareTopology {
        self.bus.topology()
    }
}
