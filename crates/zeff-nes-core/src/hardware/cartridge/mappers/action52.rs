use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Action52 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,

    prg_page_lo: usize,
    prg_page_hi: usize,

    chr_bank: usize,
}

impl Action52 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, _mirroring: Mirroring) -> Self {
        let pages = (prg_rom.len() / 0x4000).max(1);
        Self {
            prg_rom,
            chr,
            mirroring: Mirroring::Vertical,
            prg_page_lo: 0,
            prg_page_hi: 1 % pages,
            chr_bank: 0,
        }
    }

    fn prg_16k_pages(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }
}

impl Mapper for Action52 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let offset = (addr as usize - 0x8000) + self.prg_page_lo * 0x4000;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                let offset = (addr as usize - 0xC000) + self.prg_page_hi * 0x4000;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x8000 {
            return;
        }

        let a = addr as usize;
        let pages = self.prg_16k_pages();

        let mirror_h = (a >> 13) & 1 != 0;
        let chip = (a >> 11) & 0x03;
        let prg_inner = (a >> 6) & 0x1F;
        let prg_mode_16k = (a >> 5) & 1 != 0;
        let chr_low = a & 0x0F;

        let chr_high = (val as usize) & 0x03;

        self.mirroring = if mirror_h {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };

        self.chr_bank = (chr_high << 4) | chr_low;

        let effective_chip = if chip == 3 { 2 } else { chip };
        let chip_base = effective_chip * 32;

        if prg_mode_16k {
            let page = (chip_base + prg_inner) % pages;
            self.prg_page_lo = page;
            self.prg_page_hi = page;
        } else {
            let base = chip_base + (prg_inner & 0x1E);
            self.prg_page_lo = base % pages;
            self.prg_page_hi = (base + 1) % pages;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let offset = self.chr_bank * 0x2000 + addr as usize;
        self.chr[offset % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let offset = self.chr_bank * 0x2000 + addr as usize;
        let len = self.chr.len();
        self.chr[offset % len] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u32(self.prg_page_lo as u32);
        w.write_u32(self.prg_page_hi as u32);
        w.write_u32(self.chr_bank as u32);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_page_lo = r.read_u32()? as usize;
        self.prg_page_hi = r.read_u32()? as usize;
        self.chr_bank = r.read_u32()? as usize;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        let chr = r.read_vec(2 * 1024 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "Action52 CHR size mismatch: expected {}, got {}",
                self.chr.len(),
                chr.len()
            );
        }
        self.chr = chr;
        Ok(())
    }
}
