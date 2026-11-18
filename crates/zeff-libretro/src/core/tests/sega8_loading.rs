use super::*;

#[test]
fn sega8_extensions_select_expected_systems() {
    let mut sms = load_sega8("sms");
    assert!(matches!(sms.core, ActiveCore::Sega8(_)));
    assert_eq!(sms.system_label(), "SMS");
    assert_eq!(sms.video_geometry().base_width, 256);
    assert_eq!(sms.video_geometry().base_height, 192);
    assert_eq!(sms.sram_size(), 0);
    assert_eq!(
        sms.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );
    assert_copyable_regions(&mut sms);

    let mut gg = load_sega8("gg");
    assert!(matches!(gg.core, ActiveCore::Sega8(_)));
    assert_eq!(gg.system_label(), "Game Gear");
    assert_eq!(gg.video_geometry().base_width, 160);
    assert_eq!(gg.video_geometry().base_height, 144);
    assert_eq!(gg.sram_size(), 0);
    assert_eq!(
        gg.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );
    assert_copyable_regions(&mut gg);

    for ext in ["sg", "sc"] {
        let mut sg = load_sega8(ext);
        assert!(matches!(sg.core, ActiveCore::Sega8(_)));
        assert_eq!(sg.system_label(), "SG-1000/SC-3000");
        assert_eq!(sg.video_geometry().base_width, 256);
        assert_eq!(sg.video_geometry().base_height, 192);
        assert_eq!(sg.sram_size(), 0);
        assert_eq!(sg.memory_region_size(MemoryRegionKind::PaletteRam), 0);
        assert_copyable_regions(&mut sg);
    }
}

#[test]
fn sega8_libretro_uses_path_region_for_video_standard_and_fps() {
    let state =
        CoreState::from_rom(&[0x76], "Example (Europe).sms").expect("Sega 8-bit ROM should load");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(
        emu.video_standard(),
        zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal
    );
    assert_eq!(
        emu.console_region(),
        zeff_sega8_core::hardware::region::Sega8Region::Export
    );
    assert_eq!(state.fps(), 50.0);
    assert!(state.is_pal_region());
}

#[test]
fn sega8_libretro_applies_explicit_mapper_path_tag() {
    let state = CoreState::from_rom(&[0x76], "Example [mapper=janggun].sms")
        .expect("Sega 8-bit ROM should load");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(
        emu.bus().mapper().kind(),
        zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Janggun
    );
}

#[test]
fn sega8_libretro_forwards_player_two_input() {
    let mut state = load_sega8("sms");

    state.set_input_p2(0x01, 0x04);

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    let raw = emu
        .bus()
        .input()
        .read_controller(zeff_sega8_core::hardware::input::ControllerPort::Two);
    assert_eq!(raw & (1 << 4), 0, "host A should map to SMS button 1 on P2");
    assert_eq!(raw & (1 << 0), 0, "host Up should map to SMS up on P2");
    assert_ne!(raw & (1 << 5), 0);
}
