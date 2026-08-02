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
                let raw = self.controller1.read()
                    | self.expansion_device.read_4016()
                    | self.vs_system_4016_bits();
                self.controller_port_read_value(raw)
            }
            CONTROLLER2 => {
                let zapper_hit = self.current_zapper_light_detected();
                self.controller2.set_zapper_hit(zapper_hit);
                let raw = self.controller2.read()
                    | self.expansion_device.read_4017()
                    | self.vs_system_4017_bits();
                self.controller_port_read_value(raw)
            }
            0x4018..=0x401F => self.cpu_open_bus,
            0x4020..=0x7FFF => self.cartridge.cpu_read_open_bus(addr, self.cpu_open_bus),
            0x8000..=0xFFFF => {
                let rom_val = self.cartridge.cpu_read_open_bus(addr, self.cpu_open_bus);
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
                    self.write_oam_data(byte);
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
        self.cpu_open_bus = val;
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
        matches!(addr, 0x2000..=0x3FFF | APU_STATUS | CONTROLLER1 | CONTROLLER2)
    }

    #[inline]
    fn cpu_write_needs_elapsed_timing(addr: u16) -> bool {
        matches!(addr, 0x2000..=0x3FFF | 0x4000..=0x4017)
    }

    #[inline]
    fn controller_port_read_value(&self, raw: u8) -> u8 {
        if self.is_vs_system_mapper() {
            raw
        } else {
            (self.cpu_open_bus & 0xE0) | (raw & 0x1F)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::Cartridge;
    use crate::hardware::controller::Button;

    fn test_bus() -> Bus {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;

        let cart = Cartridge::load(&rom).expect("test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    fn vs_system_bus() -> Bus {
        let mut rom = vec![0u8; 16 + 0x8000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2;
        rom[5] = 1;
        rom[6] = 0x30;
        rom[7] = 0x61;

        let cart = Cartridge::load(&rom).expect("Vs. System test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    fn axrom_bus() -> Bus {
        let mut rom = vec![0u8; 16 + 0x8000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2;
        rom[5] = 0;
        rom[6] = 0x70;
        rom[16..16 + 0x8000].fill(0xA7);

        let cart = Cartridge::load(&rom).expect("AxROM test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    #[test]
    fn standard_controller_reads_preserve_high_open_bus_bits() {
        let mut bus = test_bus();
        bus.controller1.press(Button::A);
        bus.controller1.write(1);
        bus.controller1.write(0);
        bus.cpu_open_bus = 0x40;

        assert_eq!(bus.cpu_read(CONTROLLER1), 0x41);
        assert_eq!(bus.cpu_read(CONTROLLER1), 0x40);
    }

    #[test]
    fn second_controller_reads_preserve_high_open_bus_bits() {
        let mut bus = test_bus();
        bus.controller2.press(Button::B);
        bus.controller2.write(1);
        bus.controller2.write(0);
        bus.cpu_open_bus = 0xA0;

        assert_eq!(bus.cpu_read(CONTROLLER2), 0xA0);
        assert_eq!(bus.cpu_read(CONTROLLER2), 0xA1);
    }

    #[test]
    fn vs_system_controller_reads_do_not_inherit_open_bus_coin_bits() {
        let mut bus = vs_system_bus();
        bus.cpu_open_bus = 0xE0;

        assert_eq!(bus.cpu_read(CONTROLLER1) & 0x20, 0);
    }

    #[test]
    fn axrom_unmapped_lower_cart_reads_use_cpu_open_bus() {
        let mut bus = axrom_bus();
        bus.cpu_open_bus = 0x6A;

        assert_eq!(bus.cpu_read(0x6000), 0x6A);
        bus.cpu_write(0x6000, 0x5C);
        assert_eq!(bus.cpu_read(0x6000), 0x5C);
        assert_eq!(bus.cpu_read(0x8000), 0xA7);
    }
}
