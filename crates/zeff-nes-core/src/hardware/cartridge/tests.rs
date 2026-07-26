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
fn load_all_zeros_returns_error() {
    assert!(Cartridge::load(&[0u8; 16]).is_err());
}

#[test]
fn load_random_garbage_returns_error() {
    let garbage: Vec<u8> = (0..=255).cycle().take(1024).collect();
    assert!(Cartridge::load(&garbage).is_err());
}
