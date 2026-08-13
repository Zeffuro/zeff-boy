use super::*;
use header::{CHR_ROM_BANK_SIZE, PRG_ROM_BANK_SIZE};

use super::test_utils::make_header;

#[test]
fn load_empty_data_returns_error() {
    assert!(Cartridge::load(&[]).is_err());
}

#[test]
fn load_too_short_for_header_returns_error() {
    assert!(Cartridge::load(&[0x4E, 0x45, 0x53, 0x1A, 0x01]).is_err());
}

#[test]
fn load_header_only_no_prg_data_returns_error() {
    let h = make_header(1, 0, 0x00, 0x00, [0; 8]);
    assert!(Cartridge::load(&h).is_err());
}

#[test]
fn load_zero_prg_banks_returns_error() {
    let h = make_header(0, 1, 0x00, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; CHR_ROM_BANK_SIZE]);
    assert!(Cartridge::load(&rom).is_err());
}

#[test]
fn load_truncated_prg_returns_error() {
    let h = make_header(2, 0, 0x00, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; PRG_ROM_BANK_SIZE]);
    assert!(Cartridge::load(&rom).is_err());
}

#[test]
fn load_truncated_chr_returns_error() {
    let h = make_header(1, 1, 0x00, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; PRG_ROM_BANK_SIZE]);
    assert!(Cartridge::load(&rom).is_err());
}

#[test]
fn load_trainer_flag_but_truncated() {
    let h = make_header(1, 0, 0x04, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; PRG_ROM_BANK_SIZE]);
    assert!(Cartridge::load(&rom).is_err());
}

#[test]
fn load_zero_chr_uses_chr_ram() {
    let h = make_header(1, 0, 0x00, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; PRG_ROM_BANK_SIZE]);
    let cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().chr_rom_size, 0);
}

#[test]
fn load_valid_minimal_nrom_succeeds() {
    let h = make_header(1, 1, 0x00, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0u8; PRG_ROM_BANK_SIZE + CHR_ROM_BANK_SIZE]);
    let cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 0);
}

#[test]
fn load_tracks_prg_rom_crc_separately_from_file_crc() {
    let h = make_header(1, 1, 0x00, 0x00, [0; 8]);
    let prg_rom: Vec<u8> = (0..PRG_ROM_BANK_SIZE).map(|i| i as u8).collect();
    let chr_rom = vec![0u8; CHR_ROM_BANK_SIZE];
    let mut rom = h.to_vec();
    rom.extend_from_slice(&prg_rom);
    rom.extend_from_slice(&chr_rom);

    let cart = Cartridge::load(&rom).unwrap();

    assert_eq!(cart.prg_crc32(), crc32fast::hash(&prg_rom));
    assert_eq!(cart.rom_crc32(), crc32fast::hash(&rom));
    assert_ne!(cart.prg_crc32(), cart.rom_crc32());
}

#[test]
fn load_fds_requires_8k_bios() {
    let image = mappers::FdsImage::parse(&vec![0x44; mappers::FDS_SIDE_SIZE])
        .expect("FDS image should parse");

    let err = match Cartridge::load_fds(image, vec![0xFF; mappers::FDS_BIOS_SIZE - 1]) {
        Ok(_) => panic!("short FDS BIOS should be rejected"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("FDS BIOS size mismatch"));
}

#[test]
fn load_fds_builds_synthetic_mapper20_cartridge() {
    let side = vec![0x21; mappers::FDS_SIDE_SIZE];
    let image = mappers::FdsImage::parse(&side).expect("FDS image should parse");
    let bios = vec![0xEA; mappers::FDS_BIOS_SIZE];

    let cart = Cartridge::load_fds(image, bios.clone()).expect("FDS cartridge should load");

    assert_eq!(cart.header().format, RomFormat::Nes2);
    assert_eq!(cart.header().mapper_kind(), NesMapper::Fds);
    assert_eq!(cart.header().prg_rom_size, mappers::FDS_BIOS_SIZE);
    assert_eq!(cart.header().prg_ram_size, 0x8000);
    assert_eq!(cart.header().chr_ram_size, 0x2000);
    assert_eq!(cart.prg_crc32(), crc32fast::hash(&bios));
    assert_eq!(cart.rom_crc32(), crc32fast::hash(&side));
    assert_eq!(cart.effective_mapper_label(), "Famicom Disk System");
    assert_eq!(cart.cpu_peek(0xE000), 0xEA);
}

#[test]
fn load_fds_disk_crc_ignores_optional_fw_nes_header() {
    let raw_side = vec![0x7E; mappers::FDS_SIDE_SIZE];
    let mut headered = [0; mappers::FDS_HEADER_SIZE].to_vec();
    headered[..4].copy_from_slice(b"FDS\x1A");
    headered[4] = 1;
    headered.extend_from_slice(&raw_side);
    let raw_image = mappers::FdsImage::parse(&raw_side).expect("raw FDS image should parse");
    let headered_image =
        mappers::FdsImage::parse(&headered).expect("headered FDS image should parse");
    let bios = vec![0xEA; mappers::FDS_BIOS_SIZE];

    let raw_cart = Cartridge::load_fds(raw_image, bios.clone()).expect("raw FDS should load");
    let headered_cart =
        Cartridge::load_fds(headered_image, bios).expect("headered FDS should load");

    assert_eq!(raw_cart.rom_crc32(), headered_cart.rom_crc32());
}

#[test]
fn load_all_zeros_returns_error() {
    assert!(Cartridge::load(&[0u8; 16]).is_err());
}

#[test]
fn load_random_garbage_returns_error() {
    let garbage: Vec<u8> = (0..=255).cycle().take(1024).collect();
    assert!(Cartridge::load(&garbage).is_err());
}
