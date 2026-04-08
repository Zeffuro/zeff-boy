mod mapper_id;

pub use mapper_id::NesMapper;

use anyhow::{Result, bail};

const HEADER_SIZE: usize = 16;
pub(crate) const INES_MAGIC: &[u8; 4] = b"NES\x1A";

pub(crate) const PRG_ROM_BANK_SIZE: usize = 16_384;
pub(crate) const CHR_ROM_BANK_SIZE: usize = 8_192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLower,
    SingleScreenUpper,
    FourScreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChrFetchKind {
    Background,
    Sprite,
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
                "File too small for iNES header: need at least {} bytes, got {}. The file may be corrupt or not a valid NES ROM",
                HEADER_SIZE,
                raw.len()
            );
        }
        if &raw[0..4] != INES_MAGIC {
            bail!(
                "Not a valid iNES/NES 2.0 ROM (expected magic bytes 'NES\\x1A', got {:02X} {:02X} {:02X} {:02X}). The file may be corrupt or not a NES ROM",
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
        let has_ines_junk_tail =
            format == RomFormat::INes && raw[12..16].iter().any(|&byte| byte != 0);

        if has_ines_junk_tail {
            log::warn!(
                "iNES header tail bytes are non-zero; ignoring bytes 7-15 legacy extensions"
            );
        }

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_battery = flags6 & 0x02 != 0;
        let has_trainer = flags6 & 0x04 != 0;

        let console_type = if has_ines_junk_tail {
            ConsoleType::Nes
        } else {
            match flags7 & 0x03 {
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
            }
        };

        let mapper_lo = flags6 >> 4;
        let mapper_mid = if has_ines_junk_tail { 0 } else { flags7 & 0xF0 };
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
                let prg_ram = if has_ines_junk_tail || byte8 == 0 {
                    8192
                } else {
                    byte8 as usize * 8192
                };
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
        } else if has_ines_junk_tail {
            TimingMode::Ntsc
        } else {
            if byte9 & 0x01 != 0 {
                TimingMode::Pal
            } else {
                TimingMode::Ntsc
            }
        };

        let misc_roms = if format == RomFormat::Nes2 {
            byte14 & 0x03
        } else {
            0
        };
        let default_expansion_device = if format == RomFormat::Nes2 {
            byte15 & 0x3F
        } else {
            0
        };

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

    pub fn mapper_kind(&self) -> NesMapper {
        NesMapper::from(self.mapper_id)
    }

    pub fn mapper_label(&self) -> String {
        let mapper = self.mapper_kind();
        if self.submapper_id == 0 {
            mapper.to_string()
        } else {
            format!("{} (sub {})", mapper, self.submapper_id)
        }
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
        log::info!("Mapper: {}", self.mapper_label());
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

#[cfg(test)]
mod tests;
