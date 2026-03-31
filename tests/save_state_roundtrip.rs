use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn build_gb_test_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

fn build_nes_test_rom() -> Vec<u8> {
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
fn gb_save_state_roundtrip_through_emulator() {
    let rom = build_gb_test_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load");

    for _ in 0..8 {
        let _ = emu.step_instruction();
    }

    let pc_before = emu.cpu_pc();
    let cycles_before = emu.cpu_cycles();

    let state = emu.encode_state_bytes().expect("GB encode should succeed");

    let _ = emu.step_instruction();
    let _ = emu.step_instruction();

    emu.load_state_from_bytes(state)
        .expect("GB decode should succeed");

    assert_eq!(emu.cpu_pc(), pc_before);
    assert_eq!(emu.cpu_cycles(), cycles_before);
}

#[test]
fn nes_save_state_roundtrip_through_emulator() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    for _ in 0..4 {
        emu.step_instruction();
    }

    let pc_before = emu.cpu_pc();
    let cycles_before = emu.cpu_cycles();

    let state = emu.encode_state().expect("NES encode should succeed");

    emu.step_instruction();
    emu.step_instruction();

    emu.load_state_from_bytes(state)
        .expect("NES decode should succeed");

    assert_eq!(emu.cpu_pc(), pc_before);
    assert_eq!(emu.cpu_cycles(), cycles_before);
}
