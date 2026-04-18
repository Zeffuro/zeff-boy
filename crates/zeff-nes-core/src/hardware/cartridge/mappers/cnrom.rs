use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Cnrom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    chr_bank_select: u8,
}

impl Cnrom {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            chr_bank_select: 0,
        }
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank = self.chr_bank_select as usize;
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for Cnrom {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.chr_bank_select = val & 0x03;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_index(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = self.chr_index(addr);
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.chr_bank_select);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.chr_bank_select = r.read_u8()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "CNROM")?;
        Ok(())
    }
}
