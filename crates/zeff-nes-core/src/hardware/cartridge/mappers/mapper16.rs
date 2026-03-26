use crate::hardware::cartridge::{Mapper, Mirroring};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EepromState {
    Standby,
    ReceiveControl,
    ReceiveAddress,
    ReceiveWriteData,
    SendReadData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EepromReadPhase {
    OutputBits,
    AwaitMasterAck,
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
    eeprom: [u8; 256],
    eeprom_scl: bool,
    eeprom_sda_in: bool,
    eeprom_read_enable: bool,
    eeprom_state: EepromState,
    eeprom_shift: u8,
    eeprom_bits: u8,
    eeprom_pointer: u8,
    eeprom_address_latched: bool,
    eeprom_read_phase: EepromReadPhase,
    eeprom_read_bit: u8,
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
            eeprom: [0; 256],
            eeprom_scl: true,
            eeprom_sda_in: true,
            eeprom_read_enable: false,
            eeprom_state: EepromState::Standby,
            eeprom_shift: 0,
            eeprom_bits: 0,
            eeprom_pointer: 0,
            eeprom_address_latched: false,
            eeprom_read_phase: EepromReadPhase::OutputBits,
            eeprom_read_bit: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn begin_receive_byte(&mut self, next_state: EepromState) {
        self.eeprom_state = next_state;
        self.eeprom_shift = 0;
        self.eeprom_bits = 0;
    }

    fn eeprom_start_condition(&mut self) {
        self.begin_receive_byte(EepromState::ReceiveControl);
        self.eeprom_address_latched = false;
    }

    fn eeprom_stop_condition(&mut self) {
        self.eeprom_state = EepromState::Standby;
        self.eeprom_read_phase = EepromReadPhase::OutputBits;
        self.eeprom_bits = 0;
    }

    fn eeprom_receive_byte_bit(&mut self, sda: bool) {
        self.eeprom_shift = (self.eeprom_shift << 1) | u8::from(sda);
        self.eeprom_bits += 1;
        if self.eeprom_bits < 8 {
            return;
        }

        let byte = self.eeprom_shift;
        self.eeprom_shift = 0;
        self.eeprom_bits = 0;

        match self.eeprom_state {
            EepromState::ReceiveControl => {
                let device_match = (byte & 0xF0) == 0xA0;
                let read = byte & 0x01 != 0;
                if !device_match {
                    self.eeprom_state = EepromState::Standby;
                    return;
                }
                if read {
                    self.eeprom_state = EepromState::SendReadData;
                    self.eeprom_read_phase = EepromReadPhase::OutputBits;
                    self.eeprom_read_bit = 0;
                } else if self.eeprom_address_latched {
                    self.eeprom_state = EepromState::ReceiveWriteData;
                } else {
                    self.eeprom_state = EepromState::ReceiveAddress;
                }
            }
            EepromState::ReceiveAddress => {
                self.eeprom_pointer = byte;
                self.eeprom_address_latched = true;
                self.eeprom_state = EepromState::ReceiveWriteData;
            }
            EepromState::ReceiveWriteData => {
                self.eeprom[self.eeprom_pointer as usize] = byte;
                self.eeprom_pointer = self.eeprom_pointer.wrapping_add(1);
            }
            _ => {}
        }
    }

    fn eeprom_clock_rising_edge(&mut self, sda: bool) {
        match self.eeprom_state {
            EepromState::SendReadData => match self.eeprom_read_phase {
                EepromReadPhase::OutputBits => {
                    self.eeprom_read_bit = self.eeprom_read_bit.saturating_add(1);
                    if self.eeprom_read_bit >= 8 {
                        self.eeprom_read_phase = EepromReadPhase::AwaitMasterAck;
                    }
                }
                EepromReadPhase::AwaitMasterAck => {
                    if sda {
                        self.eeprom_state = EepromState::Standby;
                    } else {
                        self.eeprom_pointer = self.eeprom_pointer.wrapping_add(1);
                        self.eeprom_read_phase = EepromReadPhase::OutputBits;
                        self.eeprom_read_bit = 0;
                    }
                }
            },
            EepromState::ReceiveControl
            | EepromState::ReceiveAddress
            | EepromState::ReceiveWriteData => self.eeprom_receive_byte_bit(sda),
            EepromState::Standby => {}
        }
    }

    fn handle_eeprom_control_write(&mut self, val: u8) {
        if !self.mode.eeprom_supported() || !self.has_eeprom {
            return;
        }

        let scl = val & 0x80 != 0;
        let sda_in = val & 0x40 != 0;
        self.eeprom_read_enable = val & 0x20 != 0;

        let prev_scl = self.eeprom_scl;
        let prev_sda = self.eeprom_sda_in;

        if prev_scl && scl {
            if prev_sda && !sda_in {
                self.eeprom_start_condition();
            } else if !prev_sda && sda_in {
                self.eeprom_stop_condition();
            }
        }

        if !prev_scl && scl {
            self.eeprom_clock_rising_edge(sda_in);
        }

        self.eeprom_scl = scl;
        self.eeprom_sda_in = sda_in;
    }

    fn eeprom_data_out(&self) -> bool {
        if !self.mode.eeprom_supported() || !self.has_eeprom || !self.eeprom_read_enable {
            return true;
        }

        match self.eeprom_state {
            EepromState::SendReadData => match self.eeprom_read_phase {
                EepromReadPhase::OutputBits => {
                    let byte = self.eeprom[self.eeprom_pointer as usize];
                    let bit = 7u8.saturating_sub(self.eeprom_read_bit);
                    ((byte >> bit) & 1) != 0
                }
                EepromReadPhase::AwaitMasterAck => true,
            },
            _ => true,
        }
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
    fn cpu_read(&self, addr: u16) -> u8 {
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

    fn chr_read(&self, addr: u16) -> u8 {
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
        w.write_bytes(&self.eeprom);
        w.write_bool(self.eeprom_scl);
        w.write_bool(self.eeprom_sda_in);
        w.write_bool(self.eeprom_read_enable);
        w.write_u8(match self.eeprom_state {
            EepromState::Standby => 0,
            EepromState::ReceiveControl => 1,
            EepromState::ReceiveAddress => 2,
            EepromState::ReceiveWriteData => 3,
            EepromState::SendReadData => 4,
        });
        w.write_u8(self.eeprom_shift);
        w.write_u8(self.eeprom_bits);
        w.write_u8(self.eeprom_pointer);
        w.write_bool(self.eeprom_address_latched);
        w.write_u8(match self.eeprom_read_phase {
            EepromReadPhase::OutputBits => 0,
            EepromReadPhase::AwaitMasterAck => 1,
        });
        w.write_u8(self.eeprom_read_bit);

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
        r.read_exact(&mut self.eeprom)?;
        self.eeprom_scl = r.read_bool()?;
        self.eeprom_sda_in = r.read_bool()?;
        self.eeprom_read_enable = r.read_bool()?;
        self.eeprom_state = match r.read_u8()? {
            0 => EepromState::Standby,
            1 => EepromState::ReceiveControl,
            2 => EepromState::ReceiveAddress,
            3 => EepromState::ReceiveWriteData,
            4 => EepromState::SendReadData,
            other => anyhow::bail!("invalid mapper16 EEPROM state tag: {other}"),
        };
        self.eeprom_shift = r.read_u8()?;
        self.eeprom_bits = r.read_u8()?;
        self.eeprom_pointer = r.read_u8()?;
        self.eeprom_address_latched = r.read_bool()?;
        self.eeprom_read_phase = match r.read_u8()? {
            0 => EepromReadPhase::OutputBits,
            1 => EepromReadPhase::AwaitMasterAck,
            other => anyhow::bail!("invalid mapper16 EEPROM read phase tag: {other}"),
        };
        self.eeprom_read_bit = r.read_u8()?;

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
            Some(self.eeprom.to_vec())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_eeprom {
            return Ok(());
        }
        let copy_len = self.eeprom.len().min(bytes.len());
        self.eeprom[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.eeprom.len() {
            self.eeprom[copy_len..].fill(0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_ctrl(m: &mut BandaiFcg16, scl: bool, sda: bool, read_en: bool) {
        let mut v = 0u8;
        if scl {
            v |= 0x80;
        }
        if sda {
            v |= 0x40;
        }
        if read_en {
            v |= 0x20;
        }
        m.cpu_write(0x800D, v);
    }

    fn i2c_start(m: &mut BandaiFcg16, read_en: bool) {
        write_ctrl(m, true, true, read_en);
        write_ctrl(m, true, false, read_en);
        write_ctrl(m, false, false, read_en);
    }

    fn i2c_stop(m: &mut BandaiFcg16, read_en: bool) {
        write_ctrl(m, false, false, read_en);
        write_ctrl(m, true, false, read_en);
        write_ctrl(m, true, true, read_en);
    }

    fn i2c_write_byte(m: &mut BandaiFcg16, b: u8) {
        for bit in (0..8).rev() {
            let sda = (b >> bit) & 1 != 0;
            write_ctrl(m, false, sda, false);
            write_ctrl(m, true, sda, false);
        }
        write_ctrl(m, false, true, false);
        write_ctrl(m, true, true, false);
        write_ctrl(m, false, true, false);
    }

    fn i2c_read_byte_nack(m: &mut BandaiFcg16) -> u8 {
        let mut out = 0u8;
        for _ in 0..8 {
            write_ctrl(m, false, true, true);
            write_ctrl(m, true, true, true);
            out = (out << 1) | u8::from(m.cpu_read(0x6000) & 0x10 != 0);
        }
        write_ctrl(m, false, true, true);
        write_ctrl(m, true, true, true);
        write_ctrl(m, false, true, true);
        out
    }

    #[test]
    fn mapper16_switches_prg_bank_at_8000() {
        let mut prg = vec![0u8; 3 * 0x4000];
        for bank in 0..3usize {
            prg[bank * 0x4000] = bank as u8;
        }
        let chr = vec![0u8; 0x2000];

        let mut mapper = BandaiFcg16::new(prg, chr, Mirroring::Horizontal, 4, false);
        mapper.cpu_write(0x6008, 0x01);

        assert_eq!(mapper.cpu_read(0x8000), 1);
    }

    #[test]
    fn mapper16_eeprom_write_then_read_random_access() {
        let prg = vec![0u8; 2 * 0x4000];
        let chr = vec![0u8; 0x2000];
        let mut mapper = BandaiFcg16::new(prg, chr, Mirroring::Horizontal, 5, true);

        i2c_start(&mut mapper, false);
        i2c_write_byte(&mut mapper, 0xA0);
        i2c_write_byte(&mut mapper, 0x12);
        i2c_write_byte(&mut mapper, 0xAB);
        i2c_stop(&mut mapper, false);

        i2c_start(&mut mapper, false);
        i2c_write_byte(&mut mapper, 0xA0);
        i2c_write_byte(&mut mapper, 0x12);
        i2c_start(&mut mapper, true);
        i2c_write_byte(&mut mapper, 0xA1);
        let read_back = i2c_read_byte_nack(&mut mapper);
        i2c_stop(&mut mapper, true);

        assert_eq!(read_back, 0xAB);
    }
}
