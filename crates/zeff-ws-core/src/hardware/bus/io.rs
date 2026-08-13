use super::*;

impl Bus {
    pub fn io_read8(&mut self, port: u16) -> u8 {
        let value = match port {
            CURRENT_LINE_PORT => self.ppu.vcount() as u8,
            port if !self.is_color_model() && color_only_port(port) => self.io_open_bus(),
            port if Apu::handles_port(port) => self.apu.read8(port),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)],
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.hblank_timer_count();
                count.to_le_bytes()[usize::from(port - HBLANK_TIMER_COUNT_LO_PORT)]
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.vblank_timer_count();
                count.to_le_bytes()[usize::from(port - VBLANK_TIMER_COUNT_LO_PORT)]
            }
            DMA_SOURCE_SEGMENT_HIGH_PORT
            | SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT
            | SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT => 0,
            SYSTEM_CONTROL_PORT => self.system_control_read(),
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT => self.io[usize::from(port)],
            INTERNAL_EEPROM_COMMAND_PORT => self.internal_eeprom_status_read(),
            INTERNAL_EEPROM_COMMAND_HIGH_PORT => 0,
            IRQ_VECTOR_BASE_PORT => self.interrupt_base_read(),
            IRQ_ENABLE_PORT => self.io[usize::from(IRQ_ENABLE_PORT)],
            SERIAL_DATA_PORT => {
                let value = self.uart.read_data();
                self.io[usize::from(SERIAL_DATA_PORT)] = value;
                value
            }
            SERIAL_CONTROL_PORT => self.serial_control_read(),
            KEYPAD_PORT => self.keypad.read(),
            IRQ_STATUS_PORT => self.io[usize::from(IRQ_STATUS_PORT)],
            IRQ_ACK_PORT => self.io_open_bus(),
            ROM_LINEAR_BANK_PORT => self.linear_bank_read() | 0x20,
            ROM_RAM_BANK_PORT => self.cartridge.ram_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)],
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.cartridge_eeprom_status_read(),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => 0,
            RTC_COMMAND_STATUS_PORT => self.rtc.read_status(),
            RTC_PAYLOAD_PORT => self.rtc.read_payload(),
            _ => self.io[usize::from(port)],
        };
        self.record(DebugTraceEvent::IoRead { port, value });
        value
    }

    pub fn io_read16(&mut self, port: u16) -> u16 {
        u16::from_le_bytes([self.io_read8(port), self.io_read8(port.wrapping_add(1))])
    }

    pub fn io_peek8(&self, port: u16) -> u8 {
        match port {
            CURRENT_LINE_PORT => self.ppu.vcount() as u8,
            port if !self.is_color_model() && color_only_port(port) => self.io_open_bus(),
            port if Apu::handles_port(port) => self.apu.read8(port),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)],
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.hblank_timer_count();
                count.to_le_bytes()[usize::from(port - HBLANK_TIMER_COUNT_LO_PORT)]
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.vblank_timer_count();
                count.to_le_bytes()[usize::from(port - VBLANK_TIMER_COUNT_LO_PORT)]
            }
            DMA_SOURCE_SEGMENT_HIGH_PORT
            | SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT
            | SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT => 0,
            SYSTEM_CONTROL_PORT => self.system_control_read(),
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT => self.io[usize::from(port)],
            INTERNAL_EEPROM_COMMAND_PORT => self.internal_eeprom_status_peek(),
            INTERNAL_EEPROM_COMMAND_HIGH_PORT => 0,
            IRQ_VECTOR_BASE_PORT => self.interrupt_base_read(),
            IRQ_ENABLE_PORT => self.io[usize::from(IRQ_ENABLE_PORT)],
            SERIAL_DATA_PORT => self.uart.peek_data(),
            SERIAL_CONTROL_PORT => self.serial_control_read(),
            KEYPAD_PORT => self.keypad.read(),
            IRQ_STATUS_PORT => self.io[usize::from(IRQ_STATUS_PORT)],
            IRQ_ACK_PORT => self.io_open_bus(),
            ROM_LINEAR_BANK_PORT => self.linear_bank_read() | 0x20,
            ROM_RAM_BANK_PORT => self.cartridge.ram_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)],
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.cartridge_eeprom_status_read(),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => 0,
            RTC_COMMAND_STATUS_PORT => self.rtc.peek_status(),
            RTC_PAYLOAD_PORT => self.rtc.peek_payload(),
            _ => self.io[usize::from(port)],
        }
    }

    pub fn io_write8(&mut self, port: u16, value: u8) {
        let old_value = self.io_peek8(port);
        match port {
            CURRENT_LINE_PORT => {}
            port if !self.is_color_model() && color_only_port(port) => {}
            port if Apu::handles_port(port) => self.apu.write8(port, value),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)] = value,
            TIMER_CONTROL_PORT => self.io[usize::from(TIMER_CONTROL_PORT)] = value & 0x0F,
            HBLANK_TIMER_RELOAD_LO_PORT | HBLANK_TIMER_RELOAD_HI_PORT => {
                self.io[usize::from(port)] = value;
                self.set_hblank_timer_count(self.hblank_timer_reload());
            }
            VBLANK_TIMER_RELOAD_LO_PORT | VBLANK_TIMER_RELOAD_HI_PORT => {
                self.io[usize::from(port)] = value;
                self.set_vblank_timer_count(self.vblank_timer_reload());
            }
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            MONO_PALETTE_PORT_START..=MONO_PALETTE_PORT_END if !self.is_color_model() => {
                self.write_mono_palette_port(port, value);
            }
            DMA_SOURCE_LO_PORT | DMA_DESTINATION_LO_PORT | DMA_LENGTH_LO_PORT => {
                self.io[usize::from(port)] = value & !1;
            }
            DMA_SOURCE_HI_PORT | DMA_DESTINATION_HI_PORT | DMA_LENGTH_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            DMA_SOURCE_SEGMENT_PORT => {
                self.io[usize::from(DMA_SOURCE_SEGMENT_PORT)] = value & 0x0F;
                self.io[usize::from(DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
            }
            DMA_SOURCE_SEGMENT_HIGH_PORT => {
                self.io[usize::from(DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
            }
            DMA_CONTROL_PORT => {
                self.io[usize::from(DMA_CONTROL_PORT)] = value;
                if value & 0x80 != 0 {
                    self.run_dma_transfer(value);
                }
            }
            SOUND_DMA_SOURCE_LO_PORT
            | SOUND_DMA_SOURCE_HI_PORT
            | SOUND_DMA_LENGTH_LO_PORT
            | SOUND_DMA_LENGTH_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            SOUND_DMA_SOURCE_SEGMENT_PORT => {
                self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_PORT)] = value & 0x0F;
                self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
            }
            SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT => {
                self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
            }
            SOUND_DMA_LENGTH_SEGMENT_PORT => {
                self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_PORT)] = value & 0x0F;
                self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT)] = 0;
            }
            SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT => {
                self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT)] = 0;
            }
            SOUND_DMA_CONTROL_PORT => self.write_sound_dma_control(value),
            SYSTEM_CONTROL_PORT => {
                let color_bit = u8::from(
                    self.cartridge.minimum_system()
                        != super::super::cartridge::MinimumSystem::WonderSwan,
                ) << 1;
                self.io[usize::from(SYSTEM_CONTROL_PORT)] = (value & 0xFD) | color_bit;
            }
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            INTERNAL_EEPROM_COMMAND_PORT => self.write_internal_eeprom_command(value),
            INTERNAL_EEPROM_COMMAND_HIGH_PORT => {}
            IRQ_VECTOR_BASE_PORT => self.io[usize::from(IRQ_VECTOR_BASE_PORT)] = value & 0xF8,
            IRQ_ENABLE_PORT => {
                self.io[usize::from(IRQ_ENABLE_PORT)] = value;
                self.refresh_level_interrupts();
            }
            SERIAL_DATA_PORT => {
                self.io[usize::from(SERIAL_DATA_PORT)] = value;
                self.uart
                    .write_data(value, self.io[usize::from(SERIAL_CONTROL_PORT)]);
                self.refresh_level_interrupts();
            }
            SERIAL_CONTROL_PORT => {
                self.io[usize::from(SERIAL_CONTROL_PORT)] = self.uart.write_control(value);
                self.refresh_level_interrupts();
            }
            KEYPAD_PORT => self.keypad.write(value),
            IRQ_ACK_PORT => {
                self.io[usize::from(IRQ_STATUS_PORT)] &= !value;
                self.refresh_level_interrupts();
            }
            ROM_LINEAR_BANK_PORT => self.defer_linear_bank(value),
            ROM_RAM_BANK_PORT => self.cartridge.set_ram_bank(value),
            ROM_BANK0_PORT => self.cartridge.set_bank0(value),
            ROM_BANK1_PORT => self.cartridge.set_bank1(value),
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)] = value,
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.write_cartridge_eeprom_control(value),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => {}
            RTC_COMMAND_STATUS_PORT => {
                self.rtc
                    .write_command(value, self.io[usize::from(RTC_PAYLOAD_PORT)]);
                self.io[usize::from(RTC_COMMAND_STATUS_PORT)] = value;
            }
            RTC_PAYLOAD_PORT => {
                self.rtc.write_payload(value);
                self.io[usize::from(RTC_PAYLOAD_PORT)] = value;
            }
            _ => self.io[usize::from(port)] = value,
        }
        let new_value = self.io_peek8(port);
        self.record(DebugTraceEvent::IoWrite {
            port,
            old_value,
            new_value,
        });
    }

    pub fn io_write16(&mut self, port: u16, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io_write8(port, lo);
        self.io_write8(port.wrapping_add(1), hi);
    }
}

fn color_only_port(port: u16) -> bool {
    Apu::handles_color_only_port(port)
        || matches!(
            port,
            DMA_SOURCE_LO_PORT
                | DMA_SOURCE_HI_PORT
                | DMA_SOURCE_SEGMENT_PORT
                | DMA_SOURCE_SEGMENT_HIGH_PORT
                | DMA_DESTINATION_LO_PORT
                | DMA_DESTINATION_HI_PORT
                | DMA_LENGTH_LO_PORT
                | DMA_LENGTH_HI_PORT
                | DMA_CONTROL_PORT
                | SOUND_DMA_SOURCE_LO_PORT
                | SOUND_DMA_SOURCE_HI_PORT
                | SOUND_DMA_SOURCE_SEGMENT_PORT
                | SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT
                | SOUND_DMA_LENGTH_LO_PORT
                | SOUND_DMA_LENGTH_HI_PORT
                | SOUND_DMA_LENGTH_SEGMENT_PORT
                | SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT
                | SOUND_DMA_CONTROL_PORT
        )
}
