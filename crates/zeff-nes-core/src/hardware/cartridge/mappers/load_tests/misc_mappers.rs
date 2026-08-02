use super::common::*;

#[test]
fn load_mapper94_uses_senjou_no_ookami() {
    let h = make_header(8, 0, 0xE0, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 94);

    cart.cpu_write(0x8000, 0x14);
    assert_eq!(cart.cpu_read(0x8000), 0x05);
    assert_eq!(cart.cpu_read(0xC000), 0x07);

    cart.chr_write(0x0123, 0xA5);
    assert_eq!(cart.chr_read(0x0123), 0xA5);
}

#[test]
fn load_mapper97_uses_irem_tam_s1() {
    let h = make_header(8, 0, 0x10, 0x60, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 97);

    cart.cpu_write(0x8000, 0x84);
    assert_eq!(cart.cpu_read(0x8000), 0x07);
    assert_eq!(cart.cpu_read(0xC000), 0x04);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);
}

#[test]
fn load_legacy_mapper98_vs_vrc1_shape_uses_four_screen_vrc1() {
    let h = make_header(4, 8, 0x21, 0x60, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 98);
    assert_eq!(cart.mirroring(), Mirroring::FourScreen);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xC000, 0x05);
    cart.cpu_write(0x9000, 0x07);
    cart.cpu_write(0xE000, 0x02);
    cart.cpu_write(0xF000, 0x03);

    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.cpu_read(0xA000), 4);
    assert_eq!(cart.cpu_read(0xC000), 5);
    assert_eq!(cart.cpu_read(0xE000), 7);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x1000), 0x03);
    assert_eq!(cart.mirroring(), Mirroring::FourScreen);
}

#[test]
fn load_mapper18_uses_jaleco_ss8806() {
    let h = make_header(32, 32, 0x20, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..256usize {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 18);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0x8001, 0x02);
    cart.cpu_write(0xA002, 0x0A);
    cart.cpu_write(0xA003, 0x01);
    cart.cpu_write(0xF002, 0x02);

    assert_eq!(cart.cpu_read(0x8000), 0x23);
    assert_eq!(cart.cpu_read(0xE000), 0x3F);
    assert_eq!(cart.chr_read(0x0400), 0x1A);
    assert_eq!(cart.mirroring(), Mirroring::SingleScreenLower);
}

#[test]
fn load_mapper65_uses_irem_h3001() {
    let h = make_header(8, 16, 0x10, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 65);

    assert_eq!(cart.cpu_read(0x8000), 0);
    assert_eq!(cart.cpu_read(0xA000), 1);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xB004, 0x0A);
    cart.cpu_write(0x9001, 0x80);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x0E);
    assert_eq!(cart.chr_read(0x1000), 0x0A);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
}

#[test]
fn load_mapper67_uses_sunsoft3() {
    let h = make_header(8, 16, 0x30, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..64 {
        rom.extend(vec![bank; 0x0800]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 67);

    cart.cpu_write(0xF800, 0x05);
    cart.cpu_write(0x9800, 0x0A);
    cart.cpu_write(0xE800, 0x03);

    assert_eq!(cart.cpu_read(0x8000), 0x05);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0800), 0x0A);
    assert_eq!(cart.mirroring(), Mirroring::SingleScreenUpper);
}

#[test]
fn load_mapper67_32k_16k_bad_header_uses_cnrom() {
    let h = make_header(2, 2, 0x30, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..2 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..2 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 67);
    assert_eq!(
        cart.effective_mapper_label(),
        "CNROM (bad mapper 67 header)"
    );

    assert_eq!(cart.chr_read(0x0000), 0);
    cart.cpu_write(0xC000, 0xFF);
    assert_eq!(cart.chr_read(0x0000), 1);
}

#[test]
fn bad_mapper33_sweet_home_translation_uses_mmc1_override() {
    let mut mapper_kind = NesMapper::TaitoTc0190;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        SWEET_HOME_TRANSLATION_BAD_MAPPER33_CRC32,
        None,
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::SxRom);
    assert_eq!(
        effective_mapper_label,
        Some("SxROM / MMC1 (bad mapper 33 header)")
    );
}

#[test]
fn bad_mapper64_smb_extreme_uses_nrom_override() {
    let mut mapper_kind = NesMapper::Rambo1;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        SMB_EXTREME_BAD_MAPPER64_CRC32,
        None,
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::Nrom);
    assert_eq!(effective_mapper_label, Some("NROM (bad mapper 64 header)"));
}

#[test]
fn bad_header_mapper3_crc_uses_gxrom_override() {
    let mut mapper_kind = NesMapper::CnRom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER3_TO_GXROM_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::GxRom);
    assert_eq!(effective_mapper_label, Some("GxROM (bad mapper 3 header)"));
}

#[test]
fn mapper3_crc_override_disables_bus_conflicts() {
    for &crc in MAPPER3_NO_BUS_CONFLICT_PRG_CRC32S {
        assert!(!mapper3_has_bus_conflicts(0, Some(crc)));
    }
    assert!(mapper3_has_bus_conflicts(0, Some(0)));
    assert!(!mapper3_has_bus_conflicts(1, None));
    assert!(mapper3_has_bus_conflicts(2, None));
}

#[test]
fn bad_header_false_four_screen_crc_uses_horizontal_mirroring() {
    let mut mirroring = Mirroring::FourScreen;
    apply_bad_header_mirroring_overrides(
        Some(BAD_HEADER_FALSE_FOUR_SCREEN_PRG_CRC32),
        &mut mirroring,
    );
    assert_eq!(mirroring, Mirroring::Horizontal);

    let mut mirroring = Mirroring::FourScreen;
    apply_bad_header_mirroring_overrides(None, &mut mirroring);
    assert_eq!(mirroring, Mirroring::FourScreen);
}

#[test]
fn bad_header_mapper0_crc_uses_mapper16_override() {
    let mut mapper_kind = NesMapper::Nrom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER0_TO_16_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::BandaiEprom24C02);
    assert_eq!(
        effective_mapper_label,
        Some("Bandai mapper 16 (bad mapper 0 header)")
    );
}

#[test]
fn bad_header_mapper0_crc_uses_mapper32_override() {
    let mut mapper_kind = NesMapper::Nrom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER0_TO_32_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::IremG101);
    assert_eq!(
        effective_mapper_label,
        Some("Irem G-101 / mapper 32 (bad mapper 0 header)")
    );
}

#[test]
fn bad_header_mapper1_crc_uses_mmc5_override() {
    let mut mapper_kind = NesMapper::SxRom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER1_TO_MMC5_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::ExRom);
    assert_eq!(
        effective_mapper_label,
        Some("ExROM / MMC5 (bad mapper 1 header)")
    );
}

#[test]
fn bad_header_mapper2_crc_uses_mmc1_override() {
    let mut mapper_kind = NesMapper::UxRom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER2_TO_MMC1_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::SxRom);
    assert_eq!(
        effective_mapper_label,
        Some("SxROM / MMC1 (bad mapper 2 header)")
    );
}

#[test]
fn bad_header_mapper7_crc_uses_mapper34_override() {
    let mut mapper_kind = NesMapper::AxRom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER7_TO_34_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::BnRom);
    assert_eq!(
        effective_mapper_label,
        Some("BNROM / mapper 34 (bad mapper 7 header)")
    );
}

#[test]
fn bad_header_mapper7_crc_uses_mapper71_override() {
    let mut mapper_kind = NesMapper::AxRom;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        0,
        Some(BAD_HEADER_MAPPER7_TO_71_PRG_CRC32),
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::CamericaCodemasters);
    assert_eq!(
        effective_mapper_label,
        Some("Camerica / Codemasters mapper 71 (bad mapper 7 header)")
    );
}

#[test]
fn load_mapper64_uses_rambo1() {
    let h = make_header(8, 16, 0x00, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 64);

    cart.cpu_write(0x8000, 0x06);
    cart.cpu_write(0x8001, 0x03);
    cart.cpu_write(0x8000, 0x07);
    cart.cpu_write(0x8001, 0x04);
    cart.cpu_write(0x8000, 0x0F);
    cart.cpu_write(0x8001, 0x05);
    cart.cpu_write(0x8000, 0x20);
    cart.cpu_write(0x8001, 0x02);
    cart.cpu_write(0x8000, 0x28);
    cart.cpu_write(0x8001, 0x0A);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x05);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x0400), 0x0A);
}

#[test]
fn load_mapper113_uses_nina_multicart() {
    let h = make_header(16, 16, 0x10, 0x70, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 113);

    cart.cpu_write(0x4100, 0xD2);
    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.chr_read(0x0000), 0x0A);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);
}

#[test]
fn load_mapper119_uses_tqrom_chr_rom_and_ram() {
    let h = make_header(8, 8, 0x70, 0x70, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..64 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 119);

    cart.cpu_write(0x8000, 0x02);
    cart.cpu_write(0x8001, 0x05);
    assert_eq!(cart.chr_read(0x1000), 0x05);

    cart.cpu_write(0x8001, 0x45);
    cart.chr_write(0x1000, 0xA5);
    assert_eq!(cart.chr_read(0x1000), 0xA5);

    cart.cpu_write(0x8000, 0x06);
    cart.cpu_write(0x8001, 0x03);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
}

#[test]
fn load_mapper92_uses_jaleco_fixed_low_layout() {
    let h = make_header(16, 16, 0xC0, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        let mut prg = vec![bank; PRG_ROM_BANK_SIZE];
        if bank == 0 {
            prg[0] = 0xFF;
        }
        rom.extend(prg);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 92);

    cart.cpu_write(0x8000, 0x84);
    assert_eq!(cart.cpu_read(0x8001), 0x00);
    assert_eq!(cart.cpu_read(0xC001), 0x04);
}

#[test]
fn load_mapper15_uses_contra100_in_1() {
    let h = make_header(16, 0, 0xF0, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..32 {
        rom.extend(vec![bank; 0x2000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 15);

    cart.cpu_write(0x8001, 0x04);
    assert_eq!(cart.cpu_read(0x8000), 0x08);
    assert_eq!(cart.cpu_read(0xC000), 0x0E);

    cart.cpu_write(0x6000, 0xA5);
    assert_eq!(cart.cpu_read(0x6000), 0xA5);
    cart.chr_write(0x0100, 0x5A);
    assert_eq!(cart.chr_read(0x0100), 0x5A);
}

#[test]
fn load_mapper232_uses_quattro() {
    let h = make_header(16, 0, 0x80, 0xE0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 232);

    cart.cpu_write(0x8000, 0x10);
    cart.cpu_write(0xC000, 0x02);
    assert_eq!(cart.cpu_read(0x8000), 0x0A);
    assert_eq!(cart.cpu_read(0xC000), 0x0B);
}

#[test]
fn load_mapper23_uses_vrc4_compatible_address_lines() {
    let h = make_header(8, 16, 0x70, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 23);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xB008, 0x0A);
    cart.cpu_write(0xB00C, 0x01);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.chr_read(0x0400), 0x1A);
}

#[test]
fn load_mapper22_uses_vrc2a_address_lines() {
    let h = make_header(8, 16, 0x60, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 22);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xB001, 0x0A);
    cart.cpu_write(0xB003, 0x01);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.chr_read(0x0400), 0x0D);
}

#[test]
fn load_mapper25_uses_vrc4_compatible_address_lines() {
    let h = make_header(8, 16, 0x90, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 25);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xB004, 0x0A);
    cart.cpu_write(0xB00C, 0x01);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.chr_read(0x0400), 0x1A);
}
