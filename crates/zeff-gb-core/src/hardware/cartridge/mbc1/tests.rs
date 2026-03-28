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
    assert_eq!(restored.banking_mode, true);
}
