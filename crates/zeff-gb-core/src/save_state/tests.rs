use super::{
    SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, SAVE_STATE_VERSION, SaveStateRef, decode_state,
    encode_state_bytes,
};
use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn read_from_file(path: &Path) -> anyhow::Result<super::SaveState> {
    use anyhow::Context;
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read save state: {}", path.display()))?;
    super::decode_on_thread(bytes)
}

#[test]
fn decode_rejects_bad_magic() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BADMAGIC");
    bytes.extend_from_slice(&SAVE_STATE_FORMAT_VERSION.to_le_bytes());
    let err = decode_state(&bytes)
        .err()
        .expect("bad magic should be rejected")
        .to_string();
    assert!(err.contains("invalid save-state file header"));
}

#[test]
fn decode_rejects_unknown_format_version() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SAVE_STATE_MAGIC);
    bytes.extend_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());

    let err = decode_state(&bytes)
        .err()
        .expect("unknown format version should be rejected")
        .to_string();
    assert!(err.contains("unsupported save-state file format"));
}

#[test]
fn full_save_state_round_trip_handles_large_arrays() {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    let bus = Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");

    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xAB; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::DMG,
        cycle_count: 123,
        last_opcode: 0x00,
        last_opcode_pc: 0x0100,
    };

    let mut path = PathBuf::from(std::env::temp_dir());
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    path.push(format!("zeff-boy-save-state-roundtrip-{unique}.state"));

    super::write_to_file(&path, &state).expect("serialize full save-state should succeed");
    let restored = read_from_file(&path).expect("deserialize full save-state should succeed");
    let _ = std::fs::remove_file(&path);

    assert_eq!(restored.rom_hash, state.rom_hash);
    assert_eq!(restored.hardware_mode, state.hardware_mode);
    assert_eq!(restored.bus.vram, bus.vram);
    assert_eq!(restored.bus.wram, bus.wram);
    assert!(restored.bus.ppu_framebuffer().iter().all(|&b| b == 0));
}

#[test]
fn encoded_state_has_bess_footer() {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    let bus = Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xAB; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::DMG,
        cycle_count: 0,
        last_opcode: 0x00,
        last_opcode_pc: 0x0100,
    };

    let bytes = encode_state_bytes(&state).expect("encode should succeed");

    assert!(bytes.len() >= 8);
    assert_eq!(&bytes[bytes.len() - 4..], b"BESS");

    let footer_offset = bytes.len() - 8;
    let first_block_offset =
        u32::from_le_bytes(bytes[footer_offset..footer_offset + 4].try_into().unwrap());
    assert!(first_block_offset < bytes.len() as u32);

    let name_id = &bytes[first_block_offset as usize..first_block_offset as usize + 4];
    assert_eq!(name_id, b"NAME");
}

#[test]
fn bess_footer_does_not_break_native_decode() {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    let bus = Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xCD; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::DMG,
        cycle_count: 42,
        last_opcode: 0x76,
        last_opcode_pc: 0x0200,
    };

    let bytes = encode_state_bytes(&state).expect("encode should succeed");
    let restored = decode_state(&bytes).expect("decode should succeed with BESS trailing data");

    assert_eq!(restored.rom_hash, [0xCD; 32]);
    assert_eq!(restored.cycle_count, 42);
    assert_eq!(restored.last_opcode, 0x76);
    assert_eq!(restored.last_opcode_pc, 0x0200);
}
