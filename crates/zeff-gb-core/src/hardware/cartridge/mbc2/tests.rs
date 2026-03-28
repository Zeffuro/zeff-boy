use super::*;

fn make_mbc2(n_banks: usize) -> Mbc2 {
    let mut rom = vec![0u8; n_banks * 0x4000];
    for bank in 0..n_banks {
        let start = bank * 0x4000;
        for byte in &mut rom[start..start + 0x4000] {
            *byte = bank as u8;
        }
    }
    Mbc2::new(rom)
}

#[test]
fn default_bank_is_1() {
    let mbc = make_mbc2(4);
    assert_eq!(mbc.read_rom(0x4000), 1);
}

#[test]
fn bank_0_corrected_to_1() {
    let mut mbc = make_mbc2(4);
    mbc.write_rom(0x2100, 0x00);
    assert_eq!(mbc.read_rom(0x4000), 1);
}

#[test]
fn bank_switching() {
    let mut mbc = make_mbc2(8);
    mbc.write_rom(0x2100, 3);
    assert_eq!(mbc.read_rom(0x4000), 3);
}

#[test]
fn bank_number_masked_to_4_bits() {
    let mut mbc = make_mbc2(16);
    mbc.write_rom(0x2100, 0xFF);
    assert_eq!(mbc.read_rom(0x4000), 15);
}

#[test]
fn ram_enable_requires_bit8_clear() {
    let mut mbc = make_mbc2(4);

    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0x05);
    assert_eq!(mbc.read_ram(0xA000), 0xF5);

    mbc.write_rom(0x0000, 0x00);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
}

#[test]
fn bank_select_requires_bit8_set() {
    let mut mbc = make_mbc2(8);
    mbc.write_rom(0x2100, 5);
    assert_eq!(mbc.read_rom(0x4000), 5);

    mbc.write_rom(0x2000, 3);
    assert_eq!(mbc.read_rom(0x4000), 5);
}

#[test]
fn ram_is_4_bit() {
    let mut mbc = make_mbc2(4);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0xFF);
    assert_eq!(mbc.read_ram(0xA000), 0xFF);
    mbc.write_ram(0xA000, 0xA3);
    assert_eq!(mbc.read_ram(0xA000), 0xF3);
}

#[test]
fn ram_mirrors_within_512_bytes() {
    let mut mbc = make_mbc2(4);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_ram(0xA000, 0x07);
    assert_eq!(mbc.read_ram(0xA200), mbc.read_ram(0xA000));
}

#[test]
fn save_state_roundtrip() {
    let mut mbc = make_mbc2(4);
    mbc.write_rom(0x0000, 0x0A);
    mbc.write_rom(0x2100, 3);
    mbc.write_ram(0xA000, 0x09);

    let mut writer = StateWriter::new();
    mbc.write_state(&mut writer);
    let data = writer.into_bytes();
    let mut reader = StateReader::new(&data);
    let mut restored = Mbc2::read_state(&mut reader).unwrap();
    restored.restore_rom_bytes(mbc.rom.clone());

    assert_eq!(restored.read_rom(0x4000), 3);
    assert_eq!(restored.read_ram(0xA000), 0xF9);
}
