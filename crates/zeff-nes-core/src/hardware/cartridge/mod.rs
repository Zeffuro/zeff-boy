mod dispatch;
mod header;
pub mod mappers;

use anyhow::{Result, bail};

use dispatch::MapperImpl;
pub use header::{
    ChrFetchKind, ConsoleType, Mirroring, NesMapper, RomFormat, RomHeader, TimingMode,
};

const HEADER_SIZE: usize = 16;
const TRAINER_SIZE: usize = 512;

pub(crate) trait Mapper: Send {
    fn cpu_peek(&self, addr: u16) -> u8;
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.cpu_peek(addr)
    }
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&mut self, addr: u16) -> u8;
    fn chr_read_kind(&mut self, addr: u16, _kind: ChrFetchKind) -> u8 {
        self.chr_read(addr)
    }
    fn chr_write(&mut self, addr: u16, val: u8);
    fn ppu_nametable_read(&mut self, _addr: u16, _ciram: &[u8; 0x800]) -> Option<u8> {
        None
    }
    fn ppu_nametable_write(&mut self, _addr: u16, _val: u8, _ciram: &mut [u8; 0x800]) -> bool {
        false
    }
    fn mirroring(&self) -> Mirroring;
    fn write_state(&self, w: &mut crate::save_state::StateWriter);
    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()>;

    fn irq_pending(&self) -> bool {
        false
    }

    fn notify_scanline(&mut self) {}

    fn clock_cpu(&mut self) {}

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        None
    }

    fn load_battery_data(&mut self, _bytes: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct Cartridge {
    header: RomHeader,
    mapper: MapperImpl,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> Result<Self> {
        let header = RomHeader::parse(rom_data)?;
        header.display_info();

        if header.prg_rom_size == 0 {
            bail!(
                "ROM declares 0 bytes of PRG ROM, which is invalid:every NES ROM needs at least one PRG bank"
            );
        }

        let trainer_offset = if header.has_trainer { TRAINER_SIZE } else { 0 };
        let prg_start = HEADER_SIZE + trainer_offset;
        let prg_size = header.prg_rom_size;
        let chr_start = prg_start + prg_size;
        let chr_size = header.chr_rom_size;

        let expected_min = chr_start + chr_size;
        if rom_data.len() < expected_min {
            bail!(
                "ROM file truncated: header declares {} bytes of PRG+CHR data but file only has {} bytes total",
                expected_min,
                rom_data.len()
            );
        }

        let prg_rom = rom_data[prg_start..prg_start + prg_size].to_vec();
        let chr_rom = if chr_size > 0 {
            rom_data[chr_start..chr_start + chr_size].to_vec()
        } else {
            vec![0; header::CHR_ROM_BANK_SIZE]
        };

        let mapper_kind = header.mapper_kind();
        let mapper = match mapper_kind {
            NesMapper::Nrom => {
                MapperImpl::Nrom(mappers::Nrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::SxRom => {
                MapperImpl::Mmc1(mappers::Mmc1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::UxRom => {
                MapperImpl::Uxrom(mappers::Uxrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::CnRom => {
                MapperImpl::Cnrom(mappers::Cnrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::TxRom => {
                MapperImpl::Mmc3(mappers::Mmc3::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::ExRom => MapperImpl::Mmc5(mappers::Mmc5::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery || header.prg_nvram_size > 0,
            )),
            NesMapper::AxRom => {
                MapperImpl::Axrom(mappers::Axrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::BandaiEprom24C02 => MapperImpl::BandaiFcg16(mappers::BandaiFcg16::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.submapper_id,
                header.has_battery || header.prg_nvram_size >= 256,
            )),
            NesMapper::Vrc4A => {
                let (a0, a1) = match header.submapper_id {
                    1 => (0x02, 0x04),
                    2 => (0x40, 0x80),
                    _ => (0x02 | 0x40, 0x04 | 0x80),
                };
                MapperImpl::Vrc4(mappers::Vrc4::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    a0,
                    a1,
                ))
            }
            NesMapper::Fme7 => MapperImpl::Fme7(mappers::Fme7::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery,
            )),
            NesMapper::Action52 => {
                MapperImpl::Action52(mappers::Action52::new(prg_rom, chr_rom, header.mirroring))
            }
            _ => bail!(
                "Unsupported mapper: {}. This mapper is not yet implemented",
                header.mapper_label()
            ),
        };

        Ok(Self { header, mapper })
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        self.mapper.cpu_read(addr)
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.mapper.cpu_peek(addr)
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    #[inline]
    pub fn chr_read(&mut self, addr: u16) -> u8 {
        self.mapper.chr_read(addr)
    }

    #[inline]
    pub fn chr_read_with_kind(&mut self, addr: u16, kind: ChrFetchKind) -> u8 {
        self.mapper.chr_read_kind(addr, kind)
    }

    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }

    pub fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
        self.mapper.ppu_nametable_read(addr, ciram)
    }

    pub fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8; 0x800]) -> bool {
        self.mapper.ppu_nametable_write(addr, val, ciram)
    }

    pub fn irq_pending(&self) -> bool {
        self.mapper.irq_pending()
    }

    pub fn notify_scanline(&mut self) {
        self.mapper.notify_scanline();
    }

    pub fn clock_cpu(&mut self) {
        self.mapper.clock_cpu();
    }

    pub fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.mapper.dump_battery_data()
    }

    pub fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.mapper.load_battery_data(bytes)
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        self.mapper.write_state(w);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mapper.read_state(r)
    }
}

#[cfg(test)]
mod tests;
