use super::Bus;
use crate::hardware::constants::*;

impl Bus {
    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        let val = match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self.ppu_read_register(addr & PPU_REG_MIRROR_MASK),
            0x4000..=0x4013 => self.cpu_open_bus,
            OAM_DMA => self.cpu_open_bus,
            APU_STATUS => self.apu.read_status(),
            CONTROLLER1 => self.controller1.read(),
            CONTROLLER2 => self.controller2.read(),
            0x4018..=0x401F => self.cpu_open_bus,
            0x4020..=0x7FFF => self.cartridge.cpu_read(addr),
            0x8000..=0xFFFF => {
                let rom_val = self.cartridge.cpu_read(addr);
                self.game_genie.intercept(addr, rom_val).unwrap_or(rom_val)
            }
        };
        self.cpu_open_bus = val;
        if self.debug_trace_enabled {
            self.debug_trace_events
                .push(super::DebugTraceEvent::Read { addr, value: val });
        }
        val
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self.ppu.peek_register(addr & PPU_REG_MIRROR_MASK),
            0x4000..=0x4013 => 0,
            OAM_DMA => 0,
            APU_STATUS => self.apu.peek_status(),
            CONTROLLER1 => 0,
            CONTROLLER2 => 0,
            0x4018..=0x401F => 0,
            0x4020..=0xFFFF => self.cartridge.cpu_peek(addr),
        }
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        if self.debug_trace_enabled {
            let old = self.cpu_peek(addr);
            self.debug_trace_events.push(super::DebugTraceEvent::Write {
                addr,
                old_value: old,
                new_value: val,
            });
        }
        match addr {
            0x0000..=0x1FFF => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize] = val;
            }

            0x2000..=0x3FFF => {
                self.ppu_write_register(addr & PPU_REG_MIRROR_MASK, val);
            }

            0x4000..=0x4013 | APU_STATUS | CONTROLLER2 => {
                self.apu.write_register(addr, val, self.cpu_odd_cycle);
            }

            OAM_DMA => {
                let base = (val as u16) << 8;
                for i in 0..256u16 {
                    let byte = self.cpu_read(base + i);
                    self.ppu.oam[self.ppu.oam_addr as usize] = byte;
                    self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
                }

                self.dma_stall_cycles = if self.cpu_odd_cycle { 514 } else { 513 };
            }

            CONTROLLER1 => {
                self.controller1.write(val);
                self.controller2.write(val);
            }

            0x4018..=0x401F => {}

            0x4020..=0xFFFF => {
                self.cartridge.cpu_write(addr, val);
            }
        }
    }
}
