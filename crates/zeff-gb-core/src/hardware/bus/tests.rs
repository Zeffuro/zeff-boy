use super::*;
use crate::cheats::CheatPatch;
use crate::cheats::CheatValue;
use crate::hardware::ppu::Lcdc;
use crate::hardware::rom_header::RomHeader;

fn make_test_bus() -> Bus {
    let mut rom = vec![0u8; 0x8000];
    for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
        *byte = i as u8;
    }
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize")
}

fn make_cgb_test_bus() -> Bus {
    let mut rom = vec![0u8; 0x8000];
    for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
        *byte = i as u8;
    }
    rom[0x143] = 0x80; // CGB compatible flag
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, HardwareMode::CGBNormal).expect("test bus should initialize")
}

#[test]
fn oam_dma_transfers_one_byte_per_m_cycle() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0xAA;
    bus.oam[1] = 0xBB;
    bus.write_byte(PPU_DMA, 0x00);

    assert!(bus.oam_dma_active);
    assert_eq!(bus.oam[0], 0xAA);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0xAA);
    assert_eq!(bus.oam[1], 0xBB);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0x00);
    assert_eq!(bus.oam[1], 0xBB);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[1], 0x01);
}

#[test]
fn oam_dma_completes_after_160_m_cycles() {
    let mut bus = make_test_bus();
    bus.write_byte(PPU_DMA, 0x00);

    bus.step_oam_dma(8 + (158 * 4));
    assert!(bus.oam_dma_active);

    bus.step_oam_dma(4);
    assert!(!bus.oam_dma_active);
}

#[test]
fn oam_dma_restart_resets_progress_to_byte_zero() {
    let mut bus = make_test_bus();
    bus.write_byte(0xC000, 0x11);
    bus.write_byte(0xC001, 0x22);
    bus.write_byte(0xC100, 0xAA);
    bus.write_byte(0xC101, 0xBB);

    bus.write_byte(PPU_DMA, 0xC0);
    bus.step_oam_dma(8);
    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0x11);
    assert_eq!(bus.oam[1], 0x22);

    bus.write_byte(PPU_DMA, 0xC1);
    assert_eq!(bus.oam_dma_index, 0);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0x11);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[0], 0xAA);
    assert_eq!(bus.oam[1], 0x22);

    bus.step_oam_dma(4);
    assert_eq!(bus.oam[1], 0xBB);
}

#[test]
fn oam_dma_source_reads_ff_from_vram_during_mode_3() {
    let mut bus = make_test_bus();
    bus.vram[0] = 0x5A;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;

    bus.write_byte(PPU_DMA, 0x80);
    bus.step_oam_dma(8);

    assert_eq!(bus.oam[0], 0xFF);
}

#[test]
fn oam_dma_blocks_cpu_access_except_hram() {
    let mut bus = make_test_bus();
    bus.write_byte(PPU_DMA, 0x00);

    assert_eq!(bus.cpu_read_byte(0x0001), 0xFF);
    bus.ie = 0x1F;
    assert_eq!(bus.cpu_read_byte(IE_ADDR), 0xFF);

    bus.cpu_write_byte(0xC000, 0x12);
    assert_ne!(bus.read_byte(0xC000), 0x12);

    bus.cpu_write_byte(IE_ADDR, 0x00);
    assert_eq!(bus.ie, 0x1F);

    bus.cpu_write_byte(HRAM_START, 0x34);
    assert_eq!(bus.cpu_read_byte(HRAM_START), 0x34);
}

#[test]
fn read_rom_bank_0() {
    let bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(0x0000), 0x00);
    assert_eq!(bus.read_byte_raw(0x0010), 0x10);
    assert_eq!(bus.read_byte_raw(0x00FF), 0xFF);
}

#[test]
fn read_wram_bank_0() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0xAB;
    bus.wram[0x0FFF] = 0xCD;
    assert_eq!(bus.read_byte_raw(WRAM_0_START), 0xAB);
    assert_eq!(bus.read_byte_raw(WRAM_0_END), 0xCD);
}

#[test]
fn read_wram_bank_n() {
    let mut bus = make_test_bus();
    bus.wram[WRAM_SIZE] = 0x11;
    bus.wram[WRAM_SIZE + 0x0FFF] = 0x22;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x11);
    assert_eq!(bus.read_byte_raw(WRAM_N_END), 0x22);
}

#[test]
fn read_write_hram() {
    let mut bus = make_test_bus();
    bus.write_byte(HRAM_START, 0xDE);
    bus.write_byte(HRAM_END, 0xAD);
    assert_eq!(bus.read_byte_raw(HRAM_START), 0xDE);
    assert_eq!(bus.read_byte_raw(HRAM_END), 0xAD);
}

#[test]
fn read_write_ie() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(IE_ADDR), 0x00);
    bus.write_byte(IE_ADDR, 0x1F);
    assert_eq!(bus.read_byte_raw(IE_ADDR), 0x1F);
}

#[test]
fn read_not_usable_returns_ff() {
    let bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(NOT_USABLE_START), 0xFF);
    assert_eq!(bus.read_byte_raw(NOT_USABLE_END), 0xFF);
}

#[test]
fn read_write_oam() {
    let mut bus = make_test_bus();
    bus.write_byte(OAM_START, 0x42);
    bus.write_byte(OAM_START + 0x9F, 0x99);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x42);
    assert_eq!(bus.read_byte_raw(OAM_START + 0x9F), 0x99);
}

#[test]
fn read_write_vram() {
    let mut bus = make_test_bus();
    bus.write_byte(VRAM_START, 0xAA);
    bus.write_byte(VRAM_END, 0xBB);
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xAA);
    assert_eq!(bus.read_byte_raw(VRAM_END), 0xBB);
}

#[test]
fn echo_ram_read_mirrors_wram_bank_0() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0x77;
    bus.wram[0x0FFF] = 0x88;
    assert_eq!(bus.read_byte_raw(ECHO_RAM_START), 0x77);
    assert_eq!(bus.read_byte_raw(0xEFFF), 0x88);
}

#[test]
fn echo_ram_read_mirrors_wram_bank_n() {
    let mut bus = make_test_bus();
    bus.wram[WRAM_SIZE] = 0xAA;
    bus.wram[WRAM_SIZE + 0x0DFF] = 0xBB;
    assert_eq!(bus.read_byte_raw(0xF000), 0xAA);
    assert_eq!(bus.read_byte_raw(ECHO_RAM_END), 0xBB);
}

#[test]
fn echo_ram_write_mirrors_to_wram() {
    let mut bus = make_test_bus();
    bus.write_byte(ECHO_RAM_START, 0x55);
    assert_eq!(bus.wram[0], 0x55);
    assert_eq!(bus.read_byte_raw(WRAM_0_START), 0x55);

    bus.write_byte(0xF000, 0x66);
    assert_eq!(bus.wram[WRAM_SIZE], 0x66);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
}

#[test]
fn echo_ram_boundary_bank_0_to_n() {
    let mut bus = make_test_bus();
    bus.wram[0x0FFF] = 0x12;
    assert_eq!(bus.read_byte_raw(0xEFFF), 0x12);
    bus.wram[WRAM_SIZE] = 0x34;
    assert_eq!(bus.read_byte_raw(0xF000), 0x34);
}

#[test]
fn vram_read_returns_ff_during_mode_3() {
    let mut bus = make_test_bus();
    bus.vram[0] = 0x5A;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xFF);
}

#[test]
fn vram_write_blocked_during_mode_3() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.write_byte(VRAM_START, 0xAB);
    assert_eq!(bus.vram[0], 0x00);
}

#[test]
fn vram_accessible_when_lcd_off() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc &= !Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    bus.write_byte(VRAM_START, 0xCC);
    assert_eq!(bus.read_byte_raw(VRAM_START), 0xCC);
}

#[test]
fn oam_read_returns_ff_during_mode_2() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0x42;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
    assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
}

#[test]
fn oam_read_returns_ff_during_mode_3() {
    let mut bus = make_test_bus();
    bus.oam[0] = 0x42;
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
    assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
}

#[test]
fn oam_write_blocked_during_mode_2() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
    bus.write_byte(OAM_START, 0xEE);
    assert_eq!(bus.oam[0], 0x00);
}

#[test]
fn oam_accessible_during_mode_0_and_1() {
    let mut bus = make_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.stat &= !0x03;
    bus.write_byte(OAM_START, 0x11);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x11);
    bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x01;
    bus.write_byte(OAM_START, 0x22);
    assert_eq!(bus.read_byte_raw(OAM_START), 0x22);
}

#[test]
fn cgb_wram_bank_switching() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 1;
    bus.write_byte(WRAM_N_START, 0xAA);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
    bus.wram_bank = 2;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x00);
    bus.write_byte(WRAM_N_START, 0xBB);
    bus.wram_bank = 1;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
    bus.wram_bank = 2;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xBB);
}

#[test]
fn cgb_wram_bank_0_maps_to_bank_1() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 0;
    bus.write_byte(WRAM_N_START, 0xCC);

    bus.wram_bank = 1;
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xCC);
}

#[test]
fn cgb_vram_bank_switching() {
    let mut bus = make_cgb_test_bus();
    bus.vram_bank = 0;
    bus.write_byte(VRAM_START, 0x11);
    bus.vram_bank = 1;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x00);
    bus.write_byte(VRAM_START, 0x22);
    bus.vram_bank = 0;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x11);
    bus.vram_bank = 1;
    assert_eq!(bus.read_byte_raw(VRAM_START), 0x22);
}

#[test]
fn cgb_echo_ram_uses_active_wram_bank() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 3;
    bus.write_byte(WRAM_N_START, 0x55);
    assert_eq!(bus.read_byte_raw(0xF000), 0x55);
    bus.wram_bank = 4;
    assert_eq!(bus.read_byte_raw(0xF000), 0x00);
    bus.write_byte(0xF000, 0x66);
    assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
}

#[test]
fn gdma_transfers_all_blocks_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    for i in 0..0x20u16 {
        bus.wram[i as usize] = (i + 1) as u8;
    }
    let t_cycles = bus.execute_hdma_transfer(0x01);
    assert!(!bus.hdma_active);
    assert_eq!(bus.hdma5, 0xFF);
    assert_eq!(t_cycles, 2 * 32);
    for i in 0..0x20u16 {
        assert_eq!(bus.vram[i as usize], (i + 1) as u8);
    }
}

#[test]
fn gdma_advances_source_and_dest_pointers() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.execute_hdma_transfer(0x00);
    assert_eq!(bus.hdma1, 0xC0);
    assert_eq!(bus.hdma2, 0x10);
    assert_eq!(bus.hdma3, 0x00);
    assert_eq!(bus.hdma4, 0x10);
}

#[test]
fn gdma_double_speed_uses_64_t_per_block() {
    let mut bus = make_cgb_test_bus();
    bus.hardware_mode = HardwareMode::CGBDouble;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    let t_cycles = bus.execute_hdma_transfer(0x03);
    assert_eq!(t_cycles, 4 * 64);
}

#[test]
fn hblank_hdma_setup_does_not_transfer_immediately() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    let t_cycles = bus.execute_hdma_transfer(0x81);

    assert!(bus.hdma_active);
    assert!(bus.hdma_hblank);
    assert_eq!(bus.hdma_blocks_left, 2);
    assert_eq!(t_cycles, 0);
}

#[test]
fn hblank_hdma_transfers_one_block_per_mode_0_transition() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.wram[0] = 0xAA;
    bus.wram[0x10] = 0xBB;
    bus.execute_hdma_transfer(0x81);
    assert_eq!(bus.hdma_blocks_left, 2);
    bus.maybe_step_hblank_hdma(3, 0);
    assert_eq!(bus.hdma_blocks_left, 1);
    assert!(bus.hdma_active);
    assert_eq!(bus.vram[0], 0xAA);
    bus.maybe_step_hblank_hdma(3, 0);
    assert!(!bus.hdma_active);
    assert_eq!(bus.hdma5, 0xFF);
    assert_eq!(bus.vram[0x10], 0xBB);
}

#[test]
fn hblank_hdma_ignores_non_mode_0_transitions() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 0;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.execute_hdma_transfer(0x80);
    let blocks_before = bus.hdma_blocks_left;
    bus.maybe_step_hblank_hdma(0, 2);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
    bus.maybe_step_hblank_hdma(2, 3);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
}

#[test]
fn hblank_hdma_skipped_during_vblank() {
    let mut bus = make_cgb_test_bus();
    bus.io.ppu.lcdc |= Lcdc::LCD_ENABLE;
    bus.io.ppu.ly = 144;
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;

    bus.execute_hdma_transfer(0x80);
    let blocks_before = bus.hdma_blocks_left;

    bus.maybe_step_hblank_hdma(3, 0);
    assert_eq!(bus.hdma_blocks_left, blocks_before);
}

#[test]
fn hblank_hdma_cancel() {
    let mut bus = make_cgb_test_bus();
    bus.hdma1 = 0xC0;
    bus.hdma2 = 0x00;
    bus.hdma3 = 0x00;
    bus.hdma4 = 0x00;
    bus.execute_hdma_transfer(0x83);
    assert!(bus.hdma_active);
    assert!(bus.hdma_hblank);
    let t_cycles = bus.execute_hdma_transfer(0x00);
    assert!(!bus.hdma_active);
    assert!(!bus.hdma_hblank);
    assert_eq!(t_cycles, 0);
    assert_ne!(bus.hdma5 & 0x80, 0);
}

#[test]
fn game_genie_rom_write_overrides_read() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte(0x0010), 0x10);

    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xFF),
    });

    assert_eq!(bus.read_byte(0x0010), 0xFF);
    assert_eq!(bus.read_byte(0x0011), 0x11);
}

#[test]
fn game_genie_rom_write_if_equals_conditional() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
        address: 0x0020,
        value: CheatValue::Constant(0xAA),
        compare: CheatValue::Constant(0x20),
    });

    assert_eq!(bus.read_byte(0x0020), 0xAA);
}

#[test]
fn game_genie_rom_write_if_equals_no_match() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
        address: 0x0020,
        value: CheatValue::Constant(0xAA),
        compare: CheatValue::Constant(0x99),
    });
    assert_eq!(bus.read_byte(0x0020), 0x20);
}

#[test]
fn game_genie_empty_patches_fast_path() {
    let bus = make_test_bus();
    assert!(bus.game_genie_patches.is_empty());
    assert_eq!(bus.read_byte(0x0010), 0x10);
}

#[test]
fn game_genie_non_rom_reads_unaffected() {
    let mut bus = make_test_bus();
    bus.wram[0] = 0x42;
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0xC000,
        value: CheatValue::Constant(0xFF),
    });
    assert_eq!(bus.read_byte(WRAM_0_START), 0x42);
}

#[test]
fn game_genie_multiple_patches_first_match_wins() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xAA),
    });
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xBB),
    });
    assert_eq!(bus.read_byte(0x0010), 0xAA);
}

#[test]
fn read_byte_raw_bypasses_game_genie() {
    let mut bus = make_test_bus();
    bus.game_genie_patches.push(CheatPatch::RomWrite {
        address: 0x0010,
        value: CheatValue::Constant(0xFF),
    });
    assert_eq!(bus.read_byte(0x0010), 0xFF);
    assert_eq!(bus.read_byte_raw(0x0010), 0x10);
}

#[test]
fn write_to_wram_stores_correctly() {
    let mut bus = make_test_bus();
    bus.write_byte(WRAM_0_START, 0x11);
    bus.write_byte(WRAM_0_START + 1, 0x22);
    bus.write_byte(WRAM_N_START, 0x33);
    assert_eq!(bus.wram[0], 0x11);
    assert_eq!(bus.wram[1], 0x22);
    assert_eq!(bus.wram[WRAM_SIZE], 0x33);
}

#[test]
fn write_returns_zero_extra_cycles_for_simple_regions() {
    let mut bus = make_test_bus();
    assert_eq!(bus.write_byte(WRAM_0_START, 0x00), 0);
    assert_eq!(bus.write_byte(HRAM_START, 0x00), 0);
    assert_eq!(bus.write_byte(IE_ADDR, 0x00), 0);
    assert_eq!(bus.write_byte(OAM_START, 0x00), 0);
    assert_eq!(bus.write_byte(VRAM_START, 0x00), 0);
}

#[test]
fn if_register_read_write() {
    let mut bus = make_test_bus();
    assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0xE1);
    bus.if_reg = 0x00;
    assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0x00);
}

#[test]
fn cgb_speed_switch_toggles_hardware_mode() {
    let mut bus = make_cgb_test_bus();
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));

    bus.key1 = (bus.key1 & 0x80) | 0x01 | 0x7E;
    assert!(bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBDouble));

    assert_eq!(bus.key1 & 0x80, 0x80);
    assert_eq!(bus.key1 & 0x01, 0x00);

    bus.key1 = (bus.key1 & 0x80) | 0x01 | 0x7E;
    assert!(bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));
    assert_eq!(bus.key1 & 0x80, 0x00);
}

#[test]
fn cgb_speed_switch_ignored_without_prepare_bit() {
    let mut bus = make_cgb_test_bus();

    assert!(!bus.maybe_switch_cgb_speed());
    assert!(matches!(bus.hardware_mode, HardwareMode::CGBNormal));
}

#[test]
fn cgb_speed_switch_ignored_in_dmg_mode() {
    let mut bus = make_test_bus();
    bus.key1 |= 0x01;
    assert!(!bus.maybe_switch_cgb_speed());
}

#[test]
fn cgb_svbk_read_returns_masked_bank() {
    let mut bus = make_cgb_test_bus();
    bus.wram_bank = 5;
    let val = io_bus::read_io(&bus, CGB_SVBK);
    assert_eq!(val & 0x07, 5);
    assert_eq!(val & 0xF8, 0xF8);
}

#[test]
fn cgb_vbk_read_returns_masked_bank() {
    let mut bus = make_cgb_test_bus();
    bus.vram_bank = 1;
    let val = io_bus::read_io(&bus, PPU_VBK);
    assert_eq!(val, 0xFF);
    bus.vram_bank = 0;
    let val = io_bus::read_io(&bus, PPU_VBK);
    assert_eq!(val, 0xFE);
}

#[test]
fn cgb_key1_write_only_affects_bit_0() {
    let mut bus = make_cgb_test_bus();
    let original_key1 = bus.key1;
    io_bus::write_io(&mut bus, CGB_KEY1, 0xFF);

    assert_eq!(bus.key1 & 0x01, 0x01);
    assert_eq!(bus.key1 & 0x7E, 0x7E);
    assert_eq!(bus.key1 & 0x80, original_key1 & 0x80);
}
