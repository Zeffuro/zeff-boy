use super::*;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::constants::STATUS_SPRITE_OVERFLOW;
use crate::hardware::timing::NesTiming;

fn test_bus() -> Bus {
    test_bus_with_timing(NesTiming::Ntsc)
}

fn test_bus_with_timing(timing: NesTiming) -> Bus {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let cart = Cartridge::load(&rom).expect("test ROM should load");
    Bus::new_with_timing(cart, 44_100.0, timing)
}

fn mmc3_test_bus() -> Bus {
    let mut rom = vec![0u8; 16 + 2 * 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 1;
    rom[6] = 0x40;

    let cart = Cartridge::load(&rom).expect("test ROM should load");
    Bus::new(cart, 44_100.0)
}

fn enable_mmc3_irq(bus: &mut Bus) {
    bus.cartridge.cpu_write(0xC000, 0);
    bus.cartridge.cpu_write(0xC001, 0);
    bus.cartridge.cpu_write(0xE001, 0);
    bus.ppu.regs.mask = 0x18;
}

fn run_sprite_evaluation(bus: &mut Bus, scanline: u16, dots: std::ops::RangeInclusive<u16>) {
    bus.ppu.scanline = scanline;
    for dot in dots {
        bus.ppu.dot = dot;
        bus.ppu_render_dot();
    }
}

#[test]
fn sprite_evaluation_clears_secondary_oam_on_even_dots() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.secondary_oam = [0; 32];

    run_sprite_evaluation(&mut bus, 0, 1..=1);
    assert_eq!(bus.ppu.secondary_oam[0], 0);

    run_sprite_evaluation(&mut bus, 0, 2..=2);
    assert_eq!(bus.ppu.secondary_oam[0], 0xFF);
    assert_eq!(bus.ppu.secondary_oam[1], 0);
}

#[test]
fn sprite_evaluation_rejects_a_sprite_in_two_dots() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];

    run_sprite_evaluation(&mut bus, 20, 65..=66);

    assert_eq!(bus.ppu.sprite_eval_oam_addr, 4);
    assert_eq!(bus.ppu.sprite_eval_secondary_addr, 0);
}

#[test]
fn sprite_evaluation_copies_an_in_range_sprite_in_eight_dots() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    bus.ppu.oam[..4].copy_from_slice(&[20, 3, 2, 44]);

    run_sprite_evaluation(&mut bus, 20, 65..=72);

    assert_eq!(&bus.ppu.secondary_oam[..4], &[20, 3, 2, 44]);
    assert_eq!(bus.ppu.sprite_eval_oam_addr, 4);
    assert_eq!(bus.ppu.sprite_eval_secondary_addr, 4);
    assert!(bus.ppu.sprite_eval_sprite_zero);
}

#[test]
fn sprite_overflow_is_set_on_the_ninth_sprite_even_dot() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    for sprite in 0..9 {
        bus.ppu.oam[sprite * 4] = 20;
    }

    run_sprite_evaluation(&mut bus, 20, 65..=129);
    assert_eq!(bus.ppu.regs.status & STATUS_SPRITE_OVERFLOW, 0);

    run_sprite_evaluation(&mut bus, 20, 130..=130);
    assert_ne!(bus.ppu.regs.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn last_visible_scanline_still_evaluates_y_239_for_overflow() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    for sprite in 0..9 {
        bus.ppu.oam[sprite * 4] = 239;
    }

    run_sprite_evaluation(&mut bus, 239, 1..=130);

    assert_ne!(bus.ppu.regs.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn full_secondary_oam_uses_the_diagonal_overflow_increment() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    for sprite in 0..8 {
        bus.ppu.oam[sprite * 4] = 20;
    }

    run_sprite_evaluation(&mut bus, 20, 65..=130);

    assert_eq!(bus.ppu.sprite_eval_oam_addr, 9 * 4 + 1);
    assert_eq!(bus.ppu.regs.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn sprite_evaluation_prepares_next_visible_scanline_at_dot_257() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    bus.ppu.oam[0] = 4;
    bus.ppu.oam[1] = 0x12;
    bus.ppu.oam[2] = 0x01;
    bus.ppu.oam[3] = 24;

    run_sprite_evaluation(&mut bus, 4, 1..=257);
    assert_eq!(bus.ppu.sprite_count, 1);
    assert_eq!(bus.ppu.sprite_attribs[0], 0x01);
    assert_eq!(bus.ppu.sprite_x_counters[0], 24);

    bus.ppu.dot = 321;
    bus.ppu_render_dot();
    assert_eq!(bus.ppu.sprite_x_counters[0], 24);
}

fn nonzero_sprite_bus() -> Bus {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    rom[16 + 0x4000..].fill(0xFF);
    Bus::new(Cartridge::load(&rom).unwrap(), 44_100.0)
}

#[test]
fn late_rendering_enable_keeps_stale_sprite_slots_transparent() {
    let mut bus = nonzero_sprite_bus();
    bus.ppu.regs.mask = 0x18;
    bus.ppu.oam = [0xFF; 256];
    bus.ppu.oam[..4].copy_from_slice(&[68, 0x26, 0xA2, 100]);
    run_sprite_evaluation(&mut bus, 68, 1..=72);
    assert_eq!(bus.ppu.sprite_eval_secondary_addr, 4);

    assert_eq!(bus.ppu_bus_read(0x264), 0xFF);
    assert_eq!(bus.ppu_bus_read(0x26C), 0xFF);
    bus.ppu.dot = 73;
    bus.cpu_write(0x2001, 0);
    for _ in 0..3 {
        bus.ppu_render_dot();
        bus.ppu.tick();
    }
    assert!(!bus.ppu.rendering_enabled());
    run_sprite_evaluation(&mut bus, 199, 1..=248);
    bus.ppu.dot = 249;
    bus.cpu_write(0x2001, 0x1E);
    assert!(!bus.ppu.rendering_enabled());
    for _ in 0..3 {
        bus.ppu_render_dot();
        bus.ppu.tick();
    }
    assert!(bus.ppu.rendering_enabled());
    run_sprite_evaluation(&mut bus, 199, 252..=257);

    assert_eq!(bus.ppu.sprite_count, 1);
    assert_eq!(bus.ppu.sprite_patterns_lo[0], 0);
    assert_eq!(bus.ppu.sprite_patterns_hi[0], 0);
    assert_eq!(bus.ppu.sprite_x_counters[0], 100);
    assert_eq!(bus.ppu.sprite_attribs[0], 0xA2);
    assert!(bus.ppu.sprite_zero_rendering);
}

#[test]
fn sprite_size_change_after_evaluation_preserves_bounded_fetch() {
    for attributes in [0, SPRITE_ATTR_FLIP_VERTICAL] {
        let mut bus = nonzero_sprite_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.regs.ctrl = 0x20;
        bus.ppu.oam = [0xFF; 256];
        bus.ppu.oam[..4].copy_from_slice(&[10, 0xFF, attributes, 44]);
        run_sprite_evaluation(&mut bus, 20, 1..=256);
        assert_eq!(bus.ppu.sprite_eval_secondary_addr, 4);

        bus.cpu_write(0x2000, 0);
        run_sprite_evaluation(&mut bus, 20, 257..=257);
        assert_eq!(bus.ppu.sprite_count, 1);
        assert_eq!(bus.ppu.sprite_patterns_lo[0], 0);
        assert_eq!(bus.ppu.sprite_patterns_hi[0], 0);
        assert!(!bus.sprite_fetch_a12[0]);
        assert_eq!(bus.ppu.sprite_x_counters[0], 44);
    }
}

#[test]
fn invalid_sprite_slot_does_not_replace_the_following_valid_slot() {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    rom[16 + 0x4000 + 0x120] = 0x81;
    rom[16 + 0x4000 + 0x128] = 0x42;
    rom[16 + 0x4000 + 0xFF3] = 0xA5;
    rom[16 + 0x4000 + 0xFFB] = 0x5A;
    let mut bus = Bus::new(Cartridge::load(&rom).unwrap(), 44_100.0);
    bus.ppu.regs.mask = 0x18;
    bus.ppu.secondary_oam[..8].copy_from_slice(&[0, 0xFF, 0x80, 100, 20, 0x12, 1, 24]);
    bus.ppu.sprite_eval_secondary_addr = 8;
    bus.ppu.sprite_eval_sprite_zero = true;
    assert_eq!(bus.ppu_bus_read(0xFF3), 0xA5);
    assert_eq!(bus.ppu_bus_read(0xFFB), 0x5A);
    run_sprite_evaluation(&mut bus, 20, 257..=257);

    assert_eq!(bus.ppu.sprite_count, 2);
    assert_eq!(&bus.ppu.sprite_patterns_lo[..2], &[0, 0x81]);
    assert_eq!(&bus.ppu.sprite_patterns_hi[..2], &[0, 0x42]);
    assert_eq!(&bus.ppu.sprite_x_counters[..2], &[100, 24]);
    assert_eq!(&bus.ppu.sprite_attribs[..2], &[0x80, 1]);
    assert!(!bus.sprite_fetch_a12[0]);
    assert!(bus.ppu.sprite_zero_rendering);
}

#[test]
fn background_prefetch_dot_337_aligns_first_visible_pixel() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x0A;
    bus.ppu.palette_ram[0] = 0x0F;
    bus.ppu.palette_ram[1] = 0x12;
    bus.ppu.scanline = NesTiming::Ntsc.pre_render_scanline();
    bus.ppu.dot = 337;
    bus.ppu.bg_shift_pattern_lo = 0x4000;
    bus.ppu.bg_shift_pattern_hi = 0;
    bus.ppu.bg_shift_attrib_lo = 0;
    bus.ppu.bg_shift_attrib_hi = 0;
    bus.ppu.bg_next_tile_lo = 0;
    bus.ppu.bg_next_tile_hi = 0;
    bus.ppu.bg_next_tile_attrib = 0;

    bus.ppu_render_dot();

    bus.ppu.scanline = 0;
    bus.ppu.dot = 1;
    bus.ppu_render_dot();

    assert_eq!(&bus.ppu.framebuffer[0..4], &[48, 50, 236, 0xFF]);
}

#[test]
fn pal_and_dendy_run_pre_render_sprite_reset_and_vertical_copy() {
    for timing in [NesTiming::Pal, NesTiming::Dendy] {
        let mut bus = test_bus_with_timing(timing);
        bus.ppu.regs.mask = 0x18;
        bus.ppu.scanline = timing.pre_render_scanline();
        bus.ppu.sprite_count = 3;
        bus.ppu.dot = SPRITE_EVALUATION_DOT;
        bus.ppu_render_dot();
        assert_eq!(bus.ppu.sprite_count, 0);

        bus.ppu.v = 0;
        bus.ppu.t = 0x7BE0;
        bus.ppu.dot = VERTICAL_COPY_START_DOT;
        bus.ppu_render_dot();
        assert_eq!(bus.ppu.v, 0x7BE0);
    }
}

#[test]
fn background_reload_dot_aligns_loaded_tile_left_edge() {
    let mut bus = test_bus();
    bus.ppu.regs.mask = 0x0A;
    bus.ppu.palette_ram[0] = 0x0F;
    bus.ppu.palette_ram[1] = 0x12;
    bus.ppu.scanline = 0;
    bus.ppu.bg_next_tile_lo = 0x80;
    bus.ppu.bg_next_tile_hi = 0;
    bus.ppu.bg_next_tile_attrib = 0;

    for dot in 121..=129 {
        bus.ppu.dot = dot;
        bus.ppu_render_dot();
    }

    let pixel_127 = 127 * 4;
    let pixel_128 = 128 * 4;
    assert_eq!(
        &bus.ppu.framebuffer[pixel_127..pixel_127 + 4],
        &[0, 0, 0, 0xFF]
    );
    assert_eq!(
        &bus.ppu.framebuffer[pixel_128..pixel_128 + 4],
        &[48, 50, 236, 0xFF]
    );
}

#[test]
fn mmc3_sprite_table_edge_clocks_at_sprite_address_setup() {
    let mut bus = mmc3_test_bus();
    enable_mmc3_irq(&mut bus);
    bus.ppu.regs.ctrl = 0x08;

    for dot in 249..=257 {
        bus.ppu.dot = dot;
        bus.ppu_cycles = u64::from(dot);
        bus.ppu_render_dot();
    }
    assert!(!bus.cartridge.irq_pending());

    bus.ppu.dot = 258;
    bus.ppu_cycles = 258;
    bus.ppu_render_dot();
    assert!(bus.cartridge.irq_pending());
}

#[test]
fn mmc3_background_table_edge_clocks_during_prefetch() {
    let mut bus = mmc3_test_bus();
    enable_mmc3_irq(&mut bus);
    bus.ppu.regs.ctrl = 0x10;
    bus.ppu.scanline = NesTiming::Ntsc.pre_render_scanline();

    for dot in 313..=321 {
        bus.ppu.dot = dot;
        bus.ppu_cycles = u64::from(dot);
        bus.ppu_render_dot();
    }
    assert!(!bus.cartridge.irq_pending());

    bus.ppu.dot = 322;
    bus.ppu_cycles = 322;
    bus.ppu_render_dot();
    assert!(bus.cartridge.irq_pending());
}

#[test]
fn mmc3_tall_sprite_uses_tile_bank_for_a12() {
    let mut low_bank = mmc3_test_bus();
    enable_mmc3_irq(&mut low_bank);
    low_bank.ppu.regs.ctrl = 0x20;
    low_bank.ppu.oam = [0xFF; 256];
    low_bank.ppu.oam[0] = 0;
    low_bank.ppu.oam[1] = 0;
    for dot in 249..=258 {
        low_bank.ppu.dot = dot;
        low_bank.ppu_cycles = u64::from(dot);
        low_bank.ppu_render_dot();
    }
    assert!(!low_bank.cartridge.irq_pending());

    let mut high_bank = mmc3_test_bus();
    enable_mmc3_irq(&mut high_bank);
    high_bank.ppu.regs.ctrl = 0x20;
    high_bank.ppu.oam = [0xFF; 256];
    high_bank.ppu.oam[0] = 0;
    high_bank.ppu.oam[1] = 1;
    for dot in 249..=258 {
        high_bank.ppu.dot = dot;
        high_bank.ppu_cycles = u64::from(dot);
        high_bank.ppu_render_dot();
    }
    assert!(high_bank.cartridge.irq_pending());
}
