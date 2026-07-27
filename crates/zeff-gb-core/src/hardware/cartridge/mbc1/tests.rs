use super::*;

fn make_mbc1(n_banks: usize, ram_size: usize) -> Mbc1 {
    let mut rom = vec![0u8; n_banks * ROM_BANK_SIZE];
    for bank in 0..n_banks {
        let start = bank * ROM_BANK_SIZE;
        for byte in &mut rom[start..start + ROM_BANK_SIZE] {
            *byte = bank as u8;
        }
    }
    Mbc1::new(rom, ram_size)
}

fn make_mbc1_multicart() -> Mbc1 {
    let mut rom = vec![0u8; 64 * ROM_BANK_SIZE];
    for bank in 0..64 {
        let start = bank * ROM_BANK_SIZE;
        for byte in &mut rom[start..start + ROM_BANK_SIZE] {
            *byte = bank as u8;
        }
    }
    for bank in [0, 16, 32, 48] {
        let start = bank * ROM_BANK_SIZE + 0x0104;
        rom[start..start + NINTENDO_LOGO.len()].copy_from_slice(&NINTENDO_LOGO);
    }
    Mbc1::new(rom, 0)
}

#[test]
fn default_bank_is_1() {
    let mbc = make_mbc1(4, 0);
    assert_eq!(mbc.read_rom(0x4000), 1);
}

#[test]
fn bank_0_corrected_to_1() {
    let mut mbc = make_mbc1(8, 0);
    mbc.write_rom(0x2000, 0x00);
    assert_eq!(mbc.read_rom(0x4000), 1);
}

#[test]
fn bank_switching_basic() {
    let mut mbc = make_mbc1(8, 0);
    mbc.write_rom(0x2000, 3);
    assert_eq!(mbc.read_rom(0x4000), 3);
    mbc.write_rom(0x2000, 7);
    assert_eq!(mbc.read_rom(0x4000), 7);
}

#[test]
fn bank_number_masked_to_5_bits() {
    let mut mbc = make_mbc1(32, 0);
    mbc.write_rom(0x2000, 0xFF);
    assert_eq!(mbc.read_rom(0x4000), 31);
}

#[test]
fn ram_disabled_by_default() {
    let mbc = make_mbc1(4, 0x2000);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn ram_enable_disable() {
    let mut mbc = make_mbc1(4, 0x2000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0x42);
    assert_eq!(mbc.read_ram(0xA000), 0x42);

    mbc.write_rom(0x0000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn ram_write_ignored_when_disabled() {
    let mut mbc = make_mbc1(4, 0x2000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0x42);
    mbc.write_rom(0x0000, 0x00);
    mbc.write_ram(0xA000, 0xFF);
    mbc.write_rom(0x0000, 0x0A);
    assert_eq!(mbc.read_ram(0xA000), 0x42);
}

#[test]
fn banking_mode_0_uses_bank_0_for_low_rom() {
    let mbc = make_mbc1(64, 0);
    assert_eq!(mbc.read_rom(0x0000), 0);
}

#[test]
fn banking_mode_1_uses_ram_bank_for_low_rom() {
    let mut mbc = make_mbc1(64, 0);
    mbc.write_rom(0x6000, 0x01);
    mbc.write_rom(0x4000, 0x01);
    assert_eq!(mbc.read_rom(0x0000), 32);
}

#[test]
fn normal_64_bank_cart_uses_32_bank_high_rom_groups() {
    let mut mbc = make_mbc1(64, 0);
    assert!(!mbc.multicart);

    mbc.write_rom(0x4000, 0x01);
    mbc.write_rom(0x2000, 0x10);

    assert_eq!(mbc.read_rom(0x4000), 48);
}

#[test]
fn multicart_uses_16_bank_high_rom_groups() {
    let mut mbc = make_mbc1_multicart();
    assert!(mbc.multicart);

    mbc.write_rom(0x6000, 0x01);
    mbc.write_rom(0x4000, 0x01);
    assert_eq!(mbc.read_rom(0x0000), 16);

    mbc.write_rom(0x2000, 0x00);
    assert_eq!(mbc.read_rom(0x4000), 17);

    mbc.write_rom(0x2000, 0x10);
    assert_eq!(mbc.read_rom(0x4000), 16);

    mbc.write_rom(0x4000, 0x03);
    mbc.write_rom(0x2000, 0x1F);
    assert_eq!(mbc.read_rom(0x4000), 63);
}

#[test]
fn banking_mode_1_ram_bank_switching() {
    let mut mbc = make_mbc1(4, 0x8000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x6000, 0x01);

    mbc.write_rom(0x4000, 0x00);
    mbc.write_ram(0xA000, 0xAA);

    mbc.write_rom(0x4000, 0x01);
    mbc.write_ram(0xA000, 0xBB);

    mbc.write_rom(0x4000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0xAA);

    mbc.write_rom(0x4000, 0x01);
    assert_eq!(mbc.read_ram(0xA000), 0xBB);
}

#[test]
fn banking_mode_1_mirrors_single_ram_bank() {
    let mut mbc = make_mbc1(4, 0x2000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x6000, 0x01);

    mbc.write_rom(0x4000, 0x00);
    mbc.write_ram(0xA000, 0xAA);

    mbc.write_rom(0x4000, 0x01);
    assert_eq!(mbc.read_ram(0xA000), 0xAA);
    mbc.write_ram(0xA000, 0xBB);

    mbc.write_rom(0x4000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0xBB);
}

#[test]
fn banking_mode_0_always_uses_ram_bank_0() {
    let mut mbc = make_mbc1(4, 0x8000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x4000, 0x01);
    mbc.write_ram(0xA000, 0x42);

    mbc.write_rom(0x4000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0x42);
}

#[test]
fn rom_bank_mask_prevents_out_of_bounds() {
    let mut mbc = make_mbc1(4, 0);
    mbc.write_rom(0x2000, 7);
    assert_eq!(mbc.read_rom(0x4000), 3);
}

#[test]
fn no_ram_returns_ff() {
    let mut mbc = make_mbc1(4, 0);
    mbc.write_rom(0x0000, 0x0A);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn save_state_roundtrip() {
    let mut mbc = make_mbc1(8, 0x8000);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x2000, 5);
    mbc.write_rom(0x6000, 0x01);
    mbc.write_rom(0x4000, 0x01);
    mbc.write_ram(0xA000, 0x42);

    let mut writer = StateWriter::new();
    mbc.write_state(&mut writer);
    let data = writer.into_bytes();
    let mut reader = StateReader::new(&data);
    let mut restored = Mbc1::read_state(&mut reader).unwrap();
    restored.restore_rom_bytes(mbc.rom.clone());

    assert_eq!(restored.read_rom(0x4000), mbc.read_rom(0x4000));
    assert_eq!(restored.read_ram(0xA000), 0x42);
    assert!(restored.banking_mode);
}
