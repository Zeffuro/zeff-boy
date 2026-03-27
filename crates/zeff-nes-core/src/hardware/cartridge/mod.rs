pub mod mappers;

use anyhow::{Result, bail};
use core::fmt;

const INES_MAGIC: &[u8; 4] = b"NES\x1A";

const HEADER_SIZE: usize = 16;

const PRG_ROM_BANK_SIZE: usize = 16_384;
const CHR_ROM_BANK_SIZE: usize = 8_192;
const TRAINER_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NesMapper {
    Nrom,
    SxRom,
    UxRom,
    CnRom,
    TxRom,
    ExRom,
    AxRom,
    PxRom,
    FxRom,
    ColorDreams,
    CpRom,
    Contra100In1Function16,
    BandaiEprom24C02,
    JalecoSs8806,
    Namco163,
    Vrc4A,
    Vrc2A,
    Vrc2B,
    Vrc6A,
    Vrc4B,
    Vrc6B,
    BnRom,
    Rambo1,
    GxRom,
    AfterBurner,
    Fme7,
    CamericaCodemasters,
    Vrc3,
    PirateMmc3ChrRomChrRam2K,
    Vrc1,
    Namco109Variant,
    Nina03Or06,
    Vrc7,
    JalecoJf13,
    SenjouNoOokami,
    NesEvent,
    Nina03Or06Multicart,
    TxsRom,
    TqRom,
    BandaiEprom24C01,
    Subor166,
    Subor167,
    CrazyClimber,
    CnRomWithProtectionDiodes,
    PirateMmc3ChrRomChrRam4K,
    DxRom,
    Namco175And340,
    Action52,
    CamericaCodemastersQuattro,
    Other(u16),
}

impl NesMapper {
    pub const fn id(self) -> u16 {
        match self {
            Self::Nrom => 0,
            Self::SxRom => 1,
            Self::UxRom => 2,
            Self::CnRom => 3,
            Self::TxRom => 4,
            Self::ExRom => 5,
            Self::AxRom => 7,
            Self::PxRom => 9,
            Self::FxRom => 10,
            Self::ColorDreams => 11,
            Self::CpRom => 13,
            Self::Contra100In1Function16 => 15,
            Self::BandaiEprom24C02 => 16,
            Self::JalecoSs8806 => 18,
            Self::Namco163 => 19,
            Self::Vrc4A => 21,
            Self::Vrc2A => 22,
            Self::Vrc2B => 23,
            Self::Vrc6A => 24,
            Self::Vrc4B => 25,
            Self::Vrc6B => 26,
            Self::BnRom => 34,
            Self::Rambo1 => 64,
            Self::GxRom => 66,
            Self::AfterBurner => 68,
            Self::Fme7 => 69,
            Self::CamericaCodemasters => 71,
            Self::Vrc3 => 73,
            Self::PirateMmc3ChrRomChrRam2K => 74,
            Self::Vrc1 => 75,
            Self::Namco109Variant => 76,
            Self::Nina03Or06 => 79,
            Self::Vrc7 => 85,
            Self::JalecoJf13 => 86,
            Self::SenjouNoOokami => 94,
            Self::NesEvent => 105,
            Self::Nina03Or06Multicart => 113,
            Self::TxsRom => 118,
            Self::TqRom => 119,
            Self::BandaiEprom24C01 => 159,
            Self::Subor166 => 166,
            Self::Subor167 => 167,
            Self::CrazyClimber => 180,
            Self::CnRomWithProtectionDiodes => 185,
            Self::PirateMmc3ChrRomChrRam4K => 192,
            Self::DxRom => 206,
            Self::Namco175And340 => 210,
            Self::Action52 => 228,
            Self::CamericaCodemastersQuattro => 232,
            Self::Other(id) => id,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Nrom => "NROM",
            Self::SxRom => "SxROM / MMC1",
            Self::UxRom => "UxROM",
            Self::CnRom => "CNROM",
            Self::TxRom => "TxROM / MMC3 / MMC6",
            Self::ExRom => "ExROM / MMC5",
            Self::AxRom => "AxROM",
            Self::PxRom => "PxROM / MMC2",
            Self::FxRom => "FxROM / MMC4",
            Self::ColorDreams => "Color Dreams",
            Self::CpRom => "CPROM",
            Self::Contra100In1Function16 => "100-in-1 Contra Function 16",
            Self::BandaiEprom24C02 => "Bandai EPROM (24C02)",
            Self::JalecoSs8806 => "Jaleco SS8806",
            Self::Namco163 => "Namco 163",
            Self::Vrc4A => "VRC4a / VRC4c",
            Self::Vrc2A => "VRC2a",
            Self::Vrc2B => "VRC2b / VRC4e",
            Self::Vrc6A => "VRC6a",
            Self::Vrc4B => "VRC4b / VRC4d",
            Self::Vrc6B => "VRC6b",
            Self::BnRom => "BNROM / NINA-001",
            Self::Rambo1 => "RAMBO-1",
            Self::GxRom => "GxROM / MxROM",
            Self::AfterBurner => "After Burner",
            Self::Fme7 => "FME-7 / Sunsoft 5B",
            Self::CamericaCodemasters => "Camerica / Codemasters",
            Self::Vrc3 => "VRC3",
            Self::PirateMmc3ChrRomChrRam2K => "Pirate MMC3 derivative (2 KiB CHR RAM)",
            Self::Vrc1 => "VRC1",
            Self::Namco109Variant => "Namco 109 variant",
            Self::Nina03Or06 => "NINA-03 / NINA-06",
            Self::Vrc7 => "VRC7",
            Self::JalecoJf13 => "JALECO-JF-13",
            Self::SenjouNoOokami => "Senjou no Ookami",
            Self::NesEvent => "NES-EVENT",
            Self::Nina03Or06Multicart => "NINA-03 / NINA-06??",
            Self::TxsRom => "TxSROM",
            Self::TqRom => "TQROM",
            Self::BandaiEprom24C01 => "Bandai EPROM (24C01)",
            Self::Subor166 => "SUBOR",
            Self::Subor167 => "SUBOR",
            Self::CrazyClimber => "Crazy Climber",
            Self::CnRomWithProtectionDiodes => "CNROM with protection diodes",
            Self::PirateMmc3ChrRomChrRam4K => "Pirate MMC3 derivative (4 KiB CHR RAM)",
            Self::DxRom => "DxROM / Namco 118 / MIMIC-1",
            Self::Namco175And340 => "Namco 175 and 340",
            Self::Action52 => "Action 52",
            Self::CamericaCodemastersQuattro => "Camerica / Codemasters Quattro",
            Self::Other(_) => "Other",
        }
    }
}

impl From<u16> for NesMapper {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Nrom,
            1 => Self::SxRom,
            2 => Self::UxRom,
            3 => Self::CnRom,
            4 => Self::TxRom,
            5 => Self::ExRom,
            7 => Self::AxRom,
            9 => Self::PxRom,
            10 => Self::FxRom,
            11 => Self::ColorDreams,
            13 => Self::CpRom,
            15 => Self::Contra100In1Function16,
            16 => Self::BandaiEprom24C02,
            18 => Self::JalecoSs8806,
            19 => Self::Namco163,
            21 => Self::Vrc4A,
            22 => Self::Vrc2A,
            23 => Self::Vrc2B,
            24 => Self::Vrc6A,
            25 => Self::Vrc4B,
            26 => Self::Vrc6B,
            34 => Self::BnRom,
            64 => Self::Rambo1,
            66 => Self::GxRom,
            68 => Self::AfterBurner,
            69 => Self::Fme7,
            71 => Self::CamericaCodemasters,
            73 => Self::Vrc3,
            74 => Self::PirateMmc3ChrRomChrRam2K,
            75 => Self::Vrc1,
            76 => Self::Namco109Variant,
            79 => Self::Nina03Or06,
            85 => Self::Vrc7,
            86 => Self::JalecoJf13,
            94 => Self::SenjouNoOokami,
            105 => Self::NesEvent,
            113 => Self::Nina03Or06Multicart,
            118 => Self::TxsRom,
            119 => Self::TqRom,
            159 => Self::BandaiEprom24C01,
            166 => Self::Subor166,
            167 => Self::Subor167,
            180 => Self::CrazyClimber,
            185 => Self::CnRomWithProtectionDiodes,
            192 => Self::PirateMmc3ChrRomChrRam4K,
            206 => Self::DxRom,
            210 => Self::Namco175And340,
            228 => Self::Action52,
            232 => Self::CamericaCodemastersQuattro,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for NesMapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NesMapper::Other(id) => write!(f, "Other ({id})"),
            _ => write!(f, "{} ({})", self.name(), self.id()),
        }
    }
}

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

#[allow(clippy::large_enum_variant)]
pub enum MapperImpl {
    Nrom(mappers::Nrom),
    Mmc1(mappers::Mmc1),
    Uxrom(mappers::Uxrom),
    Cnrom(mappers::Cnrom),
    Mmc3(mappers::Mmc3),
    Mmc5(mappers::Mmc5),
    Axrom(mappers::Axrom),
    BandaiFcg16(mappers::BandaiFcg16),
    Vrc4(mappers::Vrc4),
    Fme7(mappers::Fme7),
    Action52(mappers::Action52),
}

macro_rules! dispatch_mapper {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            MapperImpl::Nrom(m) => m.$method($($arg),*),
            MapperImpl::Mmc1(m) => m.$method($($arg),*),
            MapperImpl::Uxrom(m) => m.$method($($arg),*),
            MapperImpl::Cnrom(m) => m.$method($($arg),*),
            MapperImpl::Mmc3(m) => m.$method($($arg),*),
            MapperImpl::Mmc5(m) => m.$method($($arg),*),
            MapperImpl::Axrom(m) => m.$method($($arg),*),
            MapperImpl::BandaiFcg16(m) => m.$method($($arg),*),
            MapperImpl::Vrc4(m) => m.$method($($arg),*),
            MapperImpl::Fme7(m) => m.$method($($arg),*),
            MapperImpl::Action52(m) => m.$method($($arg),*),
        }
    };
}

impl MapperImpl {
    #[inline]
    fn cpu_read(&self, addr: u16) -> u8 {
        dispatch_mapper!(self, cpu_read, addr)
    }

    #[inline]
    fn cpu_write(&mut self, addr: u16, val: u8) {
        dispatch_mapper!(self, cpu_write, addr, val)
    }

    #[inline]
    fn chr_read(&self, addr: u16) -> u8 {
        dispatch_mapper!(self, chr_read, addr)
    }

    #[inline]
    fn chr_read_kind(&self, addr: u16, kind: ChrFetchKind) -> u8 {
        dispatch_mapper!(self, chr_read_kind, addr, kind)
    }

    #[inline]
    fn chr_write(&mut self, addr: u16, val: u8) {
        dispatch_mapper!(self, chr_write, addr, val)
    }

    fn ppu_nametable_read(&self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
        dispatch_mapper!(self, ppu_nametable_read, addr, ciram)
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8; 0x800]) -> bool {
        dispatch_mapper!(self, ppu_nametable_write, addr, val, ciram)
    }

    fn mirroring(&self) -> Mirroring {
        dispatch_mapper!(self, mirroring)
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        dispatch_mapper!(self, write_state, w)
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        dispatch_mapper!(self, read_state, r)
    }

    fn irq_pending(&self) -> bool {
        dispatch_mapper!(self, irq_pending)
    }

    fn notify_scanline(&mut self) {
        dispatch_mapper!(self, notify_scanline)
    }

    fn clock_cpu(&mut self) {
        dispatch_mapper!(self, clock_cpu)
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        dispatch_mapper!(self, dump_battery_data)
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        dispatch_mapper!(self, load_battery_data, bytes)
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
            bail!("ROM declares 0 bytes of PRG ROM, which is invalid — every NES ROM needs at least one PRG bank");
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
            vec![0; CHR_ROM_BANK_SIZE]
        };

        let mapper_kind = header.mapper_kind();
        let mapper = match mapper_kind {
            NesMapper::Nrom => MapperImpl::Nrom(mappers::Nrom::new(prg_rom, chr_rom, header.mirroring)),
            NesMapper::SxRom => MapperImpl::Mmc1(mappers::Mmc1::new(prg_rom, chr_rom, header.mirroring)),
            NesMapper::UxRom => MapperImpl::Uxrom(mappers::Uxrom::new(prg_rom, chr_rom, header.mirroring)),
            NesMapper::CnRom => MapperImpl::Cnrom(mappers::Cnrom::new(prg_rom, chr_rom, header.mirroring)),
            NesMapper::TxRom => MapperImpl::Mmc3(mappers::Mmc3::new(prg_rom, chr_rom, header.mirroring)),
            NesMapper::ExRom => MapperImpl::Mmc5(mappers::Mmc5::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery || header.prg_nvram_size > 0,
            )),
            NesMapper::AxRom => MapperImpl::Axrom(mappers::Axrom::new(prg_rom, chr_rom, header.mirroring)),
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
                MapperImpl::Vrc4(mappers::Vrc4::new(prg_rom, chr_rom, header.mirroring, a0, a1))
            }
            NesMapper::Fme7 => MapperImpl::Fme7(mappers::Fme7::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery,
            )),
            NesMapper::Action52 => MapperImpl::Action52(mappers::Action52::new(prg_rom, chr_rom, header.mirroring)),
            _ => bail!("Unsupported mapper: {}. This mapper is not yet implemented", header.mapper_label()),
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
    pub fn cpu_read(&self, addr: u16) -> u8 {
        self.mapper.cpu_read(addr)
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.mapper.cpu_read(addr)
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    #[inline]
    pub fn chr_read(&self, addr: u16) -> u8 {
        self.mapper.chr_read(addr)
    }

    #[inline]
    pub fn chr_read_with_kind(&self, addr: u16, kind: ChrFetchKind) -> u8 {
        self.mapper.chr_read_kind(addr, kind)
    }

    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }

    pub fn ppu_nametable_read(&self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
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

pub(crate) trait Mapper: Send {
    fn cpu_read(&self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16) -> u8;
    fn chr_read_kind(&self, addr: u16, _kind: ChrFetchKind) -> u8 {
        self.chr_read(addr)
    }
    fn chr_write(&mut self, addr: u16, val: u8);
    fn ppu_nametable_read(&self, _addr: u16, _ciram: &[u8; 0x800]) -> Option<u8> {
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

    #[test]
    fn mapper_name_mapping_known_and_unknown() {
        assert_eq!(NesMapper::from(0).name(), "NROM");
        assert_eq!(NesMapper::from(4).name(), "TxROM / MMC3 / MMC6");
        assert_eq!(NesMapper::from(999), NesMapper::Other(999));
        assert_eq!(NesMapper::from(999).to_string(), "Other (999)");
    }

    #[test]
    fn ines_diskdude_style_junk_ignores_legacy_extension_bytes() {
        let h = make_header(0x10, 0x20, 0x51, 0x44, [0x69, 0x73, 0x6B, 0x44, 0x75, 0x64, 0x65, 0x21]);
        let hdr = RomHeader::parse(&h).unwrap();

        assert_eq!(hdr.format, RomFormat::INes);
        assert_eq!(hdr.mapper_id, 5);
        assert_eq!(hdr.prg_ram_size, 8192);
        assert_eq!(hdr.timing, TimingMode::Ntsc);
        assert_eq!(hdr.console_type, ConsoleType::Nes);
    }
}

