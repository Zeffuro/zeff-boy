use super::common::*;

#[test]
fn load_mapper34_chr_ram_uses_bnrom() {
    let h = make_header(4, 0, 0x20, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    let mut prg = vec![0x00; 0x8000];
    prg[0] = 0x01;
    rom.extend(prg);
    rom.extend(vec![0x33; 0x8000]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 34);

    cart.cpu_write(0x8000, 0x01);
    assert_eq!(cart.cpu_read(0x8000), 0x33);

    cart.chr_write(0x0123, 0xA5);
    assert_eq!(cart.chr_read(0x0123), 0xA5);
}

#[test]
fn load_mapper34_chr_rom_uses_nina001() {
    let h = make_header(4, 8, 0x20, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0x10; 0x8000]);
    rom.extend(vec![0x20; 0x8000]);
    for bank in 0..16 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 34);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);

    cart.cpu_write(0x7FFD, 0x01);
    cart.cpu_write(0x7FFE, 0x02);
    cart.cpu_write(0x7FFF, 0x03);

    assert_eq!(cart.cpu_read(0x8000), 0x20);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x1000), 0x03);
}

#[test]
fn load_mapper45_uses_ga23c_outer_registers() {
    let h = make_header(32, 32, 0xD0, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..=255 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 45);

    cart.cpu_write(0x6000, 0x00);
    cart.cpu_write(0x6000, 0x20);
    cart.cpu_write(0x6000, 0x00);
    cart.cpu_write(0x6000, 0x20);
    cart.cpu_write(0x8000, 0x06);
    cart.cpu_write(0x8001, 0x03);

    assert_eq!(cart.cpu_read(0x8000), 0x23);
}

#[test]
fn load_mapper32_uses_irem_g101() {
    let h = make_header(8, 16, 0x00, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 32);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xB004, 0x0A);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x0E);
    assert_eq!(cart.chr_read(0x1000), 0x0A);
}

#[test]
fn load_mapper33_uses_taito_tc0190() {
    let h = make_header(8, 16, 0x10, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 33);

    cart.cpu_write(0x8000, 0x43);
    cart.cpu_write(0x8001, 0x04);
    cart.cpu_write(0x8002, 0x03);
    cart.cpu_write(0xA003, 0x0B);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x0E);
    assert_eq!(cart.chr_read(0x0000), 0x06);
    assert_eq!(cart.chr_read(0x1C00), 0x0B);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
}

#[test]
fn load_mapper66_uses_gxrom() {
    let h = make_header(8, 4, 0x20, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    let mut prg = vec![0x00; 0x8000];
    prg[0] = 0x11;
    rom.extend(prg);
    rom.extend(vec![0x22; 0x8000]);
    rom.extend(vec![0x44; 0x8000]);
    rom.extend(vec![0x66; 0x8000]);
    for bank in [0x00, 0xBB, 0xCC, 0xDD] {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 66);

    cart.cpu_write(0x8000, 0x11);
    assert_eq!(cart.cpu_read(0x8000), 0x22);
    assert_eq!(cart.chr_read(0x0000), 0xBB);
}

#[test]
fn load_mapper11_uses_color_dreams() {
    let h = make_header(8, 16, 0xB0, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    let mut prg = vec![0x00; 0x8000];
    prg[0] = 0x1E;
    rom.extend(prg);
    rom.extend(vec![0x22; 0x8000]);
    rom.extend(vec![0x44; 0x8000]);
    rom.extend(vec![0x66; 0x8000]);
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 11);

    cart.cpu_write(0x8000, 0x1E);
    assert_eq!(cart.cpu_read(0x8000), 0x22);
    assert_eq!(cart.chr_read(0x0000), 14);
}

#[test]
fn load_mapper13_uses_cprom() {
    let h = make_header(2, 0, 0xD1, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    let mut prg = vec![0xFF; 2 * PRG_ROM_BANK_SIZE];
    prg[0] = 0x02;
    rom.extend(prg);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 13);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);

    cart.chr_write(0x0200, 0x11);
    cart.cpu_write(0x8000, 0x03);
    cart.chr_write(0x1200, 0x44);

    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.chr_read(0x0200), 0x11);
    assert_eq!(cart.chr_read(0x1200), 0x44);
    cart.cpu_write(0x8000, 0x00);
    assert_eq!(cart.chr_read(0x1200), 0x11);
}

#[test]
fn load_bad_mapper12_videomation_shape_uses_cprom() {
    let h = make_header(2, 0, 0xC1, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0xFF; 2 * PRG_ROM_BANK_SIZE]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 12);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);

    cart.chr_write(0x1200, 0x22);
    cart.cpu_write(0x8000, 0x01);
    cart.chr_write(0x1200, 0x44);
    assert_eq!(cart.chr_read(0x1200), 0x44);
    cart.cpu_write(0x8000, 0x00);
    assert_eq!(cart.chr_read(0x1200), 0x22);
}

#[test]
fn load_mapper8_uses_ffe_gnrom_latch() {
    let h = make_header(8, 4, 0x80, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in 0..4 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 8);

    cart.cpu_write(0x8000, 0x21);
    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.chr_read(0x0000), 0x01);
}

#[test]
fn load_mapper6_uses_super_magic_card_latch_mode() {
    let h = make_header(16, 0, 0x60, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..32 {
        rom.extend(vec![bank; 0x2000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 6);

    cart.cpu_write(0x8000, 0x14);
    assert_eq!(cart.cpu_read(0x8000), 10);
    assert_eq!(cart.cpu_read(0xC000), 14);
    cart.chr_write(0x0200, 0xA5);
    assert_eq!(cart.chr_read(0x0200), 0xA5);
}

#[test]
fn load_mapper17_uses_super_magic_card_4m_mode() {
    let h = make_header(32, 32, 0x10, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..256 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 17);

    assert_eq!(cart.cpu_read(0x8000), 60);
    assert_eq!(cart.cpu_read(0xA000), 61);
    assert_eq!(cart.cpu_read(0xC000), 62);
    assert_eq!(cart.cpu_read(0xE000), 63);
    cart.cpu_write(0x4504, 3);
    cart.cpu_write(0x4510, 9);
    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.chr_read(0x0000), 9);
}

#[test]
fn load_mapper17_copies_ines_trainer_to_7000() {
    let h = make_header(2, 1, 0x14, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    let mut trainer = vec![0; TRAINER_SIZE];
    trainer[0x123] = 0xA5;
    rom.extend(trainer);
    for bank in 0..4 {
        rom.extend(vec![bank; 0x2000]);
    }
    rom.extend(vec![0; CHR_ROM_BANK_SIZE]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 17);
    assert_eq!(cart.cpu_read(0x7123), 0xA5);
}

#[test]
fn load_mapper17_legacy_ines_trainer_hook_copies_to_5d00() {
    let h = make_header(2, 1, 0x14, 0x10, [0; 8]);
    let mut rom = h.to_vec();
    let mut trainer = vec![0; TRAINER_SIZE];
    trainer[0..3].copy_from_slice(&[0x6C, 0xFC, 0xFF]);
    trainer[0x50] = 0xAD;
    rom.extend(trainer);
    for bank in 0..4 {
        rom.extend(vec![bank; 0x2000]);
    }
    rom.extend(vec![0; CHR_ROM_BANK_SIZE]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 17);
    assert_eq!(cart.cpu_read(0x5D50), 0xAD);
}

#[test]
fn load_mapper17_nes2_submapper_copies_trainer_to_low_window() {
    let h = make_header(2, 1, 0x14, 0x18, [0x20, 0, 0, 0, 0, 0, 0, 0]);
    let mut rom = h.to_vec();
    let mut trainer = vec![0; TRAINER_SIZE];
    trainer[0x10] = 0x5A;
    rom.extend(trainer);
    for bank in 0..4 {
        rom.extend(vec![bank; 0x2000]);
    }
    rom.extend(vec![0; CHR_ROM_BANK_SIZE]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 17);
    assert_eq!(cart.header().submapper_id, 2);
    assert_eq!(cart.cpu_read(0x5E10), 0x5A);
}

#[test]
fn load_mapper99_uses_vs_system_4016_banking() {
    let h = make_header(2, 2, 0x30, 0x61, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in [0x44, 0x55] {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 99);
    assert_eq!(cart.mirroring(), Mirroring::FourScreen);

    assert_eq!(cart.cpu_read(0x8000), 0x00);
    assert_eq!(cart.cpu_read(0xA000), 0x01);
    assert_eq!(cart.cpu_read(0xE000), 0x03);
    assert_eq!(cart.chr_read(0x0100), 0x44);

    cart.cpu_write(0x4016, 0x04);
    assert_eq!(cart.cpu_read(0x8000), 0x00);
    assert_eq!(cart.chr_read(0x0100), 0x55);
}
