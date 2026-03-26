use super::Bus;
use super::io_bus;
use crate::cheats::CheatPatch;
use crate::hardware::types::constants::*;
use log::{trace, warn};

impl Bus {
    #[inline]
    #[allow(unreachable_patterns)]
    pub fn read_byte_raw(&self, addr: u16) -> u8 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => self.cartridge.read_rom(addr),
            VRAM_START..=VRAM_END => {
                if !self.io.ppu.cpu_vram_accessible() {
                    return 0xFF;
                }
                let local = (addr - VRAM_START) as usize;
                self.vram[self.active_vram_offset() + local]
            }
            ERAM_START..=ERAM_END => self.cartridge.read_ram(addr),
            WRAM_0_START..=WRAM_0_END => self.wram[(addr - WRAM_0_START) as usize],
            WRAM_N_START..=WRAM_N_END => {
                let local = (addr - WRAM_N_START) as usize;
                self.wram[self.active_wram_bank() * WRAM_SIZE + local]
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let mirror = addr - ECHO_RAM_OFFSET;
                if mirror < WRAM_N_START {
                    self.wram[(mirror - WRAM_0_START) as usize]
                } else {
                    let local = (mirror - WRAM_N_START) as usize;
                    self.wram[self.active_wram_bank() * WRAM_SIZE + local]
                }
            }
            OAM_START..=OAM_END => {
                if !self.io.ppu.cpu_oam_accessible() {
                    return 0xFF;
                }
                self.oam[(addr - OAM_START) as usize]
            }
            NOT_USABLE_START..=NOT_USABLE_END => 0xFF,
            SERIAL_SB => self.io.serial.sb(),
            SERIAL_SC => self.io.serial.sc(),
            INTERRUPT_IF => self.if_reg,
            IO_START..=IO_END => self.read_io(addr),
            HRAM_START..=HRAM_END => self.hram[(addr - HRAM_START) as usize],
            IE_ADDR => self.ie,
            _ => 0xFF,
        }
    }

    #[inline]
    #[allow(unreachable_patterns)]
    pub fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => {
                let raw = self.read_byte_raw(addr);
                if self.game_genie_patches.is_empty() {
                    return raw;
                }
                for patch in &self.game_genie_patches {
                    match *patch {
                        CheatPatch::RomWrite { address, value } if address == addr => {
                            return value.resolve_with_current(raw);
                        }
                        CheatPatch::RomWriteIfEquals {
                            address,
                            value,
                            compare,
                        } if address == addr => {
                            if compare.matches(raw) {
                                return value.resolve_with_current(raw);
                            }
                        }
                        _ => {}
                    }
                }
                raw
            }
            _ => self.read_byte_raw(addr),
        }
    }

    #[inline]
    #[allow(unreachable_patterns)]
    pub fn write_byte(&mut self, addr: u16, value: u8) -> u64 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => {
                self.cartridge.write_rom(addr, value);
                0
            }
            VRAM_START..=VRAM_END => {
                if !self.io.ppu.cpu_vram_accessible() {
                    return 0;
                }
                let local = (addr - VRAM_START) as usize;
                let index = self.active_vram_offset() + local;
                self.vram[index] = value;
                0
            }
            ERAM_START..=ERAM_END => {
                self.cartridge.write_ram(addr, value);
                0
            }
            WRAM_0_START..=WRAM_0_END => {
                self.wram[(addr - WRAM_0_START) as usize] = value;
                0
            }
            WRAM_N_START..=WRAM_N_END => {
                let local = (addr - WRAM_N_START) as usize;
                let index = self.active_wram_bank() * WRAM_SIZE + local;
                self.wram[index] = value;
                0
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let mirror = addr - ECHO_RAM_OFFSET;
                if mirror < WRAM_N_START {
                    self.wram[(mirror - WRAM_0_START) as usize] = value;
                } else {
                    let local = (mirror - WRAM_N_START) as usize;
                    self.wram[self.active_wram_bank() * WRAM_SIZE + local] = value;
                }
                0
            }
            OAM_START..=OAM_END => {
                if !self.io.ppu.cpu_oam_accessible() {
                    return 0;
                }
                self.oam[(addr - OAM_START) as usize] = value;
                0
            }
            NOT_USABLE_START..=NOT_USABLE_END => {
                trace!("Ignored write to forbidden zone at {:04X}", addr);
                0
            }
            IO_START..=IO_END => self.write_io(addr, value),
            HRAM_START..=HRAM_END => {
                self.hram[(addr - HRAM_START) as usize] = value;
                0
            }
            IE_ADDR => {
                self.ie = value;
                0
            }
            _ => {
                warn!("Attempted illegal write to UNKNOWN at address {:04X}", addr);
                0
            }
        }
    }

    fn read_io(&self, addr: u16) -> u8 {
        io_bus::read_io(self, addr)
    }

    fn write_io(&mut self, addr: u16, value: u8) -> u64 {
        io_bus::write_io(self, addr, value)
    }
}
