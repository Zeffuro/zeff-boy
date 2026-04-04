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
    rom[prg + 4] = 0x4C;
    rom[prg + 5] = 0x04;
    rom[prg + 6] = 0x80;

    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom[prg + 0x3FFA] = 0x04;
    rom[prg + 0x3FFB] = 0x80;
    rom[prg + 0x3FFE] = 0x04;
    rom[prg + 0x3FFF] = 0x80;
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

#[test]
fn gb_rewind_roundtrip() {
    let rom = build_gb_test_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load");

    let mut rewind = zeff_emu_common::rewind::RewindBuffer::new(10, 1);

    for _ in 0..4 {
        let _ = emu.step_instruction();
    }

    let state = emu.encode_state_bytes().expect("encode should succeed");
    let framebuffer = emu.framebuffer().to_vec();
    let pc_snapshot = emu.cpu_pc();
    let cycles_snapshot = emu.cpu_cycles();

    rewind.push(&state, &framebuffer);
    assert_eq!(rewind.len(), 1);

    for _ in 0..8 {
        let _ = emu.step_instruction();
    }
    assert_ne!(emu.cpu_pc(), pc_snapshot);

    let frame = rewind.pop().expect("rewind pop should return a snapshot");
    emu.load_state_from_bytes(frame.state_bytes)
        .expect("load should succeed");

    assert_eq!(emu.cpu_pc(), pc_snapshot);
    assert_eq!(emu.cpu_cycles(), cycles_snapshot);
    assert!(rewind.is_empty());
}

#[test]
fn nes_audio_drain_produces_samples() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    emu.step_frame();

    let samples = emu.drain_audio_samples();
    assert!(
        !samples.is_empty(),
        "stepping one NES frame should produce audio samples"
    );

    let second_drain = emu.drain_audio_samples();
    assert!(
        second_drain.is_empty(),
        "draining again without stepping should produce no samples"
    );
}

#[test]
fn nes_framebuffer_is_correct_size() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    emu.step_frame();

    let fb = emu.framebuffer();
    assert_eq!(
        fb.len(),
        256 * 240 * 4,
        "NES framebuffer should be 256x240 RGBA"
    );
}

#[test]
fn gb_framebuffer_is_correct_size() {
    let rom = build_gb_test_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load");

    emu.step_frame();

    let fb = emu.framebuffer();
    assert_eq!(
        fb.len(),
        160 * 144 * 4,
        "GB framebuffer should be 160x144 RGBA"
    );
}

fn build_gb_apu_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100] = 0x3E;
    rom[0x0101] = 0x80;
    rom[0x0102] = 0xE0;
    rom[0x0103] = 0x26;
    rom[0x0104] = 0x18;
    rom[0x0105] = 0xFE;
    rom
}

#[test]
fn gb_audio_drain_produces_samples() {
    let rom = build_gb_apu_test_rom();
    let mut emu =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load");

    emu.step_frame();

    let samples = emu.drain_audio_samples();
    assert!(
        !samples.is_empty(),
        "stepping one GB frame with APU powered on should produce audio samples"
    );

    let second_drain = emu.drain_audio_samples();
    assert!(
        second_drain.is_empty(),
        "draining again without stepping should produce no samples"
    );
}

#[test]
fn nes_reset_restores_initial_state() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    let initial_pc = emu.cpu_pc();

    for _ in 0..4 {
        emu.step_instruction();
    }
    assert_ne!(emu.cpu_pc(), initial_pc);

    emu.reset();

    assert_eq!(
        emu.cpu_pc(),
        initial_pc,
        "after reset, PC should match the reset vector value"
    );
}

#[test]
fn gb_step_frame_is_deterministic() {
    let rom = build_gb_test_rom();
    let mut emu_a =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load (A)");
    let mut emu_b =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB test ROM should load (B)");

    for _ in 0..3 {
        emu_a.step_frame();
        emu_b.step_frame();
    }

    assert_eq!(
        emu_a.cpu_pc(),
        emu_b.cpu_pc(),
        "PC should match after identical frames"
    );
    assert_eq!(
        emu_a.cpu_cycles(),
        emu_b.cpu_cycles(),
        "cycles should match after identical frames"
    );
    assert_eq!(
        emu_a.framebuffer(),
        emu_b.framebuffer(),
        "framebuffers should be identical for deterministic execution"
    );
}

#[test]
fn nes_step_frame_is_deterministic() {
    let rom = build_nes_test_rom();
    let mut emu_a = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("NES test ROM should load (A)");
    let mut emu_b = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("NES test ROM should load (B)");

    for _ in 0..3 {
        emu_a.step_frame();
        emu_b.step_frame();
    }

    assert_eq!(
        emu_a.cpu_pc(),
        emu_b.cpu_pc(),
        "PC should match after identical frames"
    );
    assert_eq!(
        emu_a.cpu_cycles(),
        emu_b.cpu_cycles(),
        "cycles should match after identical frames"
    );
    assert_eq!(
        emu_a.framebuffer(),
        emu_b.framebuffer(),
        "framebuffers should be identical for deterministic execution"
    );
}

fn build_nes_test_rom_alt() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x99;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x01;
    rom[prg + 4] = 0x4C;
    rom[prg + 5] = 0x04;
    rom[prg + 6] = 0x80;

    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom[prg + 0x3FFA] = 0x04;
    rom[prg + 0x3FFB] = 0x80;
    rom[prg + 0x3FFE] = 0x04;
    rom[prg + 0x3FFF] = 0x80;
    rom
}

#[test]
fn nes_cross_rom_state_load_is_rejected() {
    let rom_a = build_nes_test_rom();
    let rom_b = build_nes_test_rom_alt();
    let mut emu_a =
        zeff_nes_core::emulator::Emulator::new(&rom_a, 44_100.0).expect("NES ROM A should load");
    let mut emu_b =
        zeff_nes_core::emulator::Emulator::new(&rom_b, 44_100.0).expect("NES ROM B should load");

    for _ in 0..4 {
        emu_a.step_instruction();
    }
    let state_a = emu_a.encode_state().expect("encode should succeed");

    let result = emu_b.load_state_from_bytes(state_a);
    assert!(
        result.is_err(),
        "loading a save state from a different ROM should fail"
    );
}

#[test]
fn gb_cross_rom_state_load_is_rejected() {
    let rom_a = build_gb_test_rom();
    let mut rom_b = build_gb_test_rom();
    rom_b[0x0100] = 0x76;

    let mut emu_a =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom_a, HardwareModePreference::Auto)
            .expect("GB ROM A should load");
    let mut emu_b =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom_b, HardwareModePreference::Auto)
            .expect("GB ROM B should load");

    for _ in 0..4 {
        let _ = emu_a.step_instruction();
    }
    let state_a = emu_a.encode_state_bytes().expect("encode should succeed");

    let result = emu_b.load_state_from_bytes(state_a);
    assert!(
        result.is_err(),
        "loading a GB save state from a different ROM should fail"
    );
}

#[test]
fn nes_save_state_rejects_bad_magic() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    let garbage = vec![
        0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    ];
    let result = emu.load_state_from_bytes(garbage);
    assert!(
        result.is_err(),
        "loading data with invalid magic should fail"
    );
}

#[test]
fn nes_save_state_rejects_truncated_data() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    let too_short = vec![0x5A, 0x42, 0x4E];
    let result = emu.load_state_from_bytes(too_short);
    assert!(
        result.is_err(),
        "loading truncated save-state data should fail"
    );
}

#[test]
fn nes_multiple_save_state_roundtrips() {
    let rom = build_nes_test_rom();
    let mut emu =
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).expect("NES test ROM should load");

    for _ in 0..4 {
        emu.step_instruction();
    }
    let state1 = emu.encode_state().expect("first encode should succeed");
    let cycles1 = emu.cpu_cycles();

    emu.step_frame();
    let state2 = emu.encode_state().expect("second encode should succeed");
    let cycles2 = emu.cpu_cycles();
    assert_ne!(
        cycles1, cycles2,
        "cycles should differ after stepping a frame"
    );

    emu.load_state_from_bytes(state1.clone())
        .expect("load state1 should succeed");
    assert_eq!(
        emu.cpu_cycles(),
        cycles1,
        "should restore to first checkpoint"
    );

    emu.load_state_from_bytes(state2)
        .expect("load state2 should succeed");
    assert_eq!(
        emu.cpu_cycles(),
        cycles2,
        "should restore to second checkpoint"
    );

    emu.load_state_from_bytes(state1)
        .expect("reload state1 should succeed");
    assert_eq!(
        emu.cpu_cycles(),
        cycles1,
        "should restore to first checkpoint again"
    );
}
