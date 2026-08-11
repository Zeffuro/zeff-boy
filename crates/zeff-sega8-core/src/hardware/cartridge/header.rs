use std::path::Path;

use crate::hardware::constants::{
    CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, SEGA_HEADER_8K_OFFSET,
    SEGA_HEADER_16K_OFFSET, SEGA_HEADER_32K_OFFSET, SEGA_HEADER_MAGIC, SEGA_HEADER_SIZE,
};

const SMS_EXTENSION: &str = "sms";
const GAME_GEAR_EXTENSION: &str = "gg";
const SG1000_ROM_EXTENSION: &str = "sg";
const SG1000_SC3000_EXTENSION: &str = "sc";
const SEGA_HEADER_CHECKSUM_LO: usize = 0x0A;
const SEGA_HEADER_CHECKSUM_HI: usize = 0x0B;
const SEGA_HEADER_PRODUCT_CODE_0: usize = 0x0C;
const SEGA_HEADER_PRODUCT_CODE_1: usize = 0x0D;
const SEGA_HEADER_VERSION_PRODUCT_2: usize = 0x0E;
const SEGA_HEADER_REGION_SIZE: usize = 0x0F;
const SEGA_HEADER_PRODUCT_CODE_2_SHIFT: u8 = 4;
const SEGA_HEADER_VERSION_MASK: u8 = 0x0F;
const SEGA_HEADER_REGION_SHIFT: u8 = 4;
const SEGA_HEADER_ROM_SIZE_MASK: u8 = 0x0F;
const REGION_SMS_JAPAN: u8 = 0x3;
const REGION_SMS_EXPORT: u8 = 0x4;
const REGION_GAME_GEAR_JAPAN: u8 = 0x5;
const REGION_GAME_GEAR_EXPORT: u8 = 0x6;
const REGION_GAME_GEAR_INTERNATIONAL: u8 = 0x7;
pub(super) const CODEMASTERS_HEADER_BANK_COUNT: usize = 0x00;
pub(super) const CODEMASTERS_HEADER_DAY: usize = 0x01;
pub(super) const CODEMASTERS_HEADER_MONTH: usize = 0x02;
pub(super) const CODEMASTERS_HEADER_YEAR: usize = 0x03;
pub(super) const CODEMASTERS_HEADER_HOUR: usize = 0x04;
pub(super) const CODEMASTERS_HEADER_MINUTE: usize = 0x05;
pub(super) const CODEMASTERS_HEADER_CHECKSUM_LO: usize = 0x06;
pub(super) const CODEMASTERS_HEADER_CHECKSUM_HI: usize = 0x07;
pub(super) const CODEMASTERS_HEADER_COMPLEMENT_LO: usize = 0x08;
pub(super) const CODEMASTERS_HEADER_COMPLEMENT_HI: usize = 0x09;
pub(super) const CODEMASTERS_HEADER_ZERO_PADDING_START: usize = 0x0A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sega8System {
    MasterSystem,
    GameGear,
    Sg1000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sega8MapperKind {
    #[default]
    Sega,
    Codemasters,
    Korean,
    Msx,
    Nemesis,
    Janggun,
}

impl Sega8MapperKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sega => "sega",
            Self::Codemasters => "codemasters",
            Self::Korean => "korean",
            Self::Msx => "msx",
            Self::Nemesis => "nemesis",
            Self::Janggun => "janggun",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let text = path.file_name()?.to_str()?;
        Self::from_explicit_tag(text)
    }

    pub fn from_explicit_tag(text: &str) -> Option<Self> {
        let normalized = text
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>();
        let tokens = normalized.split_whitespace().collect::<Vec<_>>();

        for window in tokens.windows(2) {
            if window[0] == "mapper"
                && let Some(kind) = Self::from_label(window[1])
            {
                return Some(kind);
            }
            if window[1] == "mapper"
                && let Some(kind) = Self::from_label(window[0])
            {
                return Some(kind);
            }
        }

        None
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "sega" => Some(Self::Sega),
            "codemasters" => Some(Self::Codemasters),
            "korean" => Some(Self::Korean),
            "msx" => Some(Self::Msx),
            "nemesis" => Some(Self::Nemesis),
            "janggun" => Some(Self::Janggun),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SystemHint {
    #[default]
    Auto,
    MasterSystem,
    GameGear,
    Sg1000,
}

impl SystemHint {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        Self::from_extension(ext)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            SMS_EXTENSION => Some(Self::MasterSystem),
            GAME_GEAR_EXTENSION => Some(Self::GameGear),
            SG1000_ROM_EXTENSION | SG1000_SC3000_EXTENSION => Some(Self::Sg1000),
            _ => None,
        }
    }

    pub(super) fn resolve(self, header: Option<&RomHeader>) -> Sega8System {
        match self {
            Self::MasterSystem => Sega8System::MasterSystem,
            Self::GameGear => Sega8System::GameGear,
            Self::Sg1000 => Sega8System::Sg1000,
            Self::Auto => header
                .and_then(|header| header.region.implied_system())
                .unwrap_or(Sega8System::MasterSystem),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderLocation {
    Offset0x1ff0,
    Offset0x3ff0,
    Offset0x7ff0,
}

impl HeaderLocation {
    pub fn offset(self) -> usize {
        match self {
            Self::Offset0x1ff0 => SEGA_HEADER_8K_OFFSET,
            Self::Offset0x3ff0 => SEGA_HEADER_16K_OFFSET,
            Self::Offset0x7ff0 => SEGA_HEADER_32K_OFFSET,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    SmsJapan,
    SmsExport,
    GameGearJapan,
    GameGearExport,
    GameGearInternational,
    Unknown(u8),
}

impl Region {
    pub fn from_code(code: u8) -> Self {
        match code {
            REGION_SMS_JAPAN => Self::SmsJapan,
            REGION_SMS_EXPORT => Self::SmsExport,
            REGION_GAME_GEAR_JAPAN => Self::GameGearJapan,
            REGION_GAME_GEAR_EXPORT => Self::GameGearExport,
            REGION_GAME_GEAR_INTERNATIONAL => Self::GameGearInternational,
            other => Self::Unknown(other),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::SmsJapan => REGION_SMS_JAPAN,
            Self::SmsExport => REGION_SMS_EXPORT,
            Self::GameGearJapan => REGION_GAME_GEAR_JAPAN,
            Self::GameGearExport => REGION_GAME_GEAR_EXPORT,
            Self::GameGearInternational => REGION_GAME_GEAR_INTERNATIONAL,
            Self::Unknown(code) => code,
        }
    }

    pub fn implied_system(self) -> Option<Sega8System> {
        match self {
            Self::SmsJapan | Self::SmsExport => Some(Sega8System::MasterSystem),
            Self::GameGearJapan | Self::GameGearExport | Self::GameGearInternational => {
                Some(Sega8System::GameGear)
            }
            Self::Unknown(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomHeader {
    pub location: HeaderLocation,
    pub checksum: u16,
    pub product_code_bcd: [u8; 3],
    pub version: u8,
    pub region: Region,
    pub rom_size_code: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodemastersHeader {
    pub checksum_bank_count: u8,
    pub day_bcd: u8,
    pub month_bcd: u8,
    pub year_bcd: u8,
    pub hour_bcd: u8,
    pub minute_bcd: u8,
    pub checksum: u16,
    pub checksum_complement: u16,
}

impl RomHeader {
    fn parse_at(rom: &[u8], location: HeaderLocation) -> Option<Self> {
        let offset = location.offset();
        let raw = rom.get(offset..offset + SEGA_HEADER_SIZE)?;
        if raw.get(..SEGA_HEADER_MAGIC.len())? != SEGA_HEADER_MAGIC {
            return None;
        }

        let checksum =
            u16::from_le_bytes([raw[SEGA_HEADER_CHECKSUM_LO], raw[SEGA_HEADER_CHECKSUM_HI]]);
        let version_product = raw[SEGA_HEADER_VERSION_PRODUCT_2];
        let product_code_bcd = [
            raw[SEGA_HEADER_PRODUCT_CODE_0],
            raw[SEGA_HEADER_PRODUCT_CODE_1],
            version_product >> SEGA_HEADER_PRODUCT_CODE_2_SHIFT,
        ];
        let version = version_product & SEGA_HEADER_VERSION_MASK;
        let region_size = raw[SEGA_HEADER_REGION_SIZE];
        let region = Region::from_code(region_size >> SEGA_HEADER_REGION_SHIFT);
        let rom_size_code = region_size & SEGA_HEADER_ROM_SIZE_MASK;

        Some(Self {
            location,
            checksum,
            product_code_bcd,
            version,
            region,
            rom_size_code,
        })
    }
}

pub(super) fn find_header(rom: &[u8]) -> Option<RomHeader> {
    [
        HeaderLocation::Offset0x7ff0,
        HeaderLocation::Offset0x3ff0,
        HeaderLocation::Offset0x1ff0,
    ]
    .into_iter()
    .find_map(|location| RomHeader::parse_at(rom, location))
}

pub(super) fn find_codemasters_header(rom: &[u8]) -> Option<CodemastersHeader> {
    let raw =
        rom.get(CODEMASTERS_HEADER_OFFSET..CODEMASTERS_HEADER_OFFSET + CODEMASTERS_HEADER_SIZE)?;
    if raw[CODEMASTERS_HEADER_BANK_COUNT] == 0 {
        return None;
    }
    if !valid_bcd_range(raw[CODEMASTERS_HEADER_DAY], 1, 31)
        || !valid_bcd_range(raw[CODEMASTERS_HEADER_MONTH], 1, 12)
        || bcd_to_decimal(raw[CODEMASTERS_HEADER_YEAR]).is_none()
        || !valid_bcd_range(raw[CODEMASTERS_HEADER_HOUR], 0, 23)
        || !valid_bcd_range(raw[CODEMASTERS_HEADER_MINUTE], 0, 59)
    {
        return None;
    }
    if raw[CODEMASTERS_HEADER_ZERO_PADDING_START..]
        .iter()
        .any(|&byte| byte != 0)
    {
        return None;
    }

    let checksum = u16::from_le_bytes([
        raw[CODEMASTERS_HEADER_CHECKSUM_LO],
        raw[CODEMASTERS_HEADER_CHECKSUM_HI],
    ]);
    let checksum_complement = u16::from_le_bytes([
        raw[CODEMASTERS_HEADER_COMPLEMENT_LO],
        raw[CODEMASTERS_HEADER_COMPLEMENT_HI],
    ]);
    if checksum.wrapping_add(checksum_complement) != 0 {
        return None;
    }

    Some(CodemastersHeader {
        checksum_bank_count: raw[CODEMASTERS_HEADER_BANK_COUNT],
        day_bcd: raw[CODEMASTERS_HEADER_DAY],
        month_bcd: raw[CODEMASTERS_HEADER_MONTH],
        year_bcd: raw[CODEMASTERS_HEADER_YEAR],
        hour_bcd: raw[CODEMASTERS_HEADER_HOUR],
        minute_bcd: raw[CODEMASTERS_HEADER_MINUTE],
        checksum,
        checksum_complement,
    })
}

fn valid_bcd_range(byte: u8, min: u8, max: u8) -> bool {
    bcd_to_decimal(byte).is_some_and(|value| (min..=max).contains(&value))
}

fn bcd_to_decimal(byte: u8) -> Option<u8> {
    let tens = byte >> 4;
    let ones = byte & 0x0F;
    if tens < 10 && ones < 10 {
        Some(tens * 10 + ones)
    } else {
        None
    }
}
