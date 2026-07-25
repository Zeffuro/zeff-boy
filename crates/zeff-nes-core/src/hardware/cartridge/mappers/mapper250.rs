use crate::hardware::cartridge::{Mapper, Mirroring};

use super::Mmc3;

pub struct Mapper250 {
    mmc3: Mmc3,
}

impl Mapper250 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            mmc3: Mmc3::new(prg_rom, chr, mirroring),
        }
    }
}

impl Mapper for Mapper250 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        self.mmc3.cpu_peek(addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.mmc3.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.mmc3.cpu_write(addr, val),
            0x8000..=0xFFFF => {
                let mapped_addr = (addr & 0xE000) | u16::from(addr & 0x0400 != 0);
                let mapped_val = (addr & 0x00FF) as u8;
                self.mmc3.cpu_write(mapped_addr, mapped_val);
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.mmc3.chr_read(addr)
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        self.mmc3.chr_write(addr, val);
    }

    fn mirroring(&self) -> Mirroring {
        self.mmc3.mirroring()
    }

    fn irq_pending(&self) -> bool {
        self.mmc3.irq_pending()
    }

    fn notify_scanline(&mut self) {
        self.mmc3.notify_scanline();
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        self.mmc3.write_state(w);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mmc3.read_state(r)
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.mmc3.dump_battery_data()
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.mmc3.load_battery_data(bytes)
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
    fn uses_a10_as_mmc3_register_select_and_low_address_as_data() {
        let mut mapper = Mapper250::new(prg_banks(16), chr_banks(16), Mirroring::Vertical);

        mapper.cpu_write(0x8006, 0xFF);
        mapper.cpu_write(0x8403, 0x00);
        mapper.cpu_write(0x8007, 0xFF);
        mapper.cpu_write(0x8404, 0x00);

        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xA000), 0x04);

        mapper.cpu_write(0x8002, 0xFF);
        mapper.cpu_write(0x8405, 0x00);
        assert_eq!(mapper.chr_read(0x1000), 0x05);
    }

    #[test]
    fn preserves_mmc3_irq_behavior() {
        let mut mapper = Mapper250::new(prg_banks(16), chr_banks(16), Mirroring::Vertical);

        mapper.cpu_write(0xC002, 0x00);
        mapper.cpu_write(0xC400, 0x00);
        mapper.cpu_write(0xE400, 0x00);

        mapper.notify_scanline();
        mapper.notify_scanline();
        mapper.notify_scanline();

        assert!(mapper.irq_pending());
    }
}
