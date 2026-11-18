use super::*;

#[test]
fn gba_memory_regions_include_debuggable_video_side_regions() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");
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
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::ExternalWorkRam),
        zeff_gba_core::hardware::constants::EWRAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::InternalWorkRam),
        zeff_gba_core::hardware::constants::IWRAM_SIZE
    );
    assert!(regions.iter().any(|region| region.id == "ewram"));
    assert!(regions.iter().any(|region| region.id == "iwram"));
    assert!(regions.iter().any(|region| region.id == "palette_ram"));
    assert!(regions.iter().any(|region| region.id == "oam"));
    assert!(regions.iter().any(|region| region.id == "io_registers"));
    assert_copyable_regions(&mut state);
}

#[test]
fn nes_memory_regions_include_ppu_palette_and_oam() {
    let rom = nes_rom();
    let mut state = CoreState::from_rom(&rom, "test.nes").expect("NES ROM should load");

    assert_eq!(state.memory_region_size(MemoryRegionKind::PaletteRam), 32);
    assert_eq!(state.memory_region_size(MemoryRegionKind::Oam), 256);
    assert_eq!(state.memory_region_size(MemoryRegionKind::IoRegisters), 0);
    assert_copyable_regions(&mut state);
}

#[test]
fn gb_memory_regions_are_copyable_by_descriptor() {
    let rom = gb_rom();
    let mut state = CoreState::from_rom(&rom, "test.gb").expect("GB ROM should load");

    assert!(state.memory_region_size(MemoryRegionKind::SystemRam) > 0);
    assert!(state.memory_region_size(MemoryRegionKind::VideoRam) > 0);
    assert_copyable_regions(&mut state);
}

#[test]
fn libretro_valid_extensions_include_registered_systems() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for ext in [
        "gb", "gbc", "sgb", "gba", "nes", "fds", "pce", "ws", "wsc", "sms", "gg", "sg", "sc",
    ] {
        assert!(
            extensions.split('|').any(|entry| entry == ext),
            "missing extension: {ext}"
        );
    }
}

#[test]
fn libretro_rejects_deferred_coleco_roms() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");
    assert!(!extensions.split('|').any(|entry| entry == "col"));

    let error = CoreState::from_rom(&[0xAA, 0x55], "test.col")
        .err()
        .expect("ColecoVision should have an explicit libretro error");
    assert_eq!(
        error.to_string(),
        "ColecoVision is not available in zeff-libretro"
    );
}

#[test]
fn codemasters_save_ram_region_uses_mapper_visible_eight_kilobytes() {
    let mut state = CoreState::from_rom(&codemasters_rom(4), "codemasters.sms")
        .expect("Codemasters ROM should load");
    let mut copied = Vec::new();

    let region = state
        .copy_memory_region("save_ram", &mut copied)
        .expect("Codemasters mapper RAM should be copyable");

    assert_eq!(state.save_ram_kind().size(), 0x2000);
    assert_eq!(region.size, Some(0x2000));
    assert_eq!(copied.len(), 0x2000);
}

#[test]
fn wonderswan_extensions_select_ws_core() {
    let rom = ws_rom();
    for ext in ["ws", "wsc"] {
        let mut state =
            CoreState::from_rom(&rom, &format!("test.{ext}")).expect("WonderSwan ROM should load");

        assert!(matches!(state.core, ActiveCore::Ws(_)));
        assert_eq!(state.system_label(), "WonderSwan");
        assert_eq!(state.video_geometry().base_width, 224);
        assert_eq!(state.video_geometry().base_height, 144);
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
        assert_copyable_regions(&mut state);
        assert!(state.encode_state().is_ok());
    }
}
