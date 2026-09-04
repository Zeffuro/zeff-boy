use anyhow::{Context, bail};
use zeff_emu_common::save_ram::SaveRamKind;

use super::constants::{
    CODEMASTERS_CARTRIDGE_RAM_SIZE, COPIER_HEADER_SIZE, ROM_BANK_SIZE, ROM_PAGE_8K_SIZE,
    SMS_CARTRIDGE_RAM_SIZE,
};

mod compat;
mod header;

#[cfg(test)]
mod tests;

pub use header::{
    CodemastersHeader, HeaderLocation, Region, RomHeader, Sega8MapperKind, Sega8System, SystemHint,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameGearStandardMapperRam {
    Absent,
    BatteryBacked8KiB,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameGearStandardMapperRamIdentity {
    identity: GameGearCartridgeIdentity,
    ram: GameGearStandardMapperRam,
}

impl GameGearStandardMapperRamIdentity {
    pub(crate) fn matches(self, identity: GameGearCartridgeIdentity) -> bool {
        self.identity == identity
    }

    pub fn ram(self) -> GameGearStandardMapperRam {
        self.ram
    }
}

pub fn game_gear_standard_mapper_ram_identity_from_catalog_entry(
    identity: GameGearCartridgeIdentity,
    ram: GameGearStandardMapperRam,
) -> GameGearStandardMapperRamIdentity {
    GameGearStandardMapperRamIdentity { identity, ram }
}

impl GameGearStandardMapperRam {
    fn save_ram_kind(self) -> SaveRamKind {
        match self {
            Self::Absent => SaveRamKind::none(),
            Self::BatteryBacked8KiB => SaveRamKind::known_battery_backed(8 * 1024),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameGearCartridgeIdentity {
    pub sha256: [u8; 32],
    pub source_len: usize,
}

#[derive(Clone, Copy)]
struct GameGearBoardCatalogEntry {
    identity: GameGearCartridgeIdentity,
    ram: GameGearStandardMapperRam,
}

const GAME_GEAR_BOARD_CATALOG: &[GameGearBoardCatalogEntry] = &[];

pub fn game_gear_standard_mapper_ram_for_identity(
    identity: GameGearCartridgeIdentity,
) -> Option<GameGearStandardMapperRamIdentity> {
    game_gear_standard_mapper_ram_in_catalog(identity, GAME_GEAR_BOARD_CATALOG)
}

fn game_gear_standard_mapper_ram_in_catalog(
    identity: GameGearCartridgeIdentity,
    catalog: &[GameGearBoardCatalogEntry],
) -> Option<GameGearStandardMapperRamIdentity> {
    catalog
        .iter()
        .find(|entry| entry.identity == identity)
        .map(|entry| GameGearStandardMapperRamIdentity {
            identity: entry.identity,
            ram: entry.ram,
        })
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
    game_gear_standard_mapper_ram: Option<GameGearStandardMapperRam>,
    normalized_crc32: u32,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::load_with_hint(rom_data, SystemHint::Auto)
    }

    pub fn load_with_path_hint(rom_data: &[u8], path: &std::path::Path) -> anyhow::Result<Self> {
        let hint = SystemHint::from_path(path).unwrap_or(SystemHint::Auto);
        Self::load_with_hint(rom_data, hint)
    }

    pub fn load_with_hint(rom_data: &[u8], hint: SystemHint) -> anyhow::Result<Self> {
        Self::load_with_hint_and_mapper_kind(rom_data, hint, None)
    }

    pub fn load_with_hint_and_mapper_kind(
        rom_data: &[u8],
        hint: SystemHint,
        mapper_kind_override: Option<Sega8MapperKind>,
    ) -> anyhow::Result<Self> {
        Self::load_with_hint_mapper_and_game_gear_ram(rom_data, hint, mapper_kind_override, None)
    }

    pub(crate) fn load_with_hint_mapper_and_game_gear_ram(
        rom_data: &[u8],
        hint: SystemHint,
        mapper_kind_override: Option<Sega8MapperKind>,
        game_gear_standard_mapper_ram: Option<GameGearStandardMapperRam>,
    ) -> anyhow::Result<Self> {
        if rom_data.is_empty() {
            bail!("Sega 8-bit ROM is empty");
        }

        let raw_len = rom_data.len();
        let (rom, copier_header_stripped) = normalized_rom_data(rom_data)?;
        let normalized_crc32 = crc32fast::hash(&rom);
        let header = header::find_header(&rom);
        let codemasters_header = header::find_codemasters_header(&rom);
        let system = hint.resolve(header.as_ref());
        let mapper_kind = mapper_kind_override
            .or_else(|| compat::mapper_kind_for_crc32(normalized_crc32))
            .unwrap_or_else(|| detect_mapper_kind(codemasters_header.as_ref()));
        if game_gear_standard_mapper_ram.is_some()
            && (system != Sega8System::GameGear || mapper_kind != Sega8MapperKind::Sega)
        {
            bail!("Game Gear standard-mapper RAM identity requires a Game Gear Sega mapper");
        }

        Ok(Self {
            rom,
            raw_len,
            copier_header_stripped,
            header,
            codemasters_header,
            system,
            mapper_kind,
            game_gear_standard_mapper_ram,
            normalized_crc32,
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

    pub fn normalized_crc32(&self) -> u32 {
        self.normalized_crc32
    }

    pub(crate) fn uses_sg_type_b_ram_extension(&self) -> bool {
        self.system == Sega8System::Sg1000
            && compat::uses_sg_type_b_ram_extension(self.normalized_crc32)
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        if self.system == Sega8System::Sg1000 {
            return SaveRamKind::none();
        }
        match self.mapper_kind {
            Sega8MapperKind::Sega => self.game_gear_standard_mapper_ram.map_or_else(
                || SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE),
                GameGearStandardMapperRam::save_ram_kind,
            ),
            Sega8MapperKind::Codemasters => {
                SaveRamKind::known_volatile(CODEMASTERS_CARTRIDGE_RAM_SIZE)
            }
            Sega8MapperKind::Korean
            | Sega8MapperKind::Msx
            | Sega8MapperKind::Nemesis
            | Sega8MapperKind::Janggun => SaveRamKind::none(),
        }
    }

    pub fn rom_bank_count(&self) -> usize {
        self.rom.len().div_ceil(ROM_BANK_SIZE)
    }

    pub fn rom_page_8k_count(&self) -> usize {
        self.rom.len().div_ceil(ROM_PAGE_8K_SIZE)
    }

    pub fn read_flat(&self, addr: usize) -> u8 {
        self.rom[addr % self.rom.len()]
    }

    pub fn read_bank(&self, bank: u8, offset: u16) -> u8 {
        let addr = usize::from(bank) * ROM_BANK_SIZE + usize::from(offset);
        self.read_flat(addr)
    }

    pub fn read_page_8k(&self, page: u8, offset: u16) -> u8 {
        let addr = usize::from(page) * ROM_PAGE_8K_SIZE + usize::from(offset);
        self.read_flat(addr)
    }
}

fn detect_mapper_kind(codemasters_header: Option<&CodemastersHeader>) -> Sega8MapperKind {
    if codemasters_header.is_some() {
        Sega8MapperKind::Codemasters
    } else {
        Sega8MapperKind::Sega
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
