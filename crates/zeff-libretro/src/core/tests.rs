use super::*;
use zeff_emu_common::memory::MemoryRegionKind;

fn load_sega8(ext: &str) -> CoreState {
    CoreState::from_rom(&[0x76], &format!("test.{ext}")).expect("Sega 8-bit ROM should load")
}

fn ws_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[0..2].copy_from_slice(&[0x90, 0xF4]);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn nes_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0xEA;
    rom[prg + 5] = 0xEA;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

#[test]
fn sega8_extensions_select_expected_systems() {
    let sms = load_sega8("sms");
    assert!(matches!(sms.core, ActiveCore::Sega8(_)));
    assert_eq!(sms.system_label(), "SMS");
    assert_eq!(sms.native_width(), 256);
    assert_eq!(sms.native_height(), 192);
    assert_eq!(sms.sram_size(), 0);
    assert_eq!(
        sms.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );

    let gg = load_sega8("gg");
    assert!(matches!(gg.core, ActiveCore::Sega8(_)));
    assert_eq!(gg.system_label(), "Game Gear");
    assert_eq!(gg.native_width(), 160);
    assert_eq!(gg.native_height(), 144);
    assert_eq!(gg.sram_size(), 0);
    assert_eq!(
        gg.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );

    for ext in ["sg", "sc"] {
        let sg = load_sega8(ext);
        assert!(matches!(sg.core, ActiveCore::Sega8(_)));
        assert_eq!(sg.system_label(), "SG-1000/SC-3000");
        assert_eq!(sg.native_width(), 256);
        assert_eq!(sg.native_height(), 192);
        assert_eq!(sg.sram_size(), 0);
        assert_eq!(sg.memory_region_size(MemoryRegionKind::PaletteRam), 0);
    }
}

#[test]
fn gba_memory_regions_include_debuggable_video_side_regions() {
    let rom = gba_rom();
    let state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");
    let regions = state.memory_regions();

    assert_eq!(
        state.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_gba_core::hardware::constants::PALETTE_RAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::Oam),
        zeff_gba_core::hardware::constants::OAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::IoRegisters),
        zeff_gba_core::hardware::constants::IO_SIZE
    );
    assert!(regions.iter().any(|region| region.id == "palette_ram"));
    assert!(regions.iter().any(|region| region.id == "oam"));
    assert!(regions.iter().any(|region| region.id == "io_registers"));
}

#[test]
fn nes_memory_regions_include_ppu_palette_and_oam() {
    let rom = nes_rom();
    let state = CoreState::from_rom(&rom, "test.nes").expect("NES ROM should load");

    assert_eq!(state.memory_region_size(MemoryRegionKind::PaletteRam), 32);
    assert_eq!(state.memory_region_size(MemoryRegionKind::Oam), 256);
    assert_eq!(state.memory_region_size(MemoryRegionKind::IoRegisters), 0);
}

#[test]
fn libretro_valid_extensions_include_gba_and_sega8() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for ext in [
        "gb", "gbc", "gba", "nes", "ws", "wsc", "sms", "gg", "sg", "sc",
    ] {
        assert!(
            extensions.split('|').any(|entry| entry == ext),
            "missing extension: {ext}"
        );
    }
}

#[test]
fn wonderswan_extensions_select_ws_core() {
    let rom = ws_rom();
    for ext in ["ws", "wsc"] {
        let mut state =
            CoreState::from_rom(&rom, &format!("test.{ext}")).expect("WonderSwan ROM should load");

        assert!(matches!(state.core, ActiveCore::Ws(_)));
        assert_eq!(state.system_label(), "WonderSwan");
        assert_eq!(state.native_width(), 224);
        assert_eq!(state.native_height(), 144);
        assert_eq!(state.video_ram_size(), state.system_ram_size());
        assert!(state.video_ram_size() > 0);
        let regions = state.memory_regions();
        assert_eq!(
            regions
                .iter()
                .find(|region| region.id == "cpu")
                .map(|region| region.address_bits),
            Some(Some(20))
        );
        assert!(regions.iter().any(|region| {
            region.kind == MemoryRegionKind::VideoRam && region.size == Some(state.video_ram_size())
        }));
        state.refresh_video_ram();
        assert_eq!(state.video_ram_buf.len(), state.video_ram_size());
        assert!(state.encode_state().is_ok());
    }
}
