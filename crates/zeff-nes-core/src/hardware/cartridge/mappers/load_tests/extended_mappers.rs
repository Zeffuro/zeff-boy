use super::common::*;

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
