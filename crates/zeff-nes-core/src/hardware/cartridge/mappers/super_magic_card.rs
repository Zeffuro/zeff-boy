use crate::hardware::cartridge::{Mapper, Mirroring, RomFormat, RomHeader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InitialState {
    Mapper6,
    Mapper17,
}

pub struct SuperMagicCard {
    prg_rom: Vec<u8>,
    wram: Vec<u8>,
    scratch: [u8; 0x1000],
    chr: Vec<u8>,
    mirroring: Mirroring,

    latch_mode: u8,
    latch: u8,
    latch_enabled: bool,

    extended_enabled: bool,
    four_m_mode: bool,
    chr_1k_mode: bool,
    use_ciram: bool,
    irq_source_pa12: bool,
    play_mode: bool,
    wram_bank: u8,
    chr_8k_bank: u8,
    prg_2m_banks: [u8; 4],
    prg_4m_banks: [u8; 4],
    chr_1k_banks: [u8; 8],
    chr_nt_banks: [u8; 4],

    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl SuperMagicCard {
    pub fn new_mapper6(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        submapper: u8,
        has_chr_rom: bool,
    ) -> Self {
        let mut mapper = Self::new(prg_rom, chr, mirroring, has_chr_rom, InitialState::Mapper6);
        mapper.latch_mode = if submapper == 0 { 1 } else { submapper & 0x07 };
        mapper
    }

    pub fn new_mapper17(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        has_chr_rom: bool,
    ) -> Self {
        Self::new(prg_rom, chr, mirroring, has_chr_rom, InitialState::Mapper17)
    }

    fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        has_chr_rom: bool,
        state: InitialState,
    ) -> Self {
        let chr = if has_chr_rom {
            chr
        } else {
            let size = if state == InitialState::Mapper17 {
                0x40000
            } else {
                0x8000
            };
            vec![0; size]
        };
        let prg_bank_count = (prg_rom.len() / 0x2000).max(1);
        let last = prg_bank_count.saturating_sub(1);

        let mut mapper = Self {
            prg_rom,
            wram: vec![0; 0x8000],
            scratch: [0; 0x1000],
            chr,
            mirroring,
            latch_mode: 1,
            latch: 0,
            latch_enabled: true,
            extended_enabled: false,
            four_m_mode: false,
            chr_1k_mode: false,
            use_ciram: true,
            irq_source_pa12: false,
            play_mode: true,
            wram_bank: 0,
            chr_8k_bank: 0,
            prg_2m_banks: [0, 1, 2, last as u8],
            prg_4m_banks: [
                prg_bank_count.saturating_sub(4) as u8,
                prg_bank_count.saturating_sub(3) as u8,
                prg_bank_count.saturating_sub(2) as u8,
                last as u8,
            ],
            chr_1k_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            chr_nt_banks: [8, 9, 10, 11],
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        };

        match state {
            InitialState::Mapper6 => {
                mapper.extended_enabled = false;
                mapper.four_m_mode = false;
                mapper.chr_1k_mode = false;
            }
            InitialState::Mapper17 => {
                mapper.extended_enabled = true;
                mapper.four_m_mode = true;
                mapper.chr_1k_mode = true;
            }
        }

        mapper
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn read_prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = addr as usize & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn read_prg_16k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / 0x4000).max(1);
        let bank = bank % count;
        let offset = addr as usize & 0x3FFF;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }

    fn read_prg_32k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / 0x8000).max(1);
        let bank = bank % count;
        let offset = addr as usize - 0x8000;
        self.prg_rom[(bank * 0x8000 + offset) % self.prg_rom.len()]
    }

    fn latch_prg_read(&self, addr: u16) -> u8 {
        match self.latch_mode & 0x07 {
            0 => match addr {
                0x8000..=0xBFFF => self.read_prg_16k(usize::from(self.latch & 0x07), addr),
                _ => self.read_prg_16k(7, addr),
            },
            1 => match addr {
                0x8000..=0xBFFF => self.read_prg_16k(usize::from((self.latch >> 2) & 0x0F), addr),
                _ => self.read_prg_16k(7, addr),
            },
            2 => match addr {
                0x8000..=0xBFFF => self.read_prg_16k(usize::from(self.latch & 0x0F), addr),
                _ => self.read_prg_16k(15, addr),
            },
            3 => match addr {
                0x8000..=0xBFFF => self.read_prg_16k(15, addr),
                _ => self.read_prg_16k(usize::from(self.latch & 0x0F), addr),
            },
            4 => self.read_prg_32k(usize::from((self.latch >> 4) & 0x03), addr),
            5 => self.read_prg_32k(3, addr),
            6 => self.read_prg_32k(3, addr),
            7 => self.read_prg_32k(3, addr),
            _ => unreachable!(),
        }
    }

    fn current_prg_bank(&self, slot: usize) -> usize {
        if self.extended_enabled {
            if self.four_m_mode {
                usize::from(self.prg_4m_banks[slot])
            } else {
                usize::from(self.prg_2m_banks[slot] & 0x3F)
            }
        } else {
            0
        }
    }

    fn chr_8k_bank_from_latch(&self) -> usize {
        match self.latch_mode & 0x07 {
            1 => usize::from(self.latch & 0x03),
            3 => usize::from((self.latch >> 4) & 0x03),
            4 | 5 => usize::from(self.latch & 0x03),
            6 => usize::from(self.latch & 0x01),
            _ => usize::from(self.chr_8k_bank),
        }
    }

    fn chr_index_1k(&self, bank: usize, offset: usize) -> usize {
        let bank = bank % self.chr_bank_count_1k();
        (bank * 0x0400 + offset) % self.chr.len()
    }

    fn chr_index(&self, addr: u16) -> usize {
        if self.chr_1k_mode {
            let slot = addr as usize / 0x0400;
            let bank = usize::from(self.chr_1k_banks[slot]);
            self.chr_index_1k(bank, addr as usize & 0x03FF)
        } else {
            let bank = if self.extended_enabled && !self.four_m_mode {
                usize::from(self.chr_8k_bank)
            } else {
                self.chr_8k_bank_from_latch()
            } % self.chr_bank_count_8k();
            (bank * 0x2000 + addr as usize) % self.chr.len()
        }
    }

    fn set_mirroring_from_1m(&mut self, addr: u16, val: u8) {
        if addr & 0x01 != 0 {
            self.mirroring = if val & 0x10 != 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        }
        self.latch_enabled = addr & 0x02 != 0;
        self.latch_mode = (val >> 5) & 0x07;
    }

    fn write_latch(&mut self, addr: u16, val: u8) {
        let slot = usize::from((addr - 0x8000) / 0x2000);
        self.prg_2m_banks[slot] = (val >> 2) & 0x3F;
        self.chr_8k_bank = val & 0x03;
        if self.latch_enabled {
            self.latch = val;
        }
    }

    fn load_trainer_at(&mut self, start: u16, bytes: &[u8]) {
        for (offset, &byte) in bytes.iter().enumerate() {
            let addr = start.wrapping_add(offset as u16);
            match addr {
                0x5000..=0x5FFF => self.scratch[addr as usize & 0x0FFF] = byte,
                0x6000..=0x7FFF => self.wram[addr as usize & 0x1FFF] = byte,
                _ => {}
            }
        }
    }
}

impl Mapper for SuperMagicCard {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x4500 => {
                let mut value = 0x80;
                value |= (self.latch_mode & 0x07) << 4;
                if self.latch_enabled {
                    value |= 0x02;
                }
                if matches!(
                    self.mirroring,
                    Mirroring::SingleScreenUpper | Mirroring::Horizontal
                ) {
                    value |= 0x04;
                }
                value
            }
            0x4501 => self.latch & 0xFC,
            0x5000..=0x5FFF => self.scratch[addr as usize & 0x0FFF],
            0x6000..=0x7FFF => {
                let bank = usize::from(self.wram_bank & 0x03);
                self.wram[(bank * 0x2000 + (addr as usize & 0x1FFF)) % self.wram.len()]
            }
            0x8000..=0xFFFF => {
                if self.extended_enabled {
                    let slot = usize::from((addr - 0x8000) / 0x2000);
                    self.read_prg_8k(self.current_prg_bank(slot), addr)
                } else {
                    self.latch_prg_read(addr)
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x42FC..=0x42FF => self.set_mirroring_from_1m(addr, val),
            0x43FC..=0x43FF => {
                self.extended_enabled = addr & 0x01 == 0;
                self.four_m_mode = addr & 0x02 == 0;
                self.chr_8k_bank = val & 0x03;
            }
            0x4500 => {
                self.chr_1k_mode = val & 0x01 != 0;
                self.use_ciram = val & 0x02 != 0;
                self.irq_source_pa12 = val & 0x08 != 0;
                self.wram_bank = (val >> 4) & 0x03;
                self.play_mode = val & 0x40 != 0;
            }
            0x4501 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0x4502 => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(val);
                self.irq_pending = false;
            }
            0x4503 => {
                self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(val) << 8);
                self.irq_enabled = true;
                self.irq_pending = false;
            }
            0x4504..=0x4507 => {
                self.prg_4m_banks[usize::from(addr & 0x0003)] = val & 0x3F;
                if !self.four_m_mode {
                    self.prg_2m_banks[usize::from(addr & 0x0003)] = (val >> 2) & 0x3F;
                    self.chr_8k_bank = val & 0x03;
                }
            }
            0x4510..=0x4517 => self.chr_1k_banks[usize::from(addr & 0x0007)] = val,
            0x4518..=0x451B => self.chr_nt_banks[usize::from(addr & 0x0003)] = val,
            0x5000..=0x5FFF => self.scratch[addr as usize & 0x0FFF] = val,
            0x6000..=0x7FFF => {
                let bank = usize::from(self.wram_bank & 0x03);
                let idx = (bank * 0x2000 + (addr as usize & 0x1FFF)) % self.wram.len();
                self.wram[idx] = val;
            }
            0x8000..=0xFFFF => self.write_latch(addr, val),
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let idx = self.chr_index(addr);
        self.chr[idx]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = self.chr_index(addr);
        self.chr[idx] = val;
    }

    fn ppu_nametable_read(&mut self, addr: u16, _ciram: &[u8]) -> Option<u8> {
        if self.use_ciram || self.chr.is_empty() {
            return None;
        }
        let slot = usize::from((addr - 0x2000) / 0x0400) & 0x03;
        let offset = addr as usize & 0x03FF;
        let idx = self.chr_index_1k(usize::from(self.chr_nt_banks[slot]), offset);
        Some(self.chr[idx])
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, _ciram: &mut [u8]) -> bool {
        if self.use_ciram || self.chr.is_empty() {
            return false;
        }
        let slot = usize::from((addr - 0x2000) / 0x0400) & 0x03;
        let offset = addr as usize & 0x03FF;
        let idx = self.chr_index_1k(usize::from(self.chr_nt_banks[slot]), offset);
        self.chr[idx] = val;
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        if self.irq_source_pa12 {
            for _ in 0..8 {
                self.clock_irq();
            }
        }
    }

    fn clock_cpu(&mut self) {
        if !self.irq_source_pa12 {
            self.clock_irq();
        }
    }

    fn load_trainer(&mut self, bytes: &[u8], header: &RomHeader) -> anyhow::Result<()> {
        let start = match header.submapper_id {
            1 => 0x5D00,
            2 => 0x5E00,
            3 => 0x5F00,
            0 if header.format == RomFormat::INes && bytes.starts_with(&[0x6C, 0xFC, 0xFF]) => {
                0x5D00
            }
            _ => 0x7000,
        };
        self.load_trainer_at(start, bytes);
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_vec(&self.wram);
        w.write_bytes(&self.scratch);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.latch_mode);
        w.write_u8(self.latch);
        w.write_bool(self.latch_enabled);
        w.write_bool(self.extended_enabled);
        w.write_bool(self.four_m_mode);
        w.write_bool(self.chr_1k_mode);
        w.write_bool(self.use_ciram);
        w.write_bool(self.irq_source_pa12);
        w.write_bool(self.play_mode);
        w.write_u8(self.wram_bank);
        w.write_u8(self.chr_8k_bank);
        w.write_bytes(&self.prg_2m_banks);
        w.write_bytes(&self.prg_4m_banks);
        w.write_bytes(&self.chr_1k_banks);
        w.write_bytes(&self.chr_nt_banks);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        let wram = r.read_vec(0x8000)?;
        self.wram.fill(0);
        let len = self.wram.len().min(wram.len());
        self.wram[..len].copy_from_slice(&wram[..len]);
        r.read_exact(&mut self.scratch)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.latch_mode = r.read_u8()? & 0x07;
        self.latch = r.read_u8()?;
        self.latch_enabled = r.read_bool()?;
        self.extended_enabled = r.read_bool()?;
        self.four_m_mode = r.read_bool()?;
        self.chr_1k_mode = r.read_bool()?;
        self.use_ciram = r.read_bool()?;
        self.irq_source_pa12 = r.read_bool()?;
        self.play_mode = r.read_bool()?;
        self.wram_bank = r.read_u8()? & 0x03;
        self.chr_8k_bank = r.read_u8()? & 0x03;
        r.read_exact(&mut self.prg_2m_banks)?;
        r.read_exact(&mut self.prg_4m_banks)?;
        r.read_exact(&mut self.chr_1k_banks)?;
        r.read_exact(&mut self.chr_nt_banks)?;
        self.irq_counter = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Super Magic Card")?;
        Ok(())
    }
}

impl SuperMagicCard {
    fn clock_irq(&mut self) {
        if !self.irq_enabled || self.irq_pending {
            return;
        }
        let old = self.irq_counter;
        self.irq_counter = self.irq_counter.wrapping_add(1);
        if old == 0xFFFF {
            self.irq_pending = true;
        }
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

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0400]);
        }
        chr
    }

    #[test]
    fn mapper17_starts_in_4m_prg_and_1k_chr_mode() {
        let mut mapper =
            SuperMagicCard::new_mapper17(prg_banks(64), chr_banks(256), Mirroring::Vertical, true);

        assert_eq!(mapper.cpu_peek(0x8000), 60);
        assert_eq!(mapper.cpu_peek(0xA000), 61);
        assert_eq!(mapper.cpu_peek(0xC000), 62);
        assert_eq!(mapper.cpu_peek(0xE000), 63);
        assert_eq!(mapper.chr_read(0x0000), 0);

        mapper.cpu_write(0x4504, 3);
        mapper.cpu_write(0x4510, 9);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.chr_read(0x0000), 9);
    }

    #[test]
    fn mapper6_uses_ines_latch_mode_one_by_default() {
        let mut mapper = SuperMagicCard::new_mapper6(
            prg_banks(32),
            vec![0; 0x2000],
            Mirroring::Vertical,
            0,
            false,
        );

        mapper.cpu_write(0x8000, 0x14);
        assert_eq!(mapper.cpu_peek(0x8000), 10);
        assert_eq!(mapper.cpu_peek(0xC000), 14);
        mapper.chr_write(0x0200, 0xA5);
        assert_eq!(mapper.chr_read(0x0200), 0xA5);
    }

    #[test]
    fn mapper6_latch_modes_six_and_seven_use_fixed_32k_bank_three() {
        let mut mapper = SuperMagicCard::new_mapper6(
            prg_banks(32),
            vec![0; 0x2000],
            Mirroring::Vertical,
            0,
            false,
        );

        mapper.cpu_write(0x42FF, 0xC0);
        assert_eq!(mapper.cpu_peek(0x8000), 12);
        assert_eq!(mapper.cpu_peek(0xE000), 15);

        mapper.cpu_write(0x42FF, 0xE0);
        assert_eq!(mapper.cpu_peek(0x8000), 12);
        assert_eq!(mapper.cpu_peek(0xE000), 15);
    }

    #[test]
    fn mapper6_42fe_does_not_change_mirroring() {
        let mut mapper = SuperMagicCard::new_mapper6(
            prg_banks(32),
            vec![0; 0x2000],
            Mirroring::Horizontal,
            0,
            false,
        );

        mapper.cpu_write(0x42FE, 0x30);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert_eq!(mapper.latch_mode, 1);
        assert!(mapper.latch_enabled);

        mapper.cpu_write(0x42FF, 0x20);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        mapper.cpu_write(0x42FF, 0x30);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn irq_wrap_sets_pending() {
        let mut mapper =
            SuperMagicCard::new_mapper17(prg_banks(4), chr_banks(8), Mirroring::Vertical, true);

        mapper.cpu_write(0x4502, 0xFF);
        mapper.cpu_write(0x4503, 0xFF);
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
    }
}
