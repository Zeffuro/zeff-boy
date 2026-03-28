use super::*;

fn build_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg]     = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0xEA;
    rom[prg + 5] = 0xEA;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn make_emulator() -> crate::emulator::Emulator {
    let rom = build_test_rom();
    crate::emulator::Emulator::new(&rom, 44_100.0)
        .expect("test ROM should load")
}

#[test]
fn save_state_roundtrip_preserves_cpu_state() {
    let mut emu = make_emulator();

    for _ in 0..4 {
        emu.step_instruction();
    }

    let pc_before = emu.cpu.pc;
    let sp_before = emu.cpu.sp;
    let a_before = emu.cpu.regs.a;
    let x_before = emu.cpu.regs.x;
    let y_before = emu.cpu.regs.y;
    let p_before = emu.cpu.regs.p;
    let cycles_before = emu.cpu.cycles;

    let state_bytes = encode_state(&emu).expect("encode should succeed");

    emu.reset();
    assert_ne!(emu.cpu.cycles, cycles_before);

    decode_state(&mut emu, &state_bytes).expect("decode should succeed");

    assert_eq!(emu.cpu.pc, pc_before);
    assert_eq!(emu.cpu.sp, sp_before);
    assert_eq!(emu.cpu.regs.a, a_before);
    assert_eq!(emu.cpu.regs.x, x_before);
    assert_eq!(emu.cpu.regs.y, y_before);
    assert_eq!(emu.cpu.regs.p, p_before);
    assert_eq!(emu.cpu.cycles, cycles_before);
}

#[test]
fn save_state_roundtrip_preserves_bus_state() {
    let mut emu = make_emulator();

    for _ in 0..4 {
        emu.step_instruction();
    }

    let ram_00_before = emu.bus.ram[0];
    assert_eq!(ram_00_before, 0x42);

    let ppu_cycles_before = emu.bus.ppu_cycles;
    let open_bus_before = emu.bus.cpu_open_bus;

    let state_bytes = encode_state(&emu).expect("encode should succeed");

    emu.bus.ram[0] = 0x00;
    emu.bus.ppu_cycles = 0;

    decode_state(&mut emu, &state_bytes).expect("decode should succeed");

    assert_eq!(emu.bus.ram[0], 0x42);
    assert_eq!(emu.bus.ppu_cycles, ppu_cycles_before);
    assert_eq!(emu.bus.cpu_open_bus, open_bus_before);
}

#[test]
fn save_state_rom_hash_mismatch_rejected() {
    let mut emu = make_emulator();
    let state_bytes = encode_state(&emu).expect("encode should succeed");

    emu.rom_hash[0] ^= 0xFF;

    let result = decode_state(&mut emu, &state_bytes);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("ROM hash"),
        "error should mention ROM hash mismatch, got: {err_msg}"
    );
}

#[test]
fn save_state_truncated_data_rejected() {
    let emu = make_emulator();
    let state_bytes = encode_state(&emu).expect("encode should succeed");

    let mut truncated = state_bytes[..12].to_vec();
    truncated.extend_from_slice(&[0; 4]);

    let mut emu2 = make_emulator();
    let result = decode_state(&mut emu2, &truncated);
    assert!(result.is_err(), "truncated state should fail to decode");
}

#[test]
fn save_state_bad_magic_rejected() {
    let emu = make_emulator();
    let mut state_bytes = encode_state(&emu).expect("encode should succeed");

    state_bytes[0] = b'X';

    let mut emu2 = make_emulator();
    let result = decode_state(&mut emu2, &state_bytes);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("bad magic"),
        "should reject bad magic"
    );
}

#[test]
fn save_state_unsupported_version_rejected() {
    let emu = make_emulator();
    let mut state_bytes = encode_state(&emu).expect("encode should succeed");

    state_bytes[8..12].copy_from_slice(&99u32.to_le_bytes());

    let mut emu2 = make_emulator();
    let result = decode_state(&mut emu2, &state_bytes);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("unsupported"),
        "should reject unsupported version"
    );
}

#[test]
fn save_state_too_short_rejected() {
    let mut emu = make_emulator();
    let result = decode_state(&mut emu, &[0; 4]);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("too short"),
        "should reject data shorter than header"
    );
}

#[test]
fn save_state_v1_backward_compat() {
    let mut emu = make_emulator();
    for _ in 0..4 {
        emu.step_instruction();
    }
    let pc_before = emu.cpu.pc;

    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    let raw_bytes = payload.into_bytes();

    let mut v1_state = Vec::with_capacity(12 + raw_bytes.len());
    v1_state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    v1_state.extend_from_slice(&1u32.to_le_bytes()); // version 1
    v1_state.extend_from_slice(&raw_bytes);

    // Reset and restore from V1
    emu.reset();
    decode_state(&mut emu, &v1_state).expect("V1 decode should succeed");
    assert_eq!(emu.cpu.pc, pc_before);
}

