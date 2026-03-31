use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Axrom {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
}

impl Axrom {
    pub fn new(prg_rom: Vec<u8>, chr_ram: Vec<u8>, _mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram,
            mirroring: Mirroring::SingleScreenLower,
            prg_bank: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / 0x8000
    }
}

impl Mapper for Axrom {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank as usize % self.prg_bank_count();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[bank * 0x8000 + offset]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = val & 0x07;
            self.mirroring = if val & 0x10 != 0 {
                Mirroring::SingleScreenUpper
            } else {
                Mirroring::SingleScreenLower
            };
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
        if self.chr_ram.is_empty() {
            return 0;
        }
        self.chr_ram[addr as usize % self.chr_ram.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let len = self.chr_ram.len();
        if len > 0 {
            self.chr_ram[addr as usize % len] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.prg_bank);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_vec(&self.chr_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank = r.read_u8()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr_ram.len() {
            anyhow::bail!(
                "AxROM CHR-RAM size mismatch: expected {}, got {}",
                self.chr_ram.len(),
                chr.len()
            );
        }
        self.chr_ram = chr;
        Ok(())
    }
}
