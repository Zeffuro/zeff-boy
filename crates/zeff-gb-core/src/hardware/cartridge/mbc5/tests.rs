use super::*;

fn make_mbc5(n_banks: usize, ram_size: usize, has_rumble: bool) -> Mbc5 {
    let mut rom = vec![0u8; n_banks * 0x4000];
    for bank in 0..n_banks {
        let start = bank * 0x4000;
        for byte in &mut rom[start..start + 0x4000] {
            *byte = (bank & 0xFF) as u8;
        }
    }
    Mbc5::new(rom, ram_size, has_rumble)
}

#[test]
fn default_bank_is_1() {
    let mbc = make_mbc5(4, 0, false);
    assert_eq!(mbc.read_rom(0x4000), 1);
}

#[test]
fn bank_0_is_valid_for_mbc5() {
    let mut mbc = make_mbc5(4, 0, false);
    mbc.write_rom(0x2000, 0x00);
    assert_eq!(mbc.read_rom(0x4000), 0);
}

#[test]
fn bank_switching_low_byte() {
    let mut mbc = make_mbc5(256, 0, false);
    mbc.write_rom(0x2000, 0xFF);
    assert_eq!(mbc.read_rom(0x4000), 0xFF);
}

#[test]
fn bank_switching_9_bit() {
    let mut mbc = make_mbc5(512, 0, false);
    mbc.write_rom(0x2000, 0x00);
    mbc.write_rom(0x3000, 0x01);
    assert_eq!(mbc.read_rom(0x4000), 0);
    assert_eq!(mbc.rom_bank, 256);
}

#[test]
fn high_bank_bit_only_uses_bit_0() {
    let mut mbc = make_mbc5(512, 0, false);
    mbc.write_rom(0x3000, 0xFF);
    assert_eq!(mbc.rom_bank & 0x100, 0x100);
}

#[test]
fn ram_enable_disable() {
    let mut mbc = make_mbc5(4, 0x2000, false);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0x42);
    assert_eq!(mbc.read_ram(0xA000), 0x42);

    mbc.write_rom(0x0000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn ram_bank_switching() {
    let mut mbc = make_mbc5(4, 0x8000, false);
    mbc.write_rom(0x0000, 0x0A);

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
fn ram_bank_uses_4_bits_no_rumble() {
    let mut mbc = make_mbc5(4, 0x20000, false);
    mbc.write_rom(0x4000, 0xFF);
    assert_eq!(mbc.ram_bank, 15);
}

#[test]
fn rumble_bit_used_in_ram_bank_register() {
    let mut mbc = make_mbc5(4, 0x8000, true);
    mbc.write_rom(0x4000, 0x08);
    assert!(mbc.rumble_active());
    assert_eq!(mbc.ram_bank, 0);

    mbc.write_rom(0x4000, 0x0B);
    assert!(mbc.rumble_active());
    assert_eq!(mbc.ram_bank, 3);
}

#[test]
fn no_ram_returns_ff() {
    let mut mbc = make_mbc5(4, 0, false);
    mbc.write_rom(0x0000, 0x0A);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn save_state_roundtrip() {
    let mut mbc = make_mbc5(8, 0x2000, false);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x2000, 5);
    mbc.write_ram(0xA000, 0x42);

    let mut writer = StateWriter::new();
    mbc.write_state(&mut writer);
    let data = writer.into_bytes();
    let mut reader = StateReader::new(&data);
    let mut restored = Mbc5::read_state(&mut reader).unwrap();
    restored.restore_rom_bytes(mbc.rom.clone());

    assert_eq!(restored.read_rom(0x4000), 5);
    assert_eq!(restored.read_ram(0xA000), 0x42);
}
