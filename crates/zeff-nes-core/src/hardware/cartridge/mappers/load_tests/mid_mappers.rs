use super::common::*;

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
