pub mod mappers;

use anyhow::{Result, bail};

const INES_MAGIC: &[u8; 4] = b"NES\x1A";

const HEADER_SIZE: usize = 16;

const PRG_ROM_BANK_SIZE: usize = 16_384;
const CHR_ROM_BANK_SIZE: usize = 8_192;
const TRAINER_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLower,
    SingleScreenUpper,
    FourScreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RomFormat {
    INes,
    Nes2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingMode {
    Ntsc,
    Pal,
    MultiRegion,
    Dendy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleType {
    Nes,
    VsSystem,
    Playchoice10,
    Extended(u8),
}

#[derive(Clone, Debug)]
pub struct RomHeader {
    pub format: RomFormat,

    pub prg_rom_size: usize,
    pub chr_rom_size: usize,

    pub mapper_id: u16,
    pub submapper_id: u8,

    // ── Flags ──
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub console_type: ConsoleType,

    pub prg_ram_size: usize,
    pub prg_nvram_size: usize,
    pub chr_ram_size: usize,
    pub chr_nvram_size: usize,

    pub timing: TimingMode,

    pub misc_roms: u8,
    pub default_expansion_device: u8,
}

impl RomHeader {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() < HEADER_SIZE {
            bail!(
                "ROM too small for header: need {} bytes, got {}",
                HEADER_SIZE,
                raw.len()
            );
        }
        if &raw[0..4] != INES_MAGIC {
            bail!(
                "Not a valid iNES ROM (bad magic: {:02X} {:02X} {:02X} {:02X})",
                raw[0],
                raw[1],
                raw[2],
                raw[3]
            );
        }

        let flags6 = raw[6];
        let flags7 = raw[7];
        let byte8 = raw[8];
        let byte9 = raw[9];
        let byte10 = raw[10];
        let byte11 = raw[11];
        let byte12 = raw[12];
        let byte13 = raw[13];
        let byte14 = raw[14];
        let byte15 = raw[15];

        let format = if (flags7 >> 2) & 0x03 == 0x02 {
            RomFormat::Nes2
        } else {
            RomFormat::INes
        };

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_battery = flags6 & 0x02 != 0;
        let has_trainer = flags6 & 0x04 != 0;

        let console_type = match flags7 & 0x03 {
            0 => ConsoleType::Nes,
            1 => ConsoleType::VsSystem,
            2 => ConsoleType::Playchoice10,
            _ => {
                if format == RomFormat::Nes2 {
                    ConsoleType::Extended(byte13 & 0x0F)
                } else {
                    ConsoleType::Nes
                }
            }
        };

        let mapper_lo = flags6 >> 4;
        let mapper_mid = flags7 & 0xF0;
        let (mapper_id, submapper_id) = if format == RomFormat::Nes2 {
            let mapper_hi = (byte8 as u16 & 0x0F) << 8;
            let mapper = mapper_hi | (mapper_mid as u16) | (mapper_lo as u16);
            let submapper = byte8 >> 4;
            (mapper, submapper)
        } else {
            let mapper = (mapper_mid | mapper_lo) as u16;
            (mapper, 0u8)
        };

        let (prg_rom_size, chr_rom_size) = if format == RomFormat::Nes2 {
            (
                Self::compute_nes2_rom_size(raw[4], byte9 & 0x0F, PRG_ROM_BANK_SIZE)?,
                Self::compute_nes2_rom_size(raw[5], byte9 >> 4, CHR_ROM_BANK_SIZE)?,
            )
        } else {
            let prg = raw[4] as usize * PRG_ROM_BANK_SIZE;
            let chr = raw[5] as usize * CHR_ROM_BANK_SIZE;
            (prg, chr)
        };

        let (prg_ram_size, prg_nvram_size, chr_ram_size, chr_nvram_size) =
            if format == RomFormat::Nes2 {
                (
                    Self::shift_count_to_size(byte10 & 0x0F),
                    Self::shift_count_to_size(byte10 >> 4),
                    Self::shift_count_to_size(byte11 & 0x0F),
                    Self::shift_count_to_size(byte11 >> 4),
                )
            } else {
                let prg_ram = if byte8 == 0 { 8192 } else { byte8 as usize * 8192 };
                (prg_ram, 0, 0, 0)
            };

        let timing = if format == RomFormat::Nes2 {
            match byte12 & 0x03 {
                0 => TimingMode::Ntsc,
                1 => TimingMode::Pal,
                2 => TimingMode::MultiRegion,
                3 => TimingMode::Dendy,
                _ => unreachable!(),
            }
        } else {
            if byte9 & 0x01 != 0 {
                TimingMode::Pal
            } else {
                TimingMode::Ntsc
            }
        };

        let misc_roms = if format == RomFormat::Nes2 { byte14 & 0x03 } else { 0 };
        let default_expansion_device = if format == RomFormat::Nes2 { byte15 & 0x3F } else { 0 };

        let header = Self {
            format,
            prg_rom_size,
            chr_rom_size,
            mapper_id,
            submapper_id,
            mirroring,
            has_battery,
            has_trainer,
            console_type,
            prg_ram_size,
            prg_nvram_size,
            chr_ram_size,
            chr_nvram_size,
            timing,
            misc_roms,
            default_expansion_device,
        };


        Ok(header)
    }

    fn compute_nes2_rom_size(lsb: u8, msb_nibble: u8, bank_size: usize) -> Result<usize> {
        if msb_nibble == 0x0F {
            let exponent = (lsb >> 2) & 0x3F;
            let multiplier = (lsb & 0x03) as usize * 2 + 1;
            if exponent > 30 {
                bail!("NES 2.0 ROM size exponent too large: {exponent}");
            }
            Ok((1usize << exponent) * multiplier)
        } else {
            let bank_count = ((msb_nibble as usize) << 8) | (lsb as usize);
            Ok(bank_count * bank_size)
        }
    }

    fn shift_count_to_size(shift: u8) -> usize {
        if shift == 0 { 0 } else { 64 << shift as usize }
    }
}

impl RomHeader {
    pub fn display_info(&self) {
        log::info!("--- NES ROM HEADER INFO ---");
        log::info!("Format: {:?}", self.format);
        log::info!("PRG ROM: {} KiB", self.prg_rom_size / 1024);
        if self.chr_rom_size > 0 {
            log::info!("CHR ROM: {} KiB", self.chr_rom_size / 1024);
        } else {
            log::info!("CHR ROM: 0 (uses CHR-RAM)");
        }
        log::info!("Mapper: {} (sub {})", self.mapper_id, self.submapper_id);
        log::info!("Mirroring: {:?}", self.mirroring);
        log::info!("Battery: {}", self.has_battery);
        log::info!("Trainer: {}", self.has_trainer);
        log::info!("Console: {:?}", self.console_type);
        log::info!("Timing: {:?}", self.timing);
        if self.format == RomFormat::Nes2 {
            log::info!("PRG-RAM: {} B", self.prg_ram_size);
            log::info!("PRG-NVRAM: {} B", self.prg_nvram_size);
            log::info!("CHR-RAM: {} B", self.chr_ram_size);
            log::info!("CHR-NVRAM: {} B", self.chr_nvram_size);
            log::info!("Misc ROMs: {}", self.misc_roms);
            log::info!("Expansion Device: {}", self.default_expansion_device);
        }
        log::info!("---------------------------");
    }
}

pub struct Cartridge {
    header: RomHeader,
    mapper: Box<dyn Mapper>,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> Result<Self> {
        let header = RomHeader::parse(rom_data)?;
        header.display_info();

        let trainer_offset = if header.has_trainer { TRAINER_SIZE } else { 0 };
        let prg_start = HEADER_SIZE + trainer_offset;
        let prg_size = header.prg_rom_size;
        let chr_start = prg_start + prg_size;
        let chr_size = header.chr_rom_size;

        let expected_min = chr_start + chr_size;
        if rom_data.len() < expected_min {
            bail!(
                "ROM file too small: expected at least {} bytes, got {}",
                expected_min,
                rom_data.len()
            );
        }

        let prg_rom = rom_data[prg_start..prg_start + prg_size].to_vec();
        let chr_rom = if chr_size > 0 {
            rom_data[chr_start..chr_start + chr_size].to_vec()
        } else {
            vec![0; CHR_ROM_BANK_SIZE]
        };

        let mapper: Box<dyn Mapper> = match header.mapper_id {
            0 => Box::new(mappers::Nrom::new(prg_rom, chr_rom, header.mirroring)),
            1 => Box::new(mappers::Mmc1::new(prg_rom, chr_rom, header.mirroring)),
            2 => Box::new(mappers::Uxrom::new(prg_rom, chr_rom, header.mirroring)),
            3 => Box::new(mappers::Cnrom::new(prg_rom, chr_rom, header.mirroring)),
            _ => bail!("Unsupported mapper: {}", header.mapper_id),
        };

        Ok(Self { header, mapper })
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    pub fn cpu_read(&self, addr: u16) -> u8 {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    pub fn chr_read(&self, addr: u16) -> u8 {
        self.mapper.chr_read(addr)
    }

    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }
}

pub trait Mapper: Send {
    fn cpu_read(&self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16) -> u8;
    fn chr_write(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(
        prg_banks: u8,
        chr_banks: u8,
        flags6: u8,
        flags7: u8,
        rest: [u8; 8],
    ) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(INES_MAGIC);
        h[4] = prg_banks;
        h[5] = chr_banks;
        h[6] = flags6;
        h[7] = flags7;
        h[8..16].copy_from_slice(&rest);
        h
    }

    #[test]
    fn reject_short_data() {
        let short = [0u8; 10];
        assert!(RomHeader::parse(&short).is_err());
    }

    #[test]
    fn reject_bad_magic() {
        let mut bad = [0u8; 16];
        bad[0..4].copy_from_slice(b"BAD!");
        assert!(RomHeader::parse(&bad).is_err());
    }

    #[test]
    fn ines_basic_horizontal_mirroring() {
        let h = make_header(2, 1, 0x00, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.format, RomFormat::INes);
        assert_eq!(hdr.prg_rom_size, 2 * PRG_ROM_BANK_SIZE);
        assert_eq!(hdr.chr_rom_size, 1 * CHR_ROM_BANK_SIZE);
        assert_eq!(hdr.mirroring, Mirroring::Horizontal);
        assert_eq!(hdr.mapper_id, 0);
        assert!(!hdr.has_battery);
        assert!(!hdr.has_trainer);
    }

    #[test]
    fn ines_vertical_mirroring() {
        let h = make_header(1, 1, 0x01, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn ines_four_screen() {
        let h = make_header(1, 1, 0x08, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.mirroring, Mirroring::FourScreen);
    }

    #[test]
    fn ines_battery_and_trainer() {
        let h = make_header(1, 0, 0x06, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert!(hdr.has_battery);
        assert!(hdr.has_trainer);
    }

    #[test]
    fn ines_mapper_number() {
        let h = make_header(1, 0, 0x10, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.mapper_id, 1);

        let h = make_header(1, 0, 0x40, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.mapper_id, 4);

        let h = make_header(1, 0, 0xA0, 0x10, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.mapper_id, 0x1A);
    }

    #[test]
    fn ines_prg_ram_default() {
        let h = make_header(1, 0, 0x00, 0x00, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.prg_ram_size, 8192);
    }

    #[test]
    fn ines_timing_pal() {
        let mut rest = [0u8; 8];
        rest[1] = 0x01;
        let h = make_header(1, 0, 0x00, 0x00, rest);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.timing, TimingMode::Pal);
    }

    #[test]
    fn ines_console_vs_system() {
        let h = make_header(1, 0, 0x00, 0x01, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.console_type, ConsoleType::VsSystem);
    }

    #[test]
    fn nes2_detection() {
        let h = make_header(1, 0, 0x00, 0x08, [0; 8]);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.format, RomFormat::Nes2);
    }

    #[test]
    fn nes2_mapper_and_submapper() {
        let mut rest = [0u8; 8];
        rest[0] = 0x31;
        let h = make_header(1, 0, 0x00, 0x08, rest);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.format, RomFormat::Nes2);
        assert_eq!(hdr.mapper_id, 256);
        assert_eq!(hdr.submapper_id, 3);
    }

    #[test]
    fn nes2_prg_chr_rom_size_simple() {
        let mut rest = [0u8; 8];
        rest[1] = 0x00;
        let h = make_header(16, 2, 0x00, 0x08, rest);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.prg_rom_size, 16 * PRG_ROM_BANK_SIZE);
        assert_eq!(hdr.chr_rom_size, 2 * CHR_ROM_BANK_SIZE);
    }

    #[test]
    fn nes2_ram_sizes() {
        let mut rest = [0u8; 8];
        rest[2] = 0x07;
        rest[3] = 0x07;
        let h = make_header(1, 0, 0x00, 0x08, rest);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.prg_ram_size, 8192);
        assert_eq!(hdr.prg_nvram_size, 0);
        assert_eq!(hdr.chr_ram_size, 8192);
        assert_eq!(hdr.chr_nvram_size, 0);
    }

    #[test]
    fn nes2_timing_modes() {
        for (val, expected) in [
            (0, TimingMode::Ntsc),
            (1, TimingMode::Pal),
            (2, TimingMode::MultiRegion),
            (3, TimingMode::Dendy),
        ] {
            let mut rest = [0u8; 8];
            rest[4] = val;
            let h = make_header(1, 0, 0x00, 0x08, rest);
            let hdr = RomHeader::parse(&h).unwrap();
            assert_eq!(hdr.timing, expected);
        }
    }

    #[test]
    fn shift_count_to_size_values() {
        assert_eq!(RomHeader::shift_count_to_size(0), 0);
        assert_eq!(RomHeader::shift_count_to_size(1), 128);
        assert_eq!(RomHeader::shift_count_to_size(7), 8192);
        assert_eq!(RomHeader::shift_count_to_size(10), 65536);
    }
}

