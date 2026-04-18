use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn build_gb_minimal_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    let mut checksum: u8 = 0;
    for &byte in &rom[0x134..=0x14C] {
        checksum = checksum.wrapping_sub(byte).wrapping_sub(1);
    }
    rom[0x14D] = checksum;
    rom[0x150] = 0x00;
    rom[0x151] = 0x00;
    rom[0x152] = 0x18;
    rom[0x153] = 0xFE;
    rom
}

fn build_nes_minimal_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg] = 0xEA;
    rom[prg + 1] = 0x4C;
    rom[prg + 2] = 0x00;
    rom[prg + 3] = 0x80;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom[prg + 0x3FFA] = 0x00;
    rom[prg + 0x3FFB] = 0x80;
    rom[prg + 0x3FFE] = 0x00;
    rom[prg + 0x3FFF] = 0x80;
    rom
}

#[test]
fn gb_step_frame_produces_framebuffer() {
    let rom = build_gb_minimal_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB ROM should load");

    emu.step_frame();
    let fb = emu.framebuffer();
    assert!(!fb.is_empty(), "framebuffer should not be empty after a frame");
    assert!(
        fb.len() >= 160 * 144 * 4,
        "framebuffer should be at least 160x144x4 bytes"
    );
}

#[test]
fn gb_multiple_frames_advance_cycles() {
    let rom = build_gb_minimal_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB ROM should load");

    let cycles_before = emu.cpu_cycles();
    for _ in 0..10 {
        emu.step_frame();
    }
    let cycles_after = emu.cpu_cycles();
    assert!(
        cycles_after > cycles_before,
        "cycles should advance after stepping frames"
    );
}

#[test]
fn gb_audio_drain_does_not_panic() {
    let rom = build_gb_minimal_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB ROM should load");

    emu.set_sample_rate(48000);
    emu.set_apu_sample_generation_enabled(true);

    for _ in 0..10 {
        emu.step_frame();
        let mut buf = Vec::new();
        emu.drain_audio_samples_into(&mut buf);
    }
}

#[test]
fn gb_framebuffer_deterministic() {
    let rom = build_gb_minimal_rom();

    let mut emu1 =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB ROM should load");
    let mut emu2 =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB ROM should load");

    for _ in 0..10 {
        emu1.step_frame();
        emu2.step_frame();
    }

    assert_eq!(
        emu1.framebuffer(),
        emu2.framebuffer(),
        "two emulators with same ROM should produce identical framebuffers"
    );
    assert_eq!(emu1.cpu_cycles(), emu2.cpu_cycles());
}

#[test]
fn nes_step_frame_produces_framebuffer() {
    let rom = build_nes_minimal_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 48000.0).expect("NES ROM should load");

    emu.step_frame();
    let fb = emu.framebuffer();
    assert!(!fb.is_empty(), "framebuffer should not be empty after a frame");
    assert!(
        fb.len() >= 256 * 240 * 4,
        "framebuffer should be at least 256x240x4 bytes"
    );
}

#[test]
fn nes_multiple_frames_advance_cycles() {
    let rom = build_nes_minimal_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 48000.0).expect("NES ROM should load");

    let cycles_before = emu.cpu_cycles();
    for _ in 0..10 {
        emu.step_frame();
    }
    let cycles_after = emu.cpu_cycles();
    assert!(
        cycles_after > cycles_before,
        "cycles should advance after stepping frames"
    );
}

#[test]
fn nes_audio_produces_samples() {
    let rom = build_nes_minimal_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 48000.0).expect("NES ROM should load");

    emu.set_apu_sample_generation_enabled(true);

    for _ in 0..5 {
        emu.step_frame();
    }

    let buf = emu.drain_audio_samples();
    assert!(
        !buf.is_empty(),
        "audio buffer should have samples after frames with APU enabled"
    );
}

#[test]
fn nes_framebuffer_deterministic() {
    let rom = build_nes_minimal_rom();

    let mut emu1 =
        zeff_nes_core::emulator::Emulator::new(&rom, 48000.0).expect("NES ROM should load");
    let mut emu2 =
        zeff_nes_core::emulator::Emulator::new(&rom, 48000.0).expect("NES ROM should load");

    for _ in 0..10 {
        emu1.step_frame();
        emu2.step_frame();
    }

    assert_eq!(
        emu1.framebuffer(),
        emu2.framebuffer(),
        "two emulators with same ROM should produce identical framebuffers"
    );
    assert_eq!(emu1.cpu_cycles(), emu2.cpu_cycles());
}


