use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mmc1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    shift_register: u8,
    shift_count: u8,

    control: u8,

    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
}

impl Mmc1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            shift_register: 0x10,
            shift_count: 0,
            control: 0x0C,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
        }
    }

    fn prg_mode(&self) -> u8 {
        (self.control >> 2) & 0x03
    }

    fn chr_mode(&self) -> bool {
        self.control & 0x10 != 0
    }

    fn update_mirroring(&mut self) {
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };
    }

    fn write_register(&mut self, addr: u16) {
        let value = self.shift_register;
        match addr {
            0x8000..=0x9FFF => {
                self.control = value & 0x1F;
                self.update_mirroring();
            }
            0xA000..=0xBFFF => {
                self.chr_bank_0 = value & 0x1F;
            }
            0xC000..=0xDFFF => {
                self.chr_bank_1 = value & 0x1F;
            }
            0xE000..=0xFFFF => {
                self.prg_bank = value & 0x0F;
            }
            _ => {}
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / 0x4000
    }
}

impl Mapper for Mmc1 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let bank = match self.prg_mode() {
                    0 | 1 => self.prg_bank as usize & 0xFE,
                    2 => 0,
                    3 => self.prg_bank as usize,
                    _ => unreachable!(),
                };
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank % self.prg_bank_count()) * 0x4000 + offset]
            }
            0xC000..=0xFFFF => {
                let bank = match self.prg_mode() {
                    0 | 1 => self.prg_bank as usize | 0x01,
                    2 => self.prg_bank as usize,
                    3 => self.prg_bank_count() - 1,
                    _ => unreachable!(),
                };
                let offset = (addr - 0xC000) as usize;
                self.prg_rom[(bank % self.prg_bank_count()) * 0x4000 + offset]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0xFFFF => {
                if val & 0x80 != 0 {
                    self.shift_register = 0x10;
                    self.shift_count = 0;
                    self.control |= 0x0C;
                } else {
                    self.shift_register >>= 1;
                    self.shift_register |= (val & 0x01) << 4;
                    self.shift_count += 1;
                    if self.shift_count == 5 {
                        self.write_register(addr);
                        self.shift_register = 0x10;
                        self.shift_count = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let index = if self.chr_mode() {
            match addr {
                0x0000..=0x0FFF => {
                    (self.chr_bank_0 as usize) * 0x1000 + addr as usize
                }
                0x1000..=0x1FFF => {
                    (self.chr_bank_1 as usize) * 0x1000 + (addr - 0x1000) as usize
                }
                _ => addr as usize,
            }
        } else {
            (self.chr_bank_0 as usize >> 1) * 0x2000 + addr as usize
        };
        self.chr[index % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let index = if self.chr_mode() {
            match addr {
                0x0000..=0x0FFF => {
                    (self.chr_bank_0 as usize) * 0x1000 + addr as usize
                }
                0x1000..=0x1FFF => {
                    (self.chr_bank_1 as usize) * 0x1000 + (addr - 0x1000) as usize
                }
                _ => addr as usize,
            }
        } else {
            (self.chr_bank_0 as usize >> 1) * 0x2000 + addr as usize
        };
        let len = self.chr.len();
        self.chr[index % len] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(self.shift_register);
        w.write_u8(self.shift_count);
        w.write_u8(self.control);
        w.write_u8(self.chr_bank_0);
        w.write_u8(self.chr_bank_1);
        w.write_u8(self.prg_bank);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.shift_register = r.read_u8()?;
        self.shift_count = r.read_u8()?;
        self.control = r.read_u8()?;
        self.chr_bank_0 = r.read_u8()?;
        self.chr_bank_1 = r.read_u8()?;
        self.prg_bank = r.read_u8()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!("MMC1 CHR size mismatch: expected {}, got {}", self.chr.len(), chr.len());
        }
        self.chr = chr;
        Ok(())
    }
}
