use crate::hardware::cartridge::{ChrFetchKind, Mapper, Mirroring};

pub struct Cnrom185 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    chip_select: u8,
    enable_chip_select: Option<u8>,
    startup_open_bus_reads: u8,
}

impl Cnrom185 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, submapper_id: u8) -> Self {
        let enable_chip_select = match submapper_id {
            4..=7 => Some(submapper_id - 4),
            _ => None,
        };

        Self {
            prg_rom,
            chr,
            mirroring,
            chip_select: 0,
            enable_chip_select,
            startup_open_bus_reads: if enable_chip_select.is_some() { 0 } else { 2 },
        }
    }

    fn chr_enabled(&self) -> bool {
        match self.enable_chip_select {
            Some(enable_chip_select) => self.chip_select == enable_chip_select,
            None => self.startup_open_bus_reads == 0,
        }
    }

    fn chr_index(&self, addr: u16) -> usize {
        addr as usize % self.chr.len()
    }

    fn open_bus(addr: u16) -> u8 {
        if addr & 0x0001 != 0 { 0xFF } else { 0xFE }
    }
}

impl Mapper for Cnrom185 {
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
            self.chip_select = (val & self.cpu_peek(addr)) & 0x03;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_read_kind(addr, ChrFetchKind::Background)
    }

    fn chr_read_kind(&mut self, addr: u16, kind: ChrFetchKind) -> u8 {
        if matches!(kind, ChrFetchKind::CpuData)
            && self.enable_chip_select.is_none()
            && self.startup_open_bus_reads > 0
        {
            self.startup_open_bus_reads -= 1;
            return Self::open_bus(addr);
        }

        if self.chr.is_empty() || !self.chr_enabled() {
            return Self::open_bus(addr);
        }
        self.chr[self.chr_index(addr)]
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.chip_select);
        w.write_u8(self.enable_chip_select.unwrap_or(0xFF));
        w.write_u8(self.startup_open_bus_reads);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.chip_select = r.read_u8()? & 0x03;
        self.enable_chip_select = match r.read_u8()? {
            val @ 0..=3 => Some(val),
            _ => None,
        };
        self.startup_open_bus_reads = r.read_u8()?.min(2);
        crate::save_state::read_chr_state(r, &mut self.chr, "CNROM-185")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg() -> Vec<u8> {
        vec![0xFF; 0x8000]
    }

    fn chr() -> Vec<u8> {
        let mut chr = vec![0; 0x2000];
        chr[0] = 0x3C;
        chr[1] = 0x42;
        chr
    }

    #[test]
    fn submapper_selects_chr_enable_value() {
        let mut mapper = Cnrom185::new(prg(), chr(), Mirroring::Vertical, 5);

        mapper.cpu_write(0x8000, 0x00);
        assert_ne!(mapper.chr_read(0x0000), 0x3C);

        mapper.cpu_write(0x8000, 0x01);
        assert_eq!(mapper.chr_read(0x0000), 0x3C);
    }

    #[test]
    fn old_ines_heuristic_only_consumes_cpu_data_reads() {
        let mut mapper = Cnrom185::new(prg(), chr(), Mirroring::Vertical, 0);

        assert_eq!(mapper.chr_read_kind(0x0000, ChrFetchKind::Background), 0xFE);
        assert_ne!(mapper.chr_read_kind(0x0000, ChrFetchKind::CpuData), 0x3C);
        assert_ne!(mapper.chr_read_kind(0x0001, ChrFetchKind::CpuData), 0x42);
        assert_eq!(mapper.chr_read_kind(0x0000, ChrFetchKind::CpuData), 0x3C);
    }
}
