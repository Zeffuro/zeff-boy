use super::*;
use header::{CHR_ROM_BANK_SIZE, INES_MAGIC, PRG_ROM_BANK_SIZE};

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

#[test]
fn load_mapper112_uses_scrambled_mmc3_like_registers() {
    let h = make_header(8, 8, 0x00, 0x70, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..64 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 112);

    cart.cpu_write(0x8000, 0);
    cart.cpu_write(0xA000, 3);
    cart.cpu_write(0x8000, 1);
    cart.cpu_write(0xA000, 4);
    cart.cpu_write(0x8000, 2);
    cart.cpu_write(0xA000, 6);
    cart.cpu_write(0x8000, 4);
    cart.cpu_write(0xA000, 9);
    cart.cpu_write(0xE000, 1);

    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.cpu_read(0xA000), 4);
    assert_eq!(cart.cpu_read(0xC000), 14);
    assert_eq!(cart.chr_read(0x0000), 6);
    assert_eq!(cart.chr_read(0x0400), 7);
    assert_eq!(cart.chr_read(0x1000), 9);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);
}

#[test]
fn load_mapper140_uses_jaleco_jf11_bank_port() {
    let h = make_header(8, 16, 0xC0, 0x80, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 140);

    cart.cpu_write(0x6000, 0x21);
    assert_eq!(cart.cpu_read(0x8000), 2);
    assert_eq!(cart.chr_read(0x0000), 1);

    cart.cpu_write(0x8000, 0x13);
    assert_eq!(cart.cpu_read(0x8000), 2);
    assert_eq!(cart.chr_read(0x0000), 1);
}

#[test]
fn load_mapper91_uses_chr_prg_registers() {
    let h = make_header(8, 64, 0xB0, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..256 {
        rom.extend(vec![bank as u8; 0x0800]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 91);

    cart.cpu_write(0x6000, 3);
    cart.cpu_write(0x6001, 4);
    cart.cpu_write(0x7000, 5);
    cart.cpu_write(0x7001, 6);
    assert_eq!(cart.cpu_read(0x8000), 5);
    assert_eq!(cart.cpu_read(0xA000), 6);
    assert_eq!(cart.cpu_read(0xC000), 14);
    assert_eq!(cart.chr_read(0x0000), 3);
    assert_eq!(cart.chr_read(0x0800), 4);
}

#[test]
fn load_mapper90_uses_jy_asic_registers() {
    let h = make_header(32, 64, 0xA0, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..512 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 90);

    cart.cpu_write(0xD000, 0x18 | 0x02);
    cart.cpu_write(0x8000, 3);
    cart.cpu_write(0x8001, 4);
    cart.cpu_write(0x8002, 5);
    cart.cpu_write(0x9000, 9);
    cart.cpu_write(0x9002, 10);
    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.cpu_read(0xA000), 4);
    assert_eq!(cart.cpu_read(0xC000), 5);
    assert_eq!(cart.cpu_read(0xE000), 63);
    assert_eq!(cart.chr_read(0x0000), 9);
    assert_eq!(cart.chr_read(0x0800), 10);
}

#[test]
fn load_mapper209_uses_jy_asic_rom_nametable_registers() {
    let h = make_header(8, 64, 0x10, 0xD0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..512 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 209);

    cart.cpu_write(0xD000, 0x20);
    cart.cpu_write(0xD002, 0x00);
    cart.cpu_write(0xB000, 0x8D);
    cart.cpu_write(0xB004, 0x01);

    assert_eq!(cart.ppu_nametable_read(0x2000, &[0; 0x1000]), Some(0x8D));
}

#[test]
fn load_mapper211_forces_jy_asic_nametable_control() {
    let h = make_header(8, 64, 0x30, 0xD0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..512 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 211);

    cart.cpu_write(0xD002, 0x00);
    cart.cpu_write(0xB000, 0x8D);
    cart.cpu_write(0xB004, 0x01);

    assert_eq!(cart.ppu_nametable_read(0x2000, &[0; 0x1000]), Some(0x8D));
}

#[test]
fn load_mapper35_reuses_jy_asic_mapper209_variant() {
    let h = make_header(8, 64, 0x30, 0x20, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..512 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 35);

    cart.cpu_write(0xD000, 0x20);
    cart.cpu_write(0xB000, 0x8D);
    cart.cpu_write(0xB004, 0x01);
    assert_eq!(cart.ppu_nametable_read(0x2000, &[0; 0x1000]), Some(0x8D));
}

#[test]
fn load_mapper151_reuses_vrc1_with_four_screen_mirroring() {
    let h = make_header(4, 4, 0x70, 0x90, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..8 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 151);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0x9000, 0x01);
    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.mirroring(), Mirroring::FourScreen);
}

#[test]
fn load_mapper240_uses_low_address_gnrom_register() {
    let h = make_header(8, 4, 0x00, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in [0x44, 0x55, 0x66, 0x77] {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 240);

    cart.cpu_write(0x4800, 0x21);
    assert_eq!(cart.cpu_read(0x8000), 2);
    assert_eq!(cart.chr_read(0x0000), 0x55);
}

#[test]
fn load_mapper242_uses_address_latch_and_chr_ram() {
    let h = make_header(32, 0, 0x20, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..32 {
        rom.extend(vec![bank; 0x4000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 242);

    cart.cpu_write(0x822E, 0x00);
    assert_eq!(cart.cpu_read(0x8000), 0x0B);
    assert_eq!(cart.cpu_read(0xC000), 0x0F);

    cart.chr_write(0x0123, 0xA5);
    assert_eq!(cart.chr_read(0x0123), 0xA5);
}

#[test]
fn load_mapper245_uses_waixing_f003_chr_ram() {
    let h = make_header(32, 0, 0x50, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 245);

    cart.cpu_write(0x8000, 0x06);
    cart.cpu_write(0x8001, 0x03);
    assert_eq!(cart.cpu_read(0x8000), 0x03);

    cart.chr_write(0x1000, 0xA5);
    assert_eq!(cart.chr_read(0x1000), 0xA5);
}

#[test]
fn load_mapper246_uses_g0151_registers() {
    let h = make_header(32, 64, 0x60, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..64 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..256 {
        rom.extend(vec![bank as u8; 0x0800]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 246);

    cart.cpu_write(0x6000, 3);
    cart.cpu_write(0x6001, 4);
    cart.cpu_write(0x6002, 5);
    cart.cpu_write(0x6003, 6);
    cart.cpu_write(0x6004, 9);
    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.cpu_read(0xA000), 4);
    assert_eq!(cart.cpu_read(0xC000), 5);
    assert_eq!(cart.cpu_read(0xFFF0), 6);
    assert_eq!(cart.cpu_read(0xFFFC), 0x16);
    assert_eq!(cart.chr_read(0x0000), 9);
}

#[test]
fn load_mapper250_uses_nitra_address_encoded_mmc3_writes() {
    let h = make_header(8, 16, 0xA0, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 250);

    cart.cpu_write(0x8006, 0xFF);
    cart.cpu_write(0x8403, 0x00);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
}

#[test]
fn load_mapper251_treats_trailing_bytes_as_chr_after_declared_prg() {
    let h = make_header(1, 0, 0xB0, 0xF0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..2 {
        rom.extend(vec![bank; 0x2000]);
    }
    rom.extend(vec![0xA5; 0x2000]);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 251);
    assert_eq!(cart.cpu_read(0xE000), 0x01);
    assert_eq!(cart.chr_read(0x0000), 0xA5);
}

#[test]
fn load_mapper83_uses_cony_yoko_large_chr_board() {
    let h = make_header(16, 64, 0x30, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..32 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..512 {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 83);

    cart.cpu_write(0x8100, 0x10 | 0x01);
    cart.cpu_write(0x8300, 3);
    cart.cpu_write(0x8301, 4);
    cart.cpu_write(0x8310, 2);
    cart.cpu_write(0x8311, 3);
    assert_eq!(cart.cpu_read(0x8000), 3);
    assert_eq!(cart.cpu_read(0xA000), 4);
    assert_eq!(cart.cpu_read(0xE000), 31);
    assert_eq!(cart.chr_read(0x0000), 4);
    assert_eq!(cart.chr_read(0x0800), 6);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
}

#[test]
fn load_mapper9_uses_mmc2_latch_banking() {
    let h = make_header(4, 4, 0x90, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..8 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 9);

    cart.cpu_write(0xA000, 0x02);
    cart.cpu_write(0xB000, 0x01);
    cart.cpu_write(0xC000, 0x02);
    cart.cpu_write(0xD000, 0x03);
    cart.cpu_write(0xE000, 0x04);
    cart.cpu_write(0xF000, 0x01);

    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.cpu_read(0xA000), 0x05);
    assert_eq!(cart.cpu_read(0xE000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x0FD8), 0x02);
    assert_eq!(cart.chr_read(0x0000), 0x01);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
}

#[test]
fn load_mapper10_uses_mmc4_latch_banking() {
    let h = make_header(8, 4, 0xA0, 0x00, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..8 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 10);

    cart.cpu_write(0xA000, 0x03);
    cart.cpu_write(0xB000, 0x01);
    cart.cpu_write(0xC000, 0x02);
    cart.cpu_write(0xD000, 0x03);
    cart.cpu_write(0xE000, 0x04);
    cart.cpu_write(0x6000, 0xA5);

    assert_eq!(cart.cpu_read(0x6000), 0xA5);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x0FD9), 0x02);
    assert_eq!(cart.chr_read(0x0000), 0x01);
}

#[test]
fn load_mapper70_uses_bandai_74161() {
    let h = make_header(8, 6, 0x60, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        let fill = (bank as u8) * 0x11;
        let mut prg = vec![fill; PRG_ROM_BANK_SIZE];
        if bank == 7 {
            prg[0] = 0x25;
        }
        rom.extend(prg);
    }
    for bank in [0x00, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE] {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 70);

    cart.cpu_write(0xC000, 0x25);
    assert_eq!(cart.cpu_read(0x8000), 0x22);
    assert_eq!(cart.cpu_read(0xC001), 0x77);
    assert_eq!(cart.chr_read(0x0000), 0xEE);
}

#[test]
fn load_mapper71_uses_camerica() {
    let h = make_header(16, 0, 0x70, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 71);

    cart.cpu_write(0xC000, 0x02);
    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.cpu_read(0xC000), 0x0F);

    cart.chr_write(0x0400, 0xA5);
    assert_eq!(cart.chr_read(0x0400), 0xA5);
}

#[test]
fn load_mapper79_uses_nina03() {
    let h = make_header(4, 4, 0xF0, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0x88; 2 * PRG_ROM_BANK_SIZE]);
    rom.extend(vec![0x99; 2 * PRG_ROM_BANK_SIZE]);
    for bank in [0x00, 0x11, 0x22, 0x33] {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 79);

    assert_eq!(cart.cpu_read(0x8000), 0x88);
    cart.cpu_write(0x4100, 0x0A);
    assert_eq!(cart.cpu_read(0x8000), 0x99);
    assert_eq!(cart.chr_read(0x0000), 0x22);
}

#[test]
fn load_mapper75_uses_vrc1() {
    let h = make_header(8, 16, 0xB0, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..32 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 75);

    cart.cpu_write(0x8000, 0x03);
    cart.cpu_write(0xA000, 0x04);
    cart.cpu_write(0xC000, 0x05);
    cart.cpu_write(0x9000, 0x07);
    cart.cpu_write(0xE000, 0x02);
    cart.cpu_write(0xF000, 0x03);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x05);
    assert_eq!(cart.cpu_read(0xE000), 0x0F);
    assert_eq!(cart.chr_read(0x0000), 0x12);
    assert_eq!(cart.chr_read(0x1000), 0x13);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
}

#[test]
fn load_mapper77_uses_napoleon_senki() {
    let h = make_header(8, 4, 0xD0, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; 0x0800]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 77);

    cart.cpu_write(0x8000, 0x21);
    assert_eq!(cart.cpu_read(0x8000), 0x01);
    assert_eq!(cart.chr_read(0x0000), 0x02);

    cart.chr_write(0x0800, 0xA5);
    assert_eq!(cart.chr_read(0x0800), 0xA5);
}

#[test]
fn load_mapper78_uses_holy_diver_cosmo_carrier() {
    let h = make_header(8, 16, 0xE8, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 78);

    cart.cpu_write(0x8000, 0xA3);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x0A);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);

    cart.cpu_write(0x8000, 0xAB);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);
}

#[test]
fn load_mapper72_uses_jaleco_jf17() {
    let h = make_header(8, 16, 0x80, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
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
    assert_eq!(cart.header().mapper_id, 72);

    cart.cpu_write(0x8000, 0xC3);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x03);
}

#[test]
fn load_mapper73_uses_vrc3() {
    let h = make_header(8, 0, 0x90, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 73);

    cart.cpu_write(0xF000, 0x03);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);

    cart.cpu_write(0x6000, 0xA5);
    assert_eq!(cart.cpu_read(0x6000), 0xA5);
    cart.chr_write(0x0123, 0x5A);
    assert_eq!(cart.chr_read(0x0123), 0x5A);
}

#[test]
fn load_mapper88_uses_namco108_variant() {
    let h = make_header(8, 16, 0x80, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 88);

    cart.cpu_write(0x8000, 0x06);
    cart.cpu_write(0x8001, 0x03);
    cart.cpu_write(0x8000, 0x07);
    cart.cpu_write(0x8001, 0x04);
    cart.cpu_write(0x8000, 0x00);
    cart.cpu_write(0x8001, 0x06);
    cart.cpu_write(0x8000, 0x02);
    cart.cpu_write(0x8001, 0x05);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x0E);
    assert_eq!(cart.chr_read(0x0000), 0x06);
    assert_eq!(cart.chr_read(0x0400), 0x07);
    assert_eq!(cart.chr_read(0x1000), 0x45);
}

#[test]
fn load_mapper185_uses_cnrom_protection() {
    let h = make_header(2, 1, 0x90, 0xB0, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0xFF; 2 * PRG_ROM_BANK_SIZE]);
    let mut chr = vec![0; CHR_ROM_BANK_SIZE];
    chr[0] = 0x3C;
    rom.extend(chr);

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 185);

    assert_ne!(cart.chr_read_with_kind(0x0000, ChrFetchKind::CpuData), 0x3C);
    assert_ne!(cart.chr_read_with_kind(0x0000, ChrFetchKind::CpuData), 0x3C);
    assert_eq!(cart.chr_read_with_kind(0x0000, ChrFetchKind::CpuData), 0x3C);
}

#[test]
fn load_mapper184_uses_sunsoft1() {
    let h = make_header(2, 4, 0x80, 0xB0, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0xEA; 2 * PRG_ROM_BANK_SIZE]);
    for bank in 0..8 {
        rom.extend(vec![bank; 0x1000]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 184);

    cart.cpu_write(0x6000, 0x62);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    assert_eq!(cart.chr_read(0x1000), 0x06);
}

#[test]
fn load_mapper188_uses_karaoke_studio() {
    let h = make_header(16, 0, 0xC0, 0xB0, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 188);

    cart.cpu_write(0x8000, 0x12);
    assert_eq!(cart.cpu_read(0x8000), 0x02);
    assert_eq!(cart.cpu_read(0xC000), 0x07);

    cart.cpu_write(0x8000, 0x23);
    assert_eq!(cart.cpu_read(0x8000), 0x0B);
    assert_eq!(cart.mirroring(), Mirroring::Horizontal);
    assert_eq!(cart.cpu_read(0x6000), 0x03);
}

#[test]
fn load_mapper80_uses_taito_x1005() {
    let h = make_header(8, 16, 0x00, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 80);

    cart.cpu_write(0x7EFA, 0x03);
    cart.cpu_write(0x7EFC, 0x04);
    cart.cpu_write(0x7EFE, 0x05);
    cart.cpu_write(0x7EF0, 0x06);
    cart.cpu_write(0x7EF5, 0x0E);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x05);
    assert_eq!(cart.cpu_read(0xE000), 0x0F);
    assert_eq!(cart.chr_read(0x0000), 0x06);
    assert_eq!(cart.chr_read(0x0400), 0x07);
    assert_eq!(cart.chr_read(0x1C00), 0x0E);
}

#[test]
fn load_mapper82_uses_taito_x1017() {
    let h = make_header(8, 16, 0x20, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..16 {
        rom.extend(vec![bank; 0x2000]);
    }
    for bank in 0..128 {
        rom.extend(vec![bank; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 82);

    cart.cpu_write(0x7EFA, 0x0C);
    cart.cpu_write(0x7EFB, 0x10);
    cart.cpu_write(0x7EFC, 0x14);
    cart.cpu_write(0x7EF0, 0x06);
    cart.cpu_write(0x7EF5, 0x0E);
    cart.cpu_write(0x7EF6, 0x01);

    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xA000), 0x04);
    assert_eq!(cart.cpu_read(0xC000), 0x05);
    assert_eq!(cart.chr_read(0x0000), 0x06);
    assert_eq!(cart.chr_read(0x1C00), 0x0E);
    assert_eq!(cart.mirroring(), Mirroring::Vertical);
}

#[test]
fn load_mapper86_uses_jaleco_jf13() {
    let h = make_header(8, 8, 0x60, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..4 {
        rom.extend(vec![bank; 0x8000]);
    }
    for bank in 0..8 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 86);

    cart.cpu_write(0x6000, 0x72);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.chr_read(0x0000), 0x06);
}

#[test]
fn load_mapper68_uses_sunsoft4() {
    let h = make_header(8, 32, 0x40, 0x40, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..256usize {
        rom.extend(vec![bank as u8; 0x0400]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 68);

    cart.cpu_write(0xF000, 0x13);
    cart.cpu_write(0x8000, 0x04);
    cart.cpu_write(0xC000, 0x02);
    cart.cpu_write(0xD000, 0x03);
    cart.cpu_write(0xE000, 0x10);
    cart.cpu_write(0x6000, 0xA5);

    assert_eq!(cart.cpu_read(0x6000), 0xA5);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x08);
    assert_eq!(cart.ppu_nametable_read(0x2000, &[0; 0x800]), Some(0x82));
}

#[test]
fn load_mapper87_uses_j87() {
    let h = make_header(2, 4, 0x70, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    rom.extend(vec![0xEA; 2 * PRG_ROM_BANK_SIZE]);
    for bank in 0..4 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 87);

    cart.cpu_write(0x6000, 0x01);
    assert_eq!(cart.chr_read(0x0000), 0x02);
    cart.cpu_write(0x6000, 0x02);
    assert_eq!(cart.chr_read(0x0000), 0x01);
}

#[test]
fn load_mapper89_uses_sunsoft_mapper89() {
    let h = make_header(8, 16, 0x90, 0x50, [0; 8]);
    let mut rom = h.to_vec();
    for bank in 0..8 {
        rom.extend(vec![bank; PRG_ROM_BANK_SIZE]);
    }
    for bank in 0..16 {
        rom.extend(vec![bank; CHR_ROM_BANK_SIZE]);
    }

    let mut cart = Cartridge::load(&rom).unwrap();
    assert_eq!(cart.header().mapper_id, 89);

    cart.cpu_write(0x8000, 0xBA);
    assert_eq!(cart.cpu_read(0x8000), 0x03);
    assert_eq!(cart.cpu_read(0xC000), 0x07);
    assert_eq!(cart.chr_read(0x0000), 0x0A);
    assert_eq!(cart.mirroring(), Mirroring::SingleScreenUpper);
}

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
    cart.cpu_write(0x8000, 0xFF);
    assert_eq!(cart.chr_read(0x0000), 1);
}

#[test]
fn bad_mapper33_sweet_home_translation_uses_mmc1_override() {
    let mut mapper_kind = NesMapper::TaitoTc0190;
    let mut effective_mapper_label = None;

    apply_bad_header_mapper_overrides(
        SWEET_HOME_TRANSLATION_BAD_MAPPER33_CRC32,
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
        &mut mapper_kind,
        &mut effective_mapper_label,
    );

    assert_eq!(mapper_kind, NesMapper::Nrom);
    assert_eq!(effective_mapper_label, Some("NROM (bad mapper 64 header)"));
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

#[test]
fn load_all_zeros_returns_error() {
    assert!(Cartridge::load(&[0u8; 16]).is_err());
}

#[test]
fn load_random_garbage_returns_error() {
    let garbage: Vec<u8> = (0..=255).cycle().take(1024).collect();
    assert!(Cartridge::load(&garbage).is_err());
}
