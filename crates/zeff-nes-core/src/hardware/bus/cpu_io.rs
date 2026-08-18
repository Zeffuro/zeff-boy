use super::Bus;
use crate::hardware::constants::*;
use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};

const CPU_RAM_MIRROR_START: u16 = 0x0000;
const CPU_RAM_MIRROR_END: u16 = 0x1FFF;
const PPU_REGISTER_MIRROR_START: u16 = 0x2000;
const PPU_REGISTER_MIRROR_END: u16 = 0x3FFF;
const APU_REGISTER_START: u16 = 0x4000;
const APU_PULSE_DMC_REGISTER_END: u16 = 0x4013;
const CPU_TEST_IO_START: u16 = 0x4018;
const CPU_TEST_IO_END: u16 = 0x401F;
const CARTRIDGE_EXPANSION_START: u16 = 0x4020;
const CARTRIDGE_RAM_END: u16 = 0x7FFF;
const CARTRIDGE_ROM_START: u16 = 0x8000;
const CPU_ADDR_END: u16 = 0xFFFF;
const OAM_DMA_PAGE_BYTES: u16 = 256;
const OAM_DMA_STALL_CYCLES_EVEN: u64 = 513;
const OAM_DMA_STALL_CYCLES_ODD: u64 = 514;
const CONTROLLER_OPEN_BUS_MASK: u8 = 0xE0;
const CONTROLLER_DATA_MASK: u8 = 0x1F;
const PPU_DATA_ADDR_MASK: u16 = 0x3FFF;

impl Bus {
    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        let at = self.next_cpu_access_tick();
        let ppu_addr = self.ppu_data_addr_for_cpu_register_access(addr);
        let val = match addr {
            CPU_RAM_MIRROR_START..=CPU_RAM_MIRROR_END => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize]
            }
            PPU_REGISTER_MIRROR_START..=PPU_REGISTER_MIRROR_END => {
                self.ppu_read_register(addr & PPU_REG_MIRROR_MASK)
            }
            APU_REGISTER_START..=APU_PULSE_DMC_REGISTER_END => self.cpu_open_bus,
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
            CPU_TEST_IO_START..=CPU_TEST_IO_END => self.cpu_open_bus,
            CARTRIDGE_EXPANSION_START..=CARTRIDGE_RAM_END => {
                self.cartridge.cpu_read_open_bus(addr, self.cpu_open_bus)
            }
            CARTRIDGE_ROM_START..=CPU_ADDR_END => {
                let rom_val = self.cartridge.cpu_read_open_bus(addr, self.cpu_open_bus);
                self.game_genie.intercept(addr, rom_val).unwrap_or(rom_val)
            }
        };
        self.cpu_open_bus = val;
        if self.debug_trace_enabled {
            self.debug_trace_events.push(super::DebugTraceEvent::Read {
                at,
                space: TraceWriteKind::Memory,
                addr: u32::from(addr),
                value: u32::from(val),
                width: TraceWriteWidth::Byte,
                mapped_addr: ppu_addr.map(u32::from),
            });
        }
        val
    }

    #[inline]
    pub fn cpu_read_after_elapsed_cycles(&mut self, addr: u16, elapsed_cycles: u64) -> u8 {
        if Self::cpu_read_needs_elapsed_timing(addr) {
            self.advance_cpu_step_timing_to(elapsed_cycles);
        }
        self.advance_cpu_access_timing_to(elapsed_cycles);
        self.cpu_read(addr)
    }

    fn cpu_read_at_elapsed_cycles_detached(&mut self, addr: u16, elapsed_cycles: u64) -> u8 {
        let instruction_cursor = self.cpu_access_elapsed_cycles;
        self.cpu_access_elapsed_cycles = elapsed_cycles;
        let value = self.cpu_read(addr);
        self.cpu_access_elapsed_cycles = instruction_cursor;
        value
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            CPU_RAM_MIRROR_START..=CPU_RAM_MIRROR_END => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize]
            }
            PPU_REGISTER_MIRROR_START..=PPU_REGISTER_MIRROR_END => self
                .ppu
                .peek_register_at(addr & PPU_REG_MIRROR_MASK, self.ppu_cycles),
            APU_REGISTER_START..=APU_PULSE_DMC_REGISTER_END => 0,
            OAM_DMA => 0,
            APU_STATUS => self.apu.peek_status(),
            CONTROLLER1 => 0,
            CONTROLLER2 => 0,
            CPU_TEST_IO_START..=CPU_TEST_IO_END => 0,
            CARTRIDGE_EXPANSION_START..=CARTRIDGE_RAM_END => self.cartridge.cpu_peek(addr),
            CARTRIDGE_ROM_START..=CPU_ADDR_END => {
                let rom_val = self.cartridge.cpu_peek(addr);
                self.game_genie.intercept(addr, rom_val).unwrap_or(rom_val)
            }
        }
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        let access_is_odd = self.cpu_cycle_is_odd(self.cpu_access_elapsed_cycles);
        let at = self.next_cpu_access_tick();
        if self.debug_trace_enabled {
            let old = self.cpu_peek(addr);
            let ppu_addr = self.ppu_data_addr_for_cpu_register_access(addr);
            self.debug_trace_events.push(super::DebugTraceEvent::Write {
                at,
                space: TraceWriteKind::Memory,
                addr: u32::from(addr),
                old_value: u32::from(old),
                written_value: u32::from(val),
                new_value: u32::from(val),
                width: TraceWriteWidth::Byte,
                mapped_addr: ppu_addr.map(u32::from),
            });
        }
        match addr {
            CPU_RAM_MIRROR_START..=CPU_RAM_MIRROR_END => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize] = val;
            }

            PPU_REGISTER_MIRROR_START..=PPU_REGISTER_MIRROR_END => {
                self.ppu_write_register(addr & PPU_REG_MIRROR_MASK, val);
            }

            APU_REGISTER_START..=APU_PULSE_DMC_REGISTER_END | APU_STATUS | CONTROLLER2 => {
                self.apu.write_register(addr, val, access_is_odd);
            }

            OAM_DMA => {
                let base = (val as u16) << 8;
                let stall_cycles = if self.cpu_cycle_is_odd(self.cpu_access_elapsed_cycles) {
                    OAM_DMA_STALL_CYCLES_ODD
                } else {
                    OAM_DMA_STALL_CYCLES_EVEN
                };
                let first_read_cycle = self.cpu_access_elapsed_cycles + (stall_cycles - 512);
                for i in 0..OAM_DMA_PAGE_BYTES {
                    let byte = self.cpu_read_at_elapsed_cycles_detached(
                        base + i,
                        first_read_cycle + 2 * u64::from(i),
                    );
                    self.write_oam_data(byte);
                }

                self.dma_stall_cycles = stall_cycles;
            }

            CONTROLLER1 => {
                self.controller1.write(val);
                self.controller2.write(val);
                self.expansion_device.write(val);
                self.cartridge.cpu_write(addr, val);
            }

            CPU_TEST_IO_START..=CPU_TEST_IO_END => {}

            CARTRIDGE_EXPANSION_START..=CPU_ADDR_END => {
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
        self.advance_cpu_access_timing_to(elapsed_cycles);
        self.cpu_write(addr, val);
    }

    #[inline]
    fn cpu_read_needs_elapsed_timing(addr: u16) -> bool {
        matches!(
            addr,
            PPU_REGISTER_MIRROR_START
                ..=PPU_REGISTER_MIRROR_END | APU_STATUS | CONTROLLER1 | CONTROLLER2
        )
    }

    #[inline]
    fn cpu_write_needs_elapsed_timing(addr: u16) -> bool {
        matches!(addr, PPU_REGISTER_MIRROR_START..=CONTROLLER2)
    }

    #[inline]
    fn controller_port_read_value(&self, raw: u8) -> u8 {
        if self.is_vs_system_mapper() {
            raw
        } else {
            (self.cpu_open_bus & CONTROLLER_OPEN_BUS_MASK) | (raw & CONTROLLER_DATA_MASK)
        }
    }

    #[inline]
    fn ppu_data_addr_for_cpu_register_access(&self, addr: u16) -> Option<u16> {
        if (PPU_REGISTER_MIRROR_START..=PPU_REGISTER_MIRROR_END).contains(&addr)
            && (addr & PPU_REG_MIRROR_MASK) == PPU_REG_DATA
        {
            Some(self.ppu.v & PPU_DATA_ADDR_MASK)
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

    #[test]
    fn trace_preserves_mapped_ppu_address_without_claiming_a_timestamp() {
        let mut bus = test_bus();
        bus.ppu.v = 0x2345;
        bus.debug_trace_enabled = true;

        let value = bus.cpu_read(PPU_REG_DATA);

        assert_eq!(
            bus.debug_trace_events,
            vec![crate::hardware::bus::DebugTraceEvent::Read {
                at: None,
                space: TraceWriteKind::Memory,
                addr: u32::from(PPU_REG_DATA),
                value: u32::from(value),
                width: TraceWriteWidth::Byte,
                mapped_addr: Some(0x2345),
            }]
        );
    }
}
