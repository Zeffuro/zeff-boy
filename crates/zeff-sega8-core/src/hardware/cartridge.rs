use std::path::Path;

use anyhow::{Context, bail};
use zeff_emu_common::save_ram::SaveRamKind;

use super::constants::{
    CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, COPIER_HEADER_SIZE, ROM_BANK_SIZE,
    SEGA_HEADER_8K_OFFSET, SEGA_HEADER_16K_OFFSET, SEGA_HEADER_32K_OFFSET, SEGA_HEADER_MAGIC,
    SEGA_HEADER_SIZE, SMS_CARTRIDGE_RAM_SIZE,
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
const CODEMASTERS_HEADER_BANK_COUNT: usize = 0x00;
const CODEMASTERS_HEADER_DAY: usize = 0x01;
const CODEMASTERS_HEADER_MONTH: usize = 0x02;
const CODEMASTERS_HEADER_YEAR: usize = 0x03;
const CODEMASTERS_HEADER_HOUR: usize = 0x04;
const CODEMASTERS_HEADER_MINUTE: usize = 0x05;
const CODEMASTERS_HEADER_CHECKSUM_LO: usize = 0x06;
const CODEMASTERS_HEADER_CHECKSUM_HI: usize = 0x07;
const CODEMASTERS_HEADER_COMPLEMENT_LO: usize = 0x08;
const CODEMASTERS_HEADER_COMPLEMENT_HI: usize = 0x09;
const CODEMASTERS_HEADER_ZERO_PADDING_START: usize = 0x0A;

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
}

impl Sega8MapperKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sega => "sega",
            Self::Codemasters => "codemasters",
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

    fn resolve(self, header: Option<&RomHeader>) -> Sega8System {
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

#[derive(Clone, Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    raw_len: usize,
    copier_header_stripped: bool,
    header: Option<RomHeader>,
    codemasters_header: Option<CodemastersHeader>,
    system: Sega8System,
    mapper_kind: Sega8MapperKind,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::load_with_hint(rom_data, SystemHint::Auto)
    }

    pub fn load_with_path_hint(rom_data: &[u8], path: &Path) -> anyhow::Result<Self> {
        let hint = SystemHint::from_path(path).unwrap_or(SystemHint::Auto);
        Self::load_with_hint(rom_data, hint)
    }

    pub fn load_with_hint(rom_data: &[u8], hint: SystemHint) -> anyhow::Result<Self> {
        if rom_data.is_empty() {
            bail!("Sega 8-bit ROM is empty");
        }

        let raw_len = rom_data.len();
        let (rom, copier_header_stripped) = normalized_rom_data(rom_data)?;
        let header = find_header(&rom);
        let codemasters_header = find_codemasters_header(&rom);
        let system = hint.resolve(header.as_ref());
        let mapper_kind = if codemasters_header.is_some() {
            Sega8MapperKind::Codemasters
        } else {
            Sega8MapperKind::Sega
        };

        Ok(Self {
            rom,
            raw_len,
            copier_header_stripped,
            header,
            codemasters_header,
            system,
            mapper_kind,
        })
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub fn raw_len(&self) -> usize {
        self.raw_len
    }

    pub fn normalized_len(&self) -> usize {
        self.rom.len()
    }

    pub fn copier_header_stripped(&self) -> bool {
        self.copier_header_stripped
    }

    pub fn header(&self) -> Option<RomHeader> {
        self.header
    }

    pub fn codemasters_header(&self) -> Option<CodemastersHeader> {
        self.codemasters_header
    }

    pub fn system(&self) -> Sega8System {
        self.system
    }

    pub fn mapper_kind(&self) -> Sega8MapperKind {
        self.mapper_kind
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        match self.mapper_kind {
            Sega8MapperKind::Sega => SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE),
            Sega8MapperKind::Codemasters => SaveRamKind::none(),
        }
    }

    pub fn rom_bank_count(&self) -> usize {
        self.rom.len().div_ceil(ROM_BANK_SIZE)
    }

    pub fn read_flat(&self, addr: usize) -> u8 {
        self.rom[addr % self.rom.len()]
    }

    pub fn read_bank(&self, bank: u8, offset: u16) -> u8 {
        let addr = usize::from(bank) * ROM_BANK_SIZE + usize::from(offset);
        self.read_flat(addr)
    }
}

fn normalized_rom_data(rom_data: &[u8]) -> anyhow::Result<(Vec<u8>, bool)> {
    if should_strip_copier_header(rom_data) {
        let rom = rom_data
            .get(COPIER_HEADER_SIZE..)
            .context("copier header strip exceeded ROM length")?
            .to_vec();
        if rom.is_empty() {
            bail!("Sega 8-bit ROM has no data after copier header");
        }
        Ok((rom, true))
    } else {
        Ok((rom_data.to_vec(), false))
    }
}

fn should_strip_copier_header(rom_data: &[u8]) -> bool {
    rom_data.len() > COPIER_HEADER_SIZE && rom_data.len() % ROM_BANK_SIZE == COPIER_HEADER_SIZE
}

fn find_header(rom: &[u8]) -> Option<RomHeader> {
    [
        HeaderLocation::Offset0x7ff0,
        HeaderLocation::Offset0x3ff0,
        HeaderLocation::Offset0x1ff0,
    ]
    .into_iter()
    .find_map(|location| RomHeader::parse_at(rom, location))
}

fn find_codemasters_header(rom: &[u8]) -> Option<CodemastersHeader> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_header(location: HeaderLocation, region_size: u8) -> Vec<u8> {
        let mut rom = vec![0xFF; location.offset() + SEGA_HEADER_SIZE];
        let offset = location.offset();
        rom[offset..offset + SEGA_HEADER_MAGIC.len()].copy_from_slice(SEGA_HEADER_MAGIC);
        rom[offset + 0x0A..offset + 0x0C].copy_from_slice(&0x1234u16.to_le_bytes());
        rom[offset + 0x0C] = 0x42;
        rom[offset + 0x0D] = 0x31;
        rom[offset + 0x0E] = 0xA5;
        rom[offset + 0x0F] = region_size;
        rom
    }

    fn rom_with_codemasters_header() -> Vec<u8> {
        let mut rom = vec![0xFF; CODEMASTERS_HEADER_OFFSET + CODEMASTERS_HEADER_SIZE];
        let offset = CODEMASTERS_HEADER_OFFSET;
        rom[offset + CODEMASTERS_HEADER_BANK_COUNT] = 2;
        rom[offset + CODEMASTERS_HEADER_DAY] = 0x31;
        rom[offset + CODEMASTERS_HEADER_MONTH] = 0x08;
        rom[offset + CODEMASTERS_HEADER_YEAR] = 0x93;
        rom[offset + CODEMASTERS_HEADER_HOUR] = 0x10;
        rom[offset + CODEMASTERS_HEADER_MINUTE] = 0x59;
        rom[offset + CODEMASTERS_HEADER_CHECKSUM_LO..offset + CODEMASTERS_HEADER_CHECKSUM_HI + 1]
            .copy_from_slice(&0x1234u16.to_le_bytes());
        rom[offset + CODEMASTERS_HEADER_COMPLEMENT_LO
            ..offset + CODEMASTERS_HEADER_COMPLEMENT_HI + 1]
            .copy_from_slice(&0xEDCCu16.to_le_bytes());
        rom[offset + CODEMASTERS_HEADER_ZERO_PADDING_START..offset + CODEMASTERS_HEADER_SIZE]
            .fill(0);
        rom
    }

    #[test]
    fn parses_sms_header_fields() {
        let cart = Cartridge::load(&rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C))
            .expect("SMS header should parse");
        let header = cart.header().expect("header should be present");

        assert_eq!(header.location, HeaderLocation::Offset0x7ff0);
        assert_eq!(header.checksum, 0x1234);
        assert_eq!(header.product_code_bcd, [0x42, 0x31, 0x0A]);
        assert_eq!(header.version, 0x05);
        assert_eq!(header.region, Region::SmsExport);
        assert_eq!(header.rom_size_code, 0x0C);
        assert_eq!(cart.system(), Sega8System::MasterSystem);
    }

    #[test]
    fn auto_detects_game_gear_from_header_region() {
        let cart = Cartridge::load(&rom_with_header(HeaderLocation::Offset0x3ff0, 0x7A))
            .expect("GG header should parse");

        assert_eq!(cart.system(), Sega8System::GameGear);
        assert_eq!(cart.header().unwrap().region, Region::GameGearInternational);
    }

    #[test]
    fn explicit_hint_handles_sg1000_roms_without_header() {
        let cart = Cartridge::load_with_hint(&[0x00, 0x01, 0x02], SystemHint::Sg1000)
            .expect("headerless SG-1000 ROM should load with hint");

        assert_eq!(cart.system(), Sega8System::Sg1000);
        assert_eq!(cart.header(), None);
    }

    #[test]
    fn detects_codemasters_header_and_mapper_kind() {
        let cart =
            Cartridge::load_with_hint(&rom_with_codemasters_header(), SystemHint::MasterSystem)
                .expect("Codemasters-style ROM should load");

        let header = cart
            .codemasters_header()
            .expect("Codemasters header should parse");
        assert_eq!(cart.mapper_kind(), Sega8MapperKind::Codemasters);
        assert_eq!(header.checksum_bank_count, 2);
        assert_eq!(header.day_bcd, 0x31);
        assert_eq!(header.month_bcd, 0x08);
        assert_eq!(header.checksum, 0x1234);
        assert_eq!(header.checksum_complement, 0xEDCC);
    }

    #[test]
    fn classifies_standard_mapper_ram_as_unknown_persistence() {
        let cart = Cartridge::load_with_hint(
            &rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C),
            SystemHint::MasterSystem,
        )
        .expect("ROM should load");

        assert_eq!(
            cart.save_ram_kind(),
            SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE)
        );
        assert!(!cart.save_ram_kind().is_battery_backed());
    }

    #[test]
    fn codemasters_mapper_does_not_expose_standard_sega_save_ram() {
        let cart =
            Cartridge::load_with_hint(&rom_with_codemasters_header(), SystemHint::MasterSystem)
                .expect("Codemasters-style ROM should load");

        assert_eq!(cart.save_ram_kind(), SaveRamKind::none());
    }

    #[test]
    fn rejects_invalid_codemasters_header_padding() {
        let mut rom = rom_with_codemasters_header();
        rom[CODEMASTERS_HEADER_OFFSET + CODEMASTERS_HEADER_ZERO_PADDING_START] = 1;

        let cart =
            Cartridge::load_with_hint(&rom, SystemHint::MasterSystem).expect("ROM should load");

        assert_eq!(cart.codemasters_header(), None);
        assert_eq!(cart.mapper_kind(), Sega8MapperKind::Sega);
    }

    #[test]
    fn strips_512_byte_copier_header_before_header_scan() {
        let rom = rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C);
        let mut with_copier_header = vec![0x00; COPIER_HEADER_SIZE];
        with_copier_header.extend_from_slice(&rom);

        let cart = Cartridge::load(&with_copier_header).expect("ROM should load");

        assert!(cart.copier_header_stripped());
        assert_eq!(cart.raw_len(), with_copier_header.len());
        assert_eq!(cart.normalized_len(), rom.len());
        assert_eq!(
            cart.header().unwrap().location,
            HeaderLocation::Offset0x7ff0
        );
    }

    #[test]
    fn bank_reads_wrap_to_available_rom_data() {
        let cart = Cartridge::load_with_hint(&[0x10, 0x20, 0x30], SystemHint::MasterSystem)
            .expect("tiny ROM should load");

        assert_eq!(cart.rom_bank_count(), 1);
        assert_eq!(cart.read_bank(0, 0), 0x10);
        assert_eq!(cart.read_bank(0, 3), 0x10);
        assert_eq!(cart.read_bank(2, 0), 0x30);
    }

    #[test]
    fn system_hint_is_inferred_from_rom_extension() {
        assert_eq!(
            SystemHint::from_path(std::path::Path::new("game.SMS")),
            Some(SystemHint::MasterSystem)
        );
        assert_eq!(
            SystemHint::from_path(std::path::Path::new("game.gg")),
            Some(SystemHint::GameGear)
        );
        assert_eq!(
            SystemHint::from_path(std::path::Path::new("game.sg")),
            Some(SystemHint::Sg1000)
        );
        assert_eq!(
            SystemHint::from_path(std::path::Path::new("game.gbc")),
            None
        );
    }
}
