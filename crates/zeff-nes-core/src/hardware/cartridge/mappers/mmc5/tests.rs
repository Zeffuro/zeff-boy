use super::*;
use crate::hardware::cartridge::{ChrFetchKind, Mirroring};

#[test]
fn mmc5_switches_prg_bank_in_mode_3() {
    let mut prg = vec![0u8; 6 * 0x2000];
    for bank in 0..6usize {
        prg[bank * 0x2000] = bank as u8;
    }

    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Vertical, 0x2000, false);
    mapper.cpu_write(0x5100, 3);
    mapper.cpu_write(0x5114, 0x82);

    assert_eq!(mapper.cpu_read(0x8000), 2);
}

#[test]
fn mmc5_irq_raises_when_scanline_matches() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.cpu_write(0x5203, 5);
    mapper.cpu_write(0x5204, 0x80);

    // Simulate 6 scanline boundaries via nametable-read detection.
    // Each "scanline" requires: a CHR read to reset the counter,
    // then 3 consecutive nametable reads to trigger the tick.
    for _ in 0..6 {
        mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
        mapper.ppu_nametable_read(0x2000, &ciram);
        mapper.ppu_nametable_read(0x2000, &ciram);
        mapper.ppu_nametable_read(0x2000, &ciram);
    }

    assert!(mapper.irq_pending());
}

#[test]
fn mmc5_separates_bg_and_sprite_chr_banks() {
    let prg = vec![0u8; 4 * 0x2000];
    let mut chr = vec![0u8; 16 * 0x0400];
    chr[2 * 0x0400] = 0x22;
    chr[9 * 0x0400] = 0x99;

    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    mapper.cpu_write(0x5101, 3);
    mapper.cpu_write(0x5120, 2);
    mapper.cpu_write(0x5128, 9);

    assert_eq!(mapper.chr_read_kind(0x0000, ChrFetchKind::Sprite), 0x22);
    assert_eq!(mapper.chr_read_kind(0x0000, ChrFetchKind::Background), 0x99);
}

#[test]
fn mmc5_status_read_acknowledges_irq_pending() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.cpu_write(0x5203, 1);
    mapper.cpu_write(0x5204, 0x80);

    mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);

    assert!(mapper.irq_pending());
    let status = mapper.cpu_read(0x5204);
    assert_ne!(status & 0x80, 0);
    assert!(!mapper.irq_pending());
}

#[test]
fn mmc5_scanline_counter_resets_on_prerender_notification() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.current_scanline = 240;

    // Simulate a scanline boundary via nametable reads:should reset to 0
    mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);

    assert_eq!(mapper.current_scanline, 0);
    assert!(mapper.in_frame);
}

#[test]
fn mmc5_nametable_modes_support_exram_and_fill() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let mut ciram = [0u8; 0x800];

    mapper.cpu_write(0x5105, 0b11_10_01_00);
    mapper.cpu_write(0x5104, 0);
    mapper.cpu_write(0x5106, 0xAB);
    mapper.cpu_write(0x5107, 0x02);

    ciram[0x000] = 0x10;
    ciram[0x400] = 0x20;
    mapper.ex_ram[0x000] = 0x30;

    assert_eq!(mapper.ppu_nametable_read(0x2000, &ciram), Some(0x10));
    assert_eq!(mapper.ppu_nametable_read(0x2400, &ciram), Some(0x20));
    assert_eq!(mapper.ppu_nametable_read(0x2800, &ciram), Some(0x30));
    assert_eq!(mapper.ppu_nametable_read(0x2C00, &ciram), Some(0xAB));
    assert_eq!(mapper.ppu_nametable_read(0x2FC0, &ciram), Some(0xAA));

    assert!(mapper.ppu_nametable_write(0x2000, 0x11, &mut ciram));
    assert!(mapper.ppu_nametable_write(0x2400, 0x22, &mut ciram));
    assert!(mapper.ppu_nametable_write(0x2800, 0x33, &mut ciram));
    assert!(mapper.ppu_nametable_write(0x2C00, 0x44, &mut ciram));

    assert_eq!(ciram[0x000], 0x11);
    assert_eq!(ciram[0x400], 0x22);
    assert_eq!(mapper.ex_ram[0x000], 0x33);
}

#[test]
fn mmc5_hardware_multiplier() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);

    mapper.cpu_write(0x5205, 20);
    mapper.cpu_write(0x5206, 13);

    assert_eq!(mapper.cpu_read(0x5205), 0x04);
    assert_eq!(mapper.cpu_read(0x5206), 0x01);

    mapper.cpu_write(0x5205, 0xFF);
    mapper.cpu_write(0x5206, 0xFF);
    assert_eq!(mapper.cpu_read(0x5205), 0x01);
    assert_eq!(mapper.cpu_read(0x5206), 0xFE);
}

#[test]
fn mmc5_exram_cpu_read_returns_zero_in_mode_0_and_1() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);

    mapper.ex_ram[0] = 0xAB;

    mapper.cpu_write(0x5104, 0);
    assert_eq!(mapper.cpu_read(0x5C00), 0);

    mapper.cpu_write(0x5104, 1);
    assert_eq!(mapper.cpu_read(0x5C00), 0);

    mapper.cpu_write(0x5104, 2);
    assert_eq!(mapper.cpu_read(0x5C00), 0xAB);

    mapper.cpu_write(0x5104, 3);
    assert_eq!(mapper.cpu_read(0x5C00), 0xAB);
}

#[test]
fn mmc5_exram_mode3_blocks_writes() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);

    mapper.cpu_write(0x5104, 2);
    mapper.cpu_write(0x5C00, 0x42);
    assert_eq!(mapper.ex_ram[0], 0x42);

    mapper.cpu_write(0x5104, 3);
    mapper.cpu_write(0x5C00, 0xFF);
    assert_eq!(mapper.ex_ram[0], 0x42);
}

#[test]
fn mmc5_writing_5204_does_not_clear_irq_pending() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.cpu_write(0x5203, 1);
    mapper.cpu_write(0x5204, 0x80);

    mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);
    assert!(mapper.irq_pending());

    mapper.cpu_write(0x5204, 0x80);
    assert!(mapper.irq_pending());

    let _ = mapper.cpu_read(0x5204);
    assert!(!mapper.irq_pending());
}

#[test]
fn mmc5_prg_ram_bank_register_5113() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let prg_ram_size = 4 * 0x2000;
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, prg_ram_size, false);

    mapper.cpu_write(0x5102, 0x02);
    mapper.cpu_write(0x5103, 0x01);

    mapper.cpu_write(0x5113, 0);
    mapper.cpu_write(0x6000, 0xAA);

    mapper.cpu_write(0x5113, 2);
    mapper.cpu_write(0x6000, 0xBB);

    mapper.cpu_write(0x5113, 0);
    assert_eq!(mapper.cpu_read(0x6000), 0xAA);

    mapper.cpu_write(0x5113, 2);
    assert_eq!(mapper.cpu_read(0x6000), 0xBB);
}

#[test]
fn mmc5_exram_mode1_extended_attribute() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.cpu_write(0x5104, 1);
    mapper.cpu_write(0x5105, 0x00);

    mapper.ex_ram[0] = 0xC5;

    let _tile = mapper.ppu_nametable_read(0x2000, &ciram);

    let attr = mapper.ppu_nametable_read(0x23C0, &ciram);
    assert_eq!(attr, Some(0xFF));

    mapper.ex_ram[1] = 0x45;
    let _tile = mapper.ppu_nametable_read(0x2001, &ciram);
    let attr = mapper.ppu_nametable_read(0x23C0, &ciram);
    assert_eq!(attr, Some(0x55));
}

#[test]
fn mmc5_exram_mode1_chr_bank_override() {
    let prg = vec![0u8; 4 * 0x2000];
    let mut chr = vec![0u8; 64 * 0x0400];
    chr[20 * 0x0400] = 0xDE;

    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);

    mapper.cpu_write(0x5104, 1);
    mapper.cpu_write(0x5130, 0);

    mapper.exram_tile_byte = 0x05;

    let val = mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
    assert_eq!(val, 0xDE);

    let val_sprite = mapper.chr_read_kind(0x0000, ChrFetchKind::Sprite);
    assert_ne!(val_sprite, 0xDE);
}

#[test]
fn mmc5_compare_zero_does_not_fire_on_prerender() {
    let prg = vec![0u8; 4 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Mmc5::new(prg, chr, Mirroring::Horizontal, 0x2000, false);
    let ciram = [0u8; 0x800];

    mapper.cpu_write(0x5203, 0);
    mapper.cpu_write(0x5204, 0x80);

    mapper.current_scanline = 240;
    mapper.chr_read_kind(0x0000, ChrFetchKind::Background);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);
    mapper.ppu_nametable_read(0x2000, &ciram);

    assert!(!mapper.irq_pending());
}
