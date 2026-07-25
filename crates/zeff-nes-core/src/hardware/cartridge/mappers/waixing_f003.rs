use crate::hardware::cartridge::{ChrFetchKind, Mapper, Mirroring};

pub struct WaixingF003 {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 0x2000],
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    fixed_four_screen: bool,

    bank_select: u8,
    bank_registers: [u8; 8],
    prg_ram_enable: bool,
    prg_ram_write_protect: bool,

    prg_upper_chr_bank: u8,

    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl WaixingF003 {
    pub fn new(prg_rom: Vec<u8>, _chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram: [0; 0x2000],
            prg_ram: [0; 0x2000],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            bank_select: 0,
            bank_registers: [0; 8],
            prg_ram_enable: true,
            prg_ram_write_protect: false,
            prg_upper_chr_bank: 0,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_half_bank_count(&self) -> usize {
        (self.prg_rom.len() / 0x80000).max(1)
    }

    fn active_prg_half(&self) -> usize {
        let half_count = self.prg_half_bank_count();
        if half_count <= 1 {
            0
        } else {
            (((self.prg_upper_chr_bank & 0x02) >> 1) as usize) % half_count
        }
    }

    fn prg_bank_base_8k(&self) -> usize {
        self.active_prg_half() * 0x40
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_8k();
        let base = self.prg_bank_base_8k();
        let half_banks = 0x40usize.min(bank_count.saturating_sub(base).max(1));
        let last = half_banks.saturating_sub(1);
        let second_last = half_banks.saturating_sub(2);
        let prg_mode = (self.bank_select >> 6) & 1;

        let inner = match addr {
            0x8000..=0x9FFF => {
                if prg_mode == 0 {
                    self.bank_registers[6] as usize
                } else {
                    second_last
                }
            }
            0xA000..=0xBFFF => self.bank_registers[7] as usize,
            0xC000..=0xDFFF => {
                if prg_mode == 0 {
                    second_last
                } else {
                    self.bank_registers[6] as usize
                }
            }
            0xE000..=0xFFFF => last,
            _ => 0,
        } % half_banks;

        (base + inner) % bank_count
    }
}

impl Mapper for WaixingF003 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enable => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank = self.map_prg_bank(addr);
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enable && !self.prg_ram_write_protect => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    self.bank_select = val;
                } else {
                    let register = (self.bank_select & 0x07) as usize;
                    self.bank_registers[register] = val;
                    if register <= 5 {
                        self.prg_upper_chr_bank = val;
                    }
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    if !self.fixed_four_screen {
                        self.mirroring = if val & 1 == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                } else {
                    self.prg_ram_enable = val & 0x80 != 0;
                    self.prg_ram_write_protect = val & 0x40 != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_read_kind(addr, ChrFetchKind::Background)
    }

    fn chr_read_kind(&mut self, addr: u16, _kind: ChrFetchKind) -> u8 {
        self.chr_ram[(addr as usize) & 0x1FFF]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        self.chr_ram[(addr as usize) & 0x1FFF] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        let old = self.irq_counter;

        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled && (old != 0 || self.irq_reload) {
            self.irq_pending = true;
        }

        self.irq_reload = false;
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_bytes(&self.chr_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);

        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);
        w.write_bool(self.prg_ram_enable);
        w.write_bool(self.prg_ram_write_protect);
        w.write_u8(self.prg_upper_chr_bank);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_bool(self.irq_reload);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        r.read_exact(&mut self.chr_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;

        self.bank_select = r.read_u8()?;
        r.read_exact(&mut self.bank_registers)?;
        self.prg_ram_enable = r.read_bool()?;
        self.prg_ram_write_protect = r.read_bool()?;
        self.prg_upper_chr_bank = r.read_u8()?;

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_reload = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        Ok(())
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        Some(self.prg_ram.to_vec())
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let len = bytes.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&bytes[..len]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x2000]);
        }
        prg
    }

    #[test]
    fn keeps_chr_ram_unbanked_even_after_chr_register_writes() {
        let mut mapper = WaixingF003::new(prg_banks(64), Vec::new(), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x02);
        mapper.cpu_write(0x8001, 0x05);

        mapper.chr_write(0x1000, 0xA5);
        assert_eq!(mapper.chr_read(0x1000), 0xA5);
        assert_eq!(mapper.chr_read(0x0000), 0x00);
    }

    #[test]
    fn behaves_like_tnrom_for_512k_prg() {
        let mut mapper = WaixingF003::new(prg_banks(64), Vec::new(), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);
        mapper.cpu_write(0x8000, 0x07);
        mapper.cpu_write(0x8001, 0x04);

        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xA000), 0x04);
        assert_eq!(mapper.cpu_peek(0xC000), 0x3E);
        assert_eq!(mapper.cpu_peek(0xE000), 0x3F);
    }

    #[test]
    fn chr_register_write_bit_selects_1m_prg_half() {
        let mut mapper = WaixingF003::new(prg_banks(128), Vec::new(), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x02);
        mapper.cpu_write(0x8001, 0x02);

        assert_eq!(mapper.cpu_peek(0xE000), 0x7F);
    }

    #[test]
    fn prg_register_write_does_not_select_1m_prg_half() {
        let mut mapper = WaixingF003::new(prg_banks(128), Vec::new(), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x02);

        assert_eq!(mapper.cpu_peek(0xE000), 0x3F);
    }
}
