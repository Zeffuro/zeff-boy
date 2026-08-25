use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Cnrom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,
    mirroring: Mirroring,
    chr_bank_select: u8,
    bus_conflicts: bool,
}

impl Cnrom {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        chr_is_ram: bool,
        mirroring: Mirroring,
        bus_conflicts: bool,
    ) -> Self {
        Self {
            prg_rom,
            chr,
            chr_is_ram,
            mirroring,
            chr_bank_select: 0,
            bus_conflicts,
        }
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank = self.chr_bank_select as usize;
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }

    fn bank_mask(&self) -> u8 {
        if self.chr.len() > 0x8000 { 0x0F } else { 0x03 }
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
            let effective_val = if self.bus_conflicts {
                val & self.cpu_peek(addr)
            } else {
                val
            };
            self.chr_bank_select = effective_val & self.bank_mask();
        }
    }

    fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        (0x8000..=0xFFFF)
            .contains(&addr)
            .then(|| (addr as usize - 0x8000) % self.prg_rom.len())
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_index(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() || !self.chr_is_ram {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn prg(fill: u8) -> Vec<u8> {
        vec![fill; 0x8000]
    }

    fn chr_banks(values: &[u8]) -> Vec<u8> {
        let mut chr = Vec::new();
        for &value in values {
            chr.extend(vec![value; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_chr_bank_with_bus_conflict_safe_write() {
        let mut prg = prg(0xFF);
        prg[0x1234] = 0x02;
        let mut mapper = Cnrom::new(
            prg,
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            false,
            Mirroring::Vertical,
            true,
        );

        mapper.cpu_write(0x9234, 0x02);

        assert_eq!(mapper.chr_read(0x0000), 0x22);
    }

    #[test]
    fn bus_conflict_ands_write_with_rom_byte() {
        let mut prg = prg(0xFF);
        prg[0] = 0x01;
        let mut mapper = Cnrom::new(
            prg,
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            false,
            Mirroring::Horizontal,
            true,
        );

        mapper.cpu_write(0x8000, 0x03);

        assert_eq!(mapper.chr_bank_select, 0x01);
        assert_eq!(mapper.chr_read(0x0000), 0x11);
    }

    #[test]
    fn no_bus_conflict_mode_uses_written_value_directly() {
        let mut prg = prg(0xFF);
        prg[0] = 0x78;
        let mut mapper = Cnrom::new(
            prg,
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            false,
            Mirroring::Horizontal,
            false,
        );

        mapper.cpu_write(0x8000, 0x01);

        assert_eq!(mapper.chr_bank_select, 0x01);
        assert_eq!(mapper.chr_read(0x1000), 0x11);
    }

    #[test]
    fn oversize_chr_uses_low_nibble_bank_select() {
        let mut mapper = Cnrom::new(
            prg(0xFF),
            chr_banks(&[0x00, 0x11, 0x22, 0x33, 0x44]),
            false,
            Mirroring::Horizontal,
            false,
        );

        mapper.cpu_write(0x8000, 0x04);

        assert_eq!(mapper.chr_read(0x0000), 0x44);
    }

    #[test]
    fn chr_writes_only_modify_ram() {
        let mut rom = Cnrom::new(
            prg(0xFF),
            chr_banks(&[0x11]),
            false,
            Mirroring::Horizontal,
            false,
        );
        let mut ram = Cnrom::new(
            prg(0xFF),
            chr_banks(&[0x11]),
            true,
            Mirroring::Horizontal,
            false,
        );

        rom.chr_write(0x1234, 0x22);
        ram.chr_write(0x1234, 0x22);

        assert_eq!(rom.chr_read(0x1234), 0x11);
        assert_eq!(ram.chr_read(0x1234), 0x22);
    }
}
