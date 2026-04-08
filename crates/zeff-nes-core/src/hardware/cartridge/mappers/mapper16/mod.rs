use crate::hardware::cartridge::{Mapper, Mirroring};

mod eeprom;
use eeprom::{Eeprom, EepromReadPhase, EepromState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mapper16Mode {
    Unspecified,
    Fcg,
    Lz93d50,
}

impl Mapper16Mode {
    fn from_submapper(submapper: u8) -> Self {
        match submapper {
            4 => Self::Fcg,
            5 => Self::Lz93d50,
            _ => Self::Unspecified,
        }
    }

    fn accepts_6000_registers(self) -> bool {
        matches!(self, Self::Fcg | Self::Unspecified)
    }

    fn accepts_8000_registers(self) -> bool {
        matches!(self, Self::Lz93d50 | Self::Unspecified)
    }

    fn irq_is_latched(self) -> bool {
        matches!(self, Self::Lz93d50 | Self::Unspecified)
    }

    fn eeprom_supported(self) -> bool {
        matches!(self, Self::Lz93d50 | Self::Unspecified)
    }
}

pub struct BandaiFcg16 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    fixed_four_screen: bool,

    prg_bank: u8,
    chr_banks: [u8; 8],

    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,

    mode: Mapper16Mode,
    has_eeprom: bool,
    eeprom: Eeprom,
}

impl BandaiFcg16 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        submapper_id: u8,
        has_eeprom: bool,
    ) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            prg_bank: 0,
            chr_banks: [0; 8],
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            mode: Mapper16Mode::from_submapper(submapper_id),
            has_eeprom,
            eeprom: Eeprom::new(),
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn handle_eeprom_control_write(&mut self, val: u8) {
        if !self.mode.eeprom_supported() || !self.has_eeprom {
            return;
        }
        self.eeprom.handle_control_write(val);
    }

    fn eeprom_data_out(&self) -> bool {
        if !self.mode.eeprom_supported() || !self.has_eeprom || !self.eeprom.read_enable {
            return true;
        }
        self.eeprom.data_out()
    }

    fn handles_register_write(&self, addr: u16) -> bool {
        let in_6000 = (0x6000..=0x7FFF).contains(&addr) && self.mode.accepts_6000_registers();
        let in_8000 = (0x8000..=0xFFFF).contains(&addr) && self.mode.accepts_8000_registers();
        in_6000 || in_8000
    }

    fn write_register(&mut self, reg: u16, val: u8) {
        match reg & 0x000F {
            0x0..=0x7 => self.chr_banks[(reg & 0x0007) as usize] = val,
            0x8 => self.prg_bank = val,
            0x9 => {
                if !self.fixed_four_screen {
                    self.mirroring = match val & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenLower,
                        3 => Mirroring::SingleScreenUpper,
                        _ => Mirroring::Horizontal,
                    };
                }
            }
            0xA => {
                self.irq_enabled = val & 0x01 != 0;
                self.irq_pending = false;
                if self.mode.irq_is_latched() {
                    self.irq_counter = self.irq_latch;
                }
                if self.irq_enabled && self.irq_counter == 0 {
                    self.irq_pending = true;
                }
            }
            0xB => {
                if self.mode.irq_is_latched() {
                    self.irq_latch = (self.irq_latch & 0xFF00) | (val as u16)
                } else {
                    self.irq_counter = (self.irq_counter & 0xFF00) | (val as u16)
                }
            }
            0xC => {
                if self.mode.irq_is_latched() {
                    self.irq_latch = (self.irq_latch & 0x00FF) | ((val as u16) << 8)
                } else {
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8)
                }
            }
            0xD => self.handle_eeprom_control_write(val),
            _ => {}
        }
    }
}

impl Mapper for BandaiFcg16 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.mode.eeprom_supported() && self.has_eeprom {
                    if self.eeprom_data_out() { 0x10 } else { 0x00 }
                } else {
                    self.prg_ram[(addr - 0x6000) as usize]
                }
            }
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % self.prg_bank_count_16k();
                let offset = (addr as usize) & 0x3FFF;
                self.prg_rom[bank * 0x4000 + offset]
            }
            0xC000..=0xFFFF => {
                let last_bank = self.prg_bank_count_16k() - 1;
                let offset = (addr as usize) & 0x3FFF;
                self.prg_rom[last_bank * 0x4000 + offset]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize] = val;
        }
        if self.handles_register_write(addr) {
            self.write_register(addr, val);
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = (self.chr_banks[slot] as usize) % self.chr_bank_count_1k();
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = (self.chr_banks[slot] as usize) % self.chr_bank_count_1k();
        let offset = (addr as usize) & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        if !self.irq_enabled {
            return;
        }

        if self.irq_counter > 0 {
            self.irq_counter = self.irq_counter.saturating_sub(1);
        }

        if self.irq_counter == 0 {
            self.irq_pending = true;
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);

        w.write_u8(self.prg_bank);
        w.write_bytes(&self.chr_banks);

        w.write_u16(self.irq_latch);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        w.write_u8(match self.mode {
            Mapper16Mode::Unspecified => 0,
            Mapper16Mode::Fcg => 4,
            Mapper16Mode::Lz93d50 => 5,
        });
        w.write_bool(self.has_eeprom);
        w.write_bytes(&self.eeprom.data);
        w.write_bool(self.eeprom.scl);
        w.write_bool(self.eeprom.sda_in);
        w.write_bool(self.eeprom.read_enable);
        w.write_u8(match self.eeprom.state {
            EepromState::Standby => 0,
            EepromState::ReceiveControl => 1,
            EepromState::ReceiveAddress => 2,
            EepromState::ReceiveWriteData => 3,
            EepromState::SendReadData => 4,
        });
        w.write_u8(self.eeprom.shift);
        w.write_u8(self.eeprom.bits);
        w.write_u8(self.eeprom.pointer);
        w.write_bool(self.eeprom.address_latched);
        w.write_u8(match self.eeprom.read_phase {
            EepromReadPhase::OutputBits => 0,
            EepromReadPhase::AwaitMasterAck => 1,
        });
        w.write_u8(self.eeprom.read_bit);

        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;

        self.prg_bank = r.read_u8()?;
        r.read_exact(&mut self.chr_banks)?;

        self.irq_latch = r.read_u16()?;
        self.irq_counter = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        self.mode = match r.read_u8()? {
            0 => Mapper16Mode::Unspecified,
            4 => Mapper16Mode::Fcg,
            5 => Mapper16Mode::Lz93d50,
            other => anyhow::bail!("invalid mapper16 mode tag: {other}"),
        };
        self.has_eeprom = r.read_bool()?;
        r.read_exact(&mut self.eeprom.data)?;
        self.eeprom.scl = r.read_bool()?;
        self.eeprom.sda_in = r.read_bool()?;
        self.eeprom.read_enable = r.read_bool()?;
        self.eeprom.state = match r.read_u8()? {
            0 => EepromState::Standby,
            1 => EepromState::ReceiveControl,
            2 => EepromState::ReceiveAddress,
            3 => EepromState::ReceiveWriteData,
            4 => EepromState::SendReadData,
            other => anyhow::bail!("invalid mapper16 EEPROM state tag: {other}"),
        };
        self.eeprom.shift = r.read_u8()?;
        self.eeprom.bits = r.read_u8()?;
        self.eeprom.pointer = r.read_u8()?;
        self.eeprom.address_latched = r.read_bool()?;
        self.eeprom.read_phase = match r.read_u8()? {
            0 => EepromReadPhase::OutputBits,
            1 => EepromReadPhase::AwaitMasterAck,
            other => anyhow::bail!("invalid mapper16 EEPROM read phase tag: {other}"),
        };
        self.eeprom.read_bit = r.read_u8()?;

        let chr = r.read_vec(1024 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "Mapper16 CHR size mismatch: expected {}, got {}",
                self.chr.len(),
                chr.len()
            );
        }
        self.chr = chr;
        Ok(())
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_eeprom {
            Some(self.eeprom.data.to_vec())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_eeprom {
            return Ok(());
        }
        let copy_len = self.eeprom.data.len().min(bytes.len());
        self.eeprom.data[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.eeprom.data.len() {
            self.eeprom.data[copy_len..].fill(0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
