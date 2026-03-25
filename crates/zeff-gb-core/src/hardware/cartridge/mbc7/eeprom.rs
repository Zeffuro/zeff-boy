use super::Mbc7;
use super::{
    ADDR_BITS, CMD_BITS, CMD_ERASE, CMD_EXTENDED, CMD_READ, CMD_WRITE, DATA_BITS, EXT_ERAL,
    EXT_EWDS, EXT_EWEN, EXT_WRAL, SPI_CLK, SPI_CS, SPI_DI, STATE_ADDRESS, STATE_COMMAND,
    STATE_DATA, STATE_IDLE, STATE_SHIFT_OUT, STATE_WRITE_PENDING,
};

impl Mbc7 {
    fn eeprom_read_word(&self, word_addr: u8) -> u16 {
        let a = word_addr as usize;
        if a * 2 + 1 < self.eeprom.len() {
            ((self.eeprom[a * 2] as u16) << 8) | (self.eeprom[a * 2 + 1] as u16)
        } else {
            0xFFFF
        }
    }

    fn eeprom_write_word(&mut self, word_addr: u8, data: u16) {
        let a = word_addr as usize;
        if a * 2 + 1 < self.eeprom.len() {
            self.eeprom[a * 2] = (data >> 8) as u8;
            self.eeprom[a * 2 + 1] = (data & 0xFF) as u8;
        }
    }

    fn eeprom_fill_all(&mut self, data: u16) {
        for i in 0..super::EEPROM_WORD_COUNT {
            self.eeprom[i * 2] = (data >> 8) as u8;
            self.eeprom[i * 2 + 1] = (data & 0xFF) as u8;
        }
    }

    pub(super) fn write_eeprom(&mut self, value: u8) {
        let old_cs = self.cs;
        let old_clk = self.clk;

        self.cs = value & SPI_CS != 0;
        self.clk = value & SPI_CLK != 0;
        let di = value & SPI_DI != 0;

        if !old_cs && self.cs {
            if self.state == STATE_WRITE_PENDING {
                if self.write_enable {
                    self.eeprom_write_word(self.address, self.buffer);
                }
                self.state = STATE_IDLE;
                self.do_value = 1;
                self.idle = true;
            } else {
                self.idle = true;
                self.state = STATE_IDLE;
            }
        }

        if !old_clk && self.clk {
            self.clock_rising_edge(di);
        }

        if old_clk && !self.clk && self.state == STATE_SHIFT_OUT {
            self.do_value = if self.buffer & 0x8000 != 0 { 1 } else { 0 };
            self.buffer <<= 1;
            self.count += 1;
            if self.count == DATA_BITS {
                self.count = 0;
                self.state = STATE_IDLE;
            }
        }
    }

    fn clock_rising_edge(&mut self, di: bool) {
        if self.idle {
            if di {
                self.idle = false;
                self.count = 0;
                self.state = STATE_COMMAND;
            }
            return;
        }

        match self.state {
            STATE_COMMAND => self.receive_command_bits(di),
            STATE_ADDRESS => self.receive_address_bits(di),
            STATE_DATA => self.receive_data_bits(di),
            _ => {}
        }
    }

    fn receive_command_bits(&mut self, di: bool) {
        self.buffer = (self.buffer << 1) | u16::from(di);
        self.count += 1;
        if self.count == CMD_BITS {
            self.state = STATE_ADDRESS;
            self.count = 0;
            self.command = (self.buffer & 3) as u8;
        }
    }

    fn receive_address_bits(&mut self, di: bool) {
        self.buffer = (self.buffer << 1) | u16::from(di);
        self.count += 1;
        if self.count == ADDR_BITS {
            self.state = STATE_DATA;
            self.count = 0;
            self.address = (self.buffer & 0xFF) as u8;

            if self.command == CMD_EXTENDED {
                match self.address >> 6 {
                    EXT_EWDS => {
                        self.write_enable = false;
                        self.state = STATE_IDLE;
                    }
                    EXT_EWEN => {
                        self.write_enable = true;
                        self.state = STATE_IDLE;
                    }
                    _ => {}
                }
            }
        }
    }

    fn receive_data_bits(&mut self, di: bool) {
        self.buffer = (self.buffer << 1) | u16::from(di);
        self.count += 1;

        match self.command {
            CMD_EXTENDED => {
                if self.count == DATA_BITS {
                    match self.address >> 6 {
                        EXT_EWDS => {
                            self.write_enable = false;
                            self.state = STATE_IDLE;
                        }
                        EXT_WRAL => {
                            if self.write_enable {
                                self.eeprom_fill_all(self.buffer);
                            }
                            self.state = STATE_WRITE_PENDING;
                        }
                        EXT_ERAL => {
                            if self.write_enable {
                                self.eeprom.fill(0xFF);
                            }
                            self.state = STATE_WRITE_PENDING;
                        }
                        EXT_EWEN => {
                            self.write_enable = true;
                            self.state = STATE_IDLE;
                        }
                        _ => {}
                    }
                    self.count = 0;
                }
            }
            CMD_WRITE => {
                if self.count == DATA_BITS {
                    self.count = 0;
                    self.state = STATE_WRITE_PENDING;
                    self.do_value = 0;
                }
            }
            CMD_READ => {
                if self.count == 1 {
                    self.state = STATE_SHIFT_OUT;
                    self.count = 0;
                    self.buffer = self.eeprom_read_word(self.address);
                }
            }
            CMD_ERASE => {
                if self.count == DATA_BITS {
                    self.count = 0;
                    self.state = STATE_WRITE_PENDING;
                    self.do_value = 0;
                    self.buffer = 0xFFFF;
                }
            }
            _ => {}
        }
    }
}
