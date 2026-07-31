use super::Bus;
use crate::hardware::constants::*;

impl Bus {
    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        let ppu_addr = self.ppu_data_addr_for_cpu_register_access(addr);
        let val = match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self.ppu_read_register(addr & PPU_REG_MIRROR_MASK),
            0x4000..=0x4013 => self.cpu_open_bus,
            OAM_DMA => self.cpu_open_bus,
            APU_STATUS => self.apu.read_status_with_frame_irq_lookahead(1),
            CONTROLLER1 => {
                let zapper_hit = self.current_zapper_light_detected();
                self.controller1.set_zapper_hit(zapper_hit);
                self.controller1.read()
                    | self.expansion_device.read_4016()
                    | self.vs_system_4016_bits()
            }
            CONTROLLER2 => {
                let zapper_hit = self.current_zapper_light_detected();
                self.controller2.set_zapper_hit(zapper_hit);
                self.controller2.read()
                    | self.expansion_device.read_4017()
                    | self.vs_system_4017_bits()
            }
            0x4018..=0x401F => self.cpu_open_bus,
            0x4020..=0x7FFF => self.cartridge.cpu_read(addr),
            0x8000..=0xFFFF => {
                let rom_val = self.cartridge.cpu_read(addr);
                self.game_genie.intercept(addr, rom_val).unwrap_or(rom_val)
            }
        };
        self.cpu_open_bus = val;
        if self.debug_trace_enabled {
            self.debug_trace_events.push(super::DebugTraceEvent::Read {
                addr,
                value: val,
                ppu_addr,
            });
        }
        val
    }

    #[inline]
    pub fn cpu_read_after_elapsed_cycles(&mut self, addr: u16, elapsed_cycles: u64) -> u8 {
        if Self::cpu_read_needs_elapsed_timing(addr) {
            self.advance_cpu_step_timing_to(elapsed_cycles);
        }
        self.cpu_read(addr)
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self
                .ppu
                .peek_register_at(addr & PPU_REG_MIRROR_MASK, self.ppu_cycles),
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
            let ppu_addr = self.ppu_data_addr_for_cpu_register_access(addr);
            self.debug_trace_events.push(super::DebugTraceEvent::Write {
                addr,
                old_value: old,
                new_value: val,
                ppu_addr,
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
                self.expansion_device.write(val);
                self.cartridge.cpu_write(addr, val);
            }

            0x4018..=0x401F => {}

            0x4020..=0xFFFF => {
                self.cartridge.cpu_write(addr, val);
            }
        }
    }

    #[inline]
    pub fn cpu_write_after_elapsed_cycles(&mut self, addr: u16, val: u8, elapsed_cycles: u64) {
        if Self::cpu_write_needs_elapsed_timing(addr) {
            self.advance_cpu_step_timing_to(elapsed_cycles);
        }
        self.cpu_write(addr, val);
    }

    #[inline]
    fn cpu_read_needs_elapsed_timing(addr: u16) -> bool {
        matches!(addr, 0x2000..=0x3FFF | CONTROLLER1 | CONTROLLER2)
    }

    #[inline]
    fn cpu_write_needs_elapsed_timing(addr: u16) -> bool {
        matches!(addr, 0x2000..=0x3FFF | OAM_DMA | CONTROLLER1)
    }

    #[inline]
    fn ppu_data_addr_for_cpu_register_access(&self, addr: u16) -> Option<u16> {
        if (0x2000..=0x3FFF).contains(&addr) && (addr & PPU_REG_MIRROR_MASK) == 0x2007 {
            Some(self.ppu.v & 0x3FFF)
        } else {
            None
        }
    }
}
