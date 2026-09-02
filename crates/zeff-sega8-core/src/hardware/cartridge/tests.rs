use super::header::{
    CODEMASTERS_HEADER_BANK_COUNT, CODEMASTERS_HEADER_CHECKSUM_HI, CODEMASTERS_HEADER_CHECKSUM_LO,
    CODEMASTERS_HEADER_COMPLEMENT_HI, CODEMASTERS_HEADER_COMPLEMENT_LO, CODEMASTERS_HEADER_DAY,
    CODEMASTERS_HEADER_HOUR, CODEMASTERS_HEADER_MINUTE, CODEMASTERS_HEADER_MONTH,
    CODEMASTERS_HEADER_YEAR, CODEMASTERS_HEADER_ZERO_PADDING_START,
};
use super::*;
use crate::hardware::constants::{
    CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, COPIER_HEADER_SIZE, SEGA_HEADER_MAGIC,
    SEGA_HEADER_SIZE, SMS_CARTRIDGE_RAM_SIZE,
};
use zeff_emu_common::save_ram::SaveRamKind;

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
    rom[offset + CODEMASTERS_HEADER_COMPLEMENT_LO..offset + CODEMASTERS_HEADER_COMPLEMENT_HI + 1]
        .copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + CODEMASTERS_HEADER_ZERO_PADDING_START..offset + CODEMASTERS_HEADER_SIZE].fill(0);
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
    let cart = Cartridge::load_with_hint(&rom_with_codemasters_header(), SystemHint::MasterSystem)
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
fn keeps_standard_mapper_ram_persistence_unknown() {
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
fn game_gear_board_catalog_requires_exact_identity() {
    let absent = GameGearCartridgeIdentity {
        sha256: [0x11; 32],
        source_len: 32 * 1024,
    };
    let persistent = GameGearCartridgeIdentity {
        sha256: [0x22; 32],
        source_len: 256 * 1024,
    };
    let catalog = [
        GameGearBoardCatalogEntry {
            identity: absent,
            ram: GameGearStandardMapperRam::Absent,
        },
        GameGearBoardCatalogEntry {
            identity: persistent,
            ram: GameGearStandardMapperRam::BatteryBacked8KiB,
        },
    ];

    assert_eq!(
        game_gear_standard_mapper_ram_in_catalog(absent, &catalog).map(|identity| identity.ram()),
        Some(GameGearStandardMapperRam::Absent)
    );
    assert_eq!(
        game_gear_standard_mapper_ram_in_catalog(persistent, &catalog)
            .map(|identity| identity.ram()),
        Some(GameGearStandardMapperRam::BatteryBacked8KiB)
    );
    assert_eq!(
        game_gear_standard_mapper_ram_in_catalog(
            GameGearCartridgeIdentity {
                source_len: absent.source_len + 1,
                ..absent
            },
            &catalog
        ),
        None
    );
    assert_eq!(game_gear_standard_mapper_ram_for_identity(absent), None);
}

#[test]
fn game_gear_ram_identity_does_not_weaken_master_system() {
    let rom = rom_with_header(HeaderLocation::Offset0x3ff0, 0x7A);
    let absent = Cartridge::load_with_hint_mapper_and_game_gear_ram(
        &rom,
        SystemHint::GameGear,
        None,
        Some(GameGearStandardMapperRam::Absent),
    )
    .unwrap();
    let persistent = Cartridge::load_with_hint_mapper_and_game_gear_ram(
        &rom,
        SystemHint::GameGear,
        None,
        Some(GameGearStandardMapperRam::BatteryBacked8KiB),
    )
    .unwrap();
    let sms = Cartridge::load_with_hint(&rom, SystemHint::MasterSystem).unwrap();

    assert_eq!(absent.save_ram_kind(), SaveRamKind::None);
    assert_eq!(
        persistent.save_ram_kind(),
        SaveRamKind::known_battery_backed(8 * 1024)
    );
    assert_eq!(
        sms.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE)
    );
    assert!(
        Cartridge::load_with_hint_mapper_and_game_gear_ram(
            &rom,
            SystemHint::MasterSystem,
            None,
            Some(GameGearStandardMapperRam::Absent),
        )
        .is_err()
    );
}

#[test]
fn sg1000_cartridges_have_no_persistent_mapper_ram() {
    let rom = [0x00, 0x76];
    let sg1000 = Cartridge::load_with_hint(&rom, SystemHint::Sg1000).unwrap();
    let sms = Cartridge::load_with_hint(&rom, SystemHint::MasterSystem).unwrap();
    let game_gear = Cartridge::load_with_hint(&rom, SystemHint::GameGear).unwrap();
    assert_eq!(sg1000.save_ram_kind(), SaveRamKind::None);
    assert_eq!(
        sms.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE)
    );
    assert_eq!(
        game_gear.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(SMS_CARTRIDGE_RAM_SIZE)
    );
}

#[test]
fn classifies_codemasters_mapper_ram_as_volatile() {
    let cart = Cartridge::load_with_hint(&rom_with_codemasters_header(), SystemHint::MasterSystem)
        .expect("Codemasters-style ROM should load");

    assert_eq!(
        cart.save_ram_kind(),
        SaveRamKind::known_volatile(crate::hardware::constants::CODEMASTERS_CARTRIDGE_RAM_SIZE)
    );
    assert!(!cart.save_ram_kind().is_battery_backed());
}

#[test]
fn forced_rom_only_mapper_kinds_have_no_mapper_ram_metadata() {
    for mapper_kind in [
        Sega8MapperKind::Korean,
        Sega8MapperKind::Msx,
        Sega8MapperKind::Nemesis,
        Sega8MapperKind::Janggun,
    ] {
        let cart = Cartridge::load_with_hint_and_mapper_kind(
            &rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C),
            SystemHint::MasterSystem,
            Some(mapper_kind),
        )
        .expect("ROM should load with forced mapper");

        assert_eq!(cart.mapper_kind(), mapper_kind);
        assert_eq!(cart.save_ram_kind(), SaveRamKind::none());
    }
}

#[test]
fn rejects_invalid_codemasters_header_padding() {
    let mut rom = rom_with_codemasters_header();
    rom[CODEMASTERS_HEADER_OFFSET + CODEMASTERS_HEADER_ZERO_PADDING_START] = 1;

    let cart = Cartridge::load_with_hint(&rom, SystemHint::MasterSystem).expect("ROM should load");

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
    assert_eq!(cart.normalized_crc32(), crc32fast::hash(&rom));
}

#[test]
fn explicit_mapper_override_takes_precedence_over_detected_headers() {
    let cart = Cartridge::load_with_hint_and_mapper_kind(
        &rom_with_codemasters_header(),
        SystemHint::MasterSystem,
        Some(Sega8MapperKind::Janggun),
    )
    .expect("ROM should load with forced mapper");

    assert!(cart.codemasters_header().is_some());
    assert_eq!(cart.mapper_kind(), Sega8MapperKind::Janggun);
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

#[test]
fn mapper_kind_is_inferred_only_from_explicit_mapper_path_tags() {
    assert_eq!(
        Sega8MapperKind::from_path(std::path::Path::new("game [mapper=janggun].sms")),
        Some(Sega8MapperKind::Janggun)
    );
    assert_eq!(
        Sega8MapperKind::from_path(std::path::Path::new("game (mapper-msx).sms")),
        Some(Sega8MapperKind::Msx)
    );
    assert_eq!(
        Sega8MapperKind::from_path(std::path::Path::new("game nemesis mapper.sms")),
        Some(Sega8MapperKind::Nemesis)
    );
    assert_eq!(
        Sega8MapperKind::from_path(std::path::Path::new("game (Korea).sms")),
        None
    );
}
