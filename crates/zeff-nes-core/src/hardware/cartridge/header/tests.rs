use super::*;

fn make_header(prg_banks: u8, chr_banks: u8, flags6: u8, flags7: u8, rest: [u8; 8]) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(INES_MAGIC);
    h[4] = prg_banks;
    h[5] = chr_banks;
    h[6] = flags6;
    h[7] = flags7;
    h[8..16].copy_from_slice(&rest);
    h
}

#[test]
fn reject_short_data() {
    let short = [0u8; 10];
    assert!(RomHeader::parse(&short).is_err());
}

#[test]
fn reject_bad_magic() {
    let mut bad = [0u8; 16];
    bad[0..4].copy_from_slice(b"BAD!");
    assert!(RomHeader::parse(&bad).is_err());
}

#[test]
fn ines_basic_horizontal_mirroring() {
    let h = make_header(2, 1, 0x00, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.format, RomFormat::INes);
    assert_eq!(hdr.prg_rom_size, 2 * PRG_ROM_BANK_SIZE);
    assert_eq!(hdr.chr_rom_size, CHR_ROM_BANK_SIZE);
    assert_eq!(hdr.mirroring, Mirroring::Horizontal);
    assert_eq!(hdr.mapper_id, 0);
    assert!(!hdr.has_battery);
    assert!(!hdr.has_trainer);
}

#[test]
fn ines_vertical_mirroring() {
    let h = make_header(1, 1, 0x01, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.mirroring, Mirroring::Vertical);
}

#[test]
fn ines_four_screen() {
    let h = make_header(1, 1, 0x08, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.mirroring, Mirroring::FourScreen);
}

#[test]
fn ines_battery_and_trainer() {
    let h = make_header(1, 0, 0x06, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert!(hdr.has_battery);
    assert!(hdr.has_trainer);
}

#[test]
fn ines_mapper_number() {
    let h = make_header(1, 0, 0x10, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.mapper_id, 1);

    let h = make_header(1, 0, 0x40, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.mapper_id, 4);

    let h = make_header(1, 0, 0xA0, 0x10, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.mapper_id, 0x1A);
}

#[test]
fn ines_prg_ram_default() {
    let h = make_header(1, 0, 0x00, 0x00, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.prg_ram_size, 8192);
}

#[test]
fn ines_timing_pal() {
    let mut rest = [0u8; 8];
    rest[1] = 0x01;
    let h = make_header(1, 0, 0x00, 0x00, rest);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.timing, TimingMode::Pal);
}

#[test]
fn ines_console_vs_system() {
    let h = make_header(1, 0, 0x00, 0x01, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.console_type, ConsoleType::VsSystem);
}

#[test]
fn nes2_detection() {
    let h = make_header(1, 0, 0x00, 0x08, [0; 8]);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.format, RomFormat::Nes2);
}

#[test]
fn nes2_mapper_and_submapper() {
    let mut rest = [0u8; 8];
    rest[0] = 0x31;
    let h = make_header(1, 0, 0x00, 0x08, rest);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.format, RomFormat::Nes2);
    assert_eq!(hdr.mapper_id, 256);
    assert_eq!(hdr.submapper_id, 3);
}

#[test]
fn nes2_prg_chr_rom_size_simple() {
    let mut rest = [0u8; 8];
    rest[1] = 0x00;
    let h = make_header(16, 2, 0x00, 0x08, rest);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.prg_rom_size, 16 * PRG_ROM_BANK_SIZE);
    assert_eq!(hdr.chr_rom_size, 2 * CHR_ROM_BANK_SIZE);
}

#[test]
fn nes2_ram_sizes() {
    let mut rest = [0u8; 8];
    rest[2] = 0x07;
    rest[3] = 0x07;
    let h = make_header(1, 0, 0x00, 0x08, rest);
    let hdr = RomHeader::parse(&h).unwrap();
    assert_eq!(hdr.prg_ram_size, 8192);
    assert_eq!(hdr.prg_nvram_size, 0);
    assert_eq!(hdr.chr_ram_size, 8192);
    assert_eq!(hdr.chr_nvram_size, 0);
}

#[test]
fn nes2_timing_modes() {
    for (val, expected) in [
        (0, TimingMode::Ntsc),
        (1, TimingMode::Pal),
        (2, TimingMode::MultiRegion),
        (3, TimingMode::Dendy),
    ] {
        let mut rest = [0u8; 8];
        rest[4] = val;
        let h = make_header(1, 0, 0x00, 0x08, rest);
        let hdr = RomHeader::parse(&h).unwrap();
        assert_eq!(hdr.timing, expected);
    }
}

#[test]
fn shift_count_to_size_values() {
    assert_eq!(RomHeader::shift_count_to_size(0), 0);
    assert_eq!(RomHeader::shift_count_to_size(1), 128);
    assert_eq!(RomHeader::shift_count_to_size(7), 8192);
    assert_eq!(RomHeader::shift_count_to_size(10), 65536);
}

#[test]
fn mapper_name_mapping_known_and_unknown() {
    assert_eq!(NesMapper::from(0).name(), "NROM");
    assert_eq!(NesMapper::from(4).name(), "TxROM / MMC3 / MMC6");
    assert_eq!(NesMapper::from(999), NesMapper::Other(999));
    assert_eq!(NesMapper::from(999).to_string(), "Other (999)");
}

#[test]
fn ines_diskdude_style_junk_ignores_legacy_extension_bytes() {
    let h = make_header(
        0x10,
        0x20,
        0x51,
        0x44,
        [0x69, 0x73, 0x6B, 0x44, 0x75, 0x64, 0x65, 0x21],
    );
    let hdr = RomHeader::parse(&h).unwrap();

    assert_eq!(hdr.format, RomFormat::INes);
    assert_eq!(hdr.mapper_id, 5);
    assert_eq!(hdr.prg_ram_size, 8192);
    assert_eq!(hdr.timing, TimingMode::Ntsc);
    assert_eq!(hdr.console_type, ConsoleType::Nes);
}
