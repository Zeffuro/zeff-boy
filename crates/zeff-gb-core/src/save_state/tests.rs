use super::encode::{encode_legacy_v12_state_bytes, encode_legacy_v13_state_bytes};
use super::{
    SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, SAVE_STATE_VERSION, SaveStateRef,
    TAS_DETERMINISM_ABI_ID, TAS_STATE_FORMAT_COMPATIBILITY_ID, decode_state, encode_state_bytes,
    inspect_current_native_tas_state, validate_and_load_current_native_tas_state,
};
use crate::emulator::Emulator;
use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

fn assert_bytes_equal(label: &str, actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len(), "{label} length");
    let difference = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected);
    assert!(
        difference.is_none(),
        "{label} first differs at {difference:?}"
    );
}

fn dmg_emulator(rom: &[u8]) -> Emulator {
    Emulator::from_rom_data(rom, HardwareModePreference::ForceDmg).unwrap()
}

#[test]
fn current_native_tas_contract_tracks_format_version() {
    assert_eq!(
        TAS_STATE_FORMAT_COMPATIBILITY_ID,
        format!("zeff-gb-native-state-v{SAVE_STATE_FORMAT_VERSION}")
    );
    assert!(!TAS_DETERMINISM_ABI_ID.is_empty());
}

#[test]
fn legacy_v13_native_state_remains_loadable() {
    let rom = vec![0; 0x8000];
    let mut source = dmg_emulator(&rom);
    source.step_frame();
    let legacy = encode_legacy_v13_state_bytes(&source.as_save_state_ref()).unwrap();
    let mut restored = dmg_emulator(&rom);

    restored.load_state(&legacy).unwrap();

    assert_eq!(restored.frame_count(), source.frame_count());
    assert_eq!(restored.framebuffer(), source.framebuffer());
}

#[test]
fn current_native_tas_state_restores_authoritative_output_and_continues_exactly() {
    let rom = vec![0; 0x8000];
    let mut source = dmg_emulator(&rom);
    for _ in 0..3 {
        source.step_frame();
    }
    let state = source.encode_state_bytes().unwrap();
    let expected_frame_count = source.frame_count();
    let expected_framebuffer = source.framebuffer().to_vec();
    let mut restored = dmg_emulator(&rom);

    let projection = validate_and_load_current_native_tas_state(&mut restored, &state).unwrap();

    assert_eq!(projection.frame_count, expected_frame_count);
    assert_eq!(projection.lcd_framebuffer.as_ref(), expected_framebuffer);
    assert_eq!(
        projection.replay_state_bytes,
        encode_legacy_v12_state_bytes(&source.as_save_state_ref()).unwrap()
    );
    assert_eq!(restored.frame_count(), expected_frame_count);
    assert_eq!(restored.framebuffer(), expected_framebuffer);
    source.step_frame();
    restored.step_frame();
    assert_eq!(restored.frame_count(), source.frame_count());
    assert_eq!(restored.framebuffer(), source.framebuffer());
    assert_eq!(
        restored.encode_state_bytes().unwrap(),
        source.encode_state_bytes().unwrap()
    );
}

#[test]
fn current_native_tas_state_inspection_is_nonmutating() {
    let rom = vec![0; 0x8000];
    let mut source = dmg_emulator(&rom);
    source.step_frame();
    let state = source.encode_state_bytes().unwrap();
    let mut target = dmg_emulator(&rom);
    target.step_frame();
    target.step_frame();
    let before = target.encode_state_bytes().unwrap();

    let inspection = inspect_current_native_tas_state(&target, &state).unwrap();

    assert_eq!(inspection.projection.frame_count, source.frame_count());
    assert_eq!(target.encode_state_bytes().unwrap(), before);
}

#[test]
fn current_native_tas_state_rejects_legacy_generic_and_wrong_rom_without_mutation() {
    let rom = vec![0; 0x8000];
    let mut source = dmg_emulator(&rom);
    source.step_frame();
    let current = source.encode_state_bytes().unwrap();
    let legacy = encode_legacy_v12_state_bytes(&source.as_save_state_ref()).unwrap();
    let state_ref = source.as_save_state_ref();
    let mut generic_bess_writer = super::StateWriter::new();
    super::bess::append_bess_with_optional_zeff_extension(
        &mut generic_bess_writer,
        state_ref.cpu,
        state_ref.bus,
        state_ref.hardware_mode,
        None,
    )
    .unwrap();
    let generic_bess = generic_bess_writer.into_bytes();
    let mut target = dmg_emulator(&rom);
    target.step_frame();
    let before = target.encode_state_bytes().unwrap();

    let truncated = &current[..current.len() - 1];
    for bytes in [&legacy[..], &generic_bess, truncated] {
        assert!(validate_and_load_current_native_tas_state(&mut target, bytes).is_err());
        assert_eq!(target.encode_state_bytes().unwrap(), before);
    }

    let mut other_rom = rom.clone();
    other_rom[0x150] = 0x5A;
    let mut wrong_rom_target = dmg_emulator(&other_rom);
    wrong_rom_target.step_frame();
    let wrong_rom_before = wrong_rom_target.encode_state_bytes().unwrap();
    assert!(validate_and_load_current_native_tas_state(&mut wrong_rom_target, &current).is_err());
    assert_eq!(
        wrong_rom_target.encode_state_bytes().unwrap(),
        wrong_rom_before
    );
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
    let mut bus = Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
    bus.set_game_boy_serial_device(crate::hardware::GameBoySerialDevice::Printer);

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
        boot_rom_enabled: false,
        frame_count: 7,
    };

    let bytes = encode_state_bytes(&state).expect("encode should succeed");

    let mut path = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    path.push(format!("zeff-boy-save-state-roundtrip-{unique}.state"));

    std::fs::write(&path, &bytes).expect("write save-state file should succeed");
    let file_bytes = std::fs::read(&path).expect("read save-state file should succeed");
    let _ = std::fs::remove_file(&path);

    let restored = decode_state(&file_bytes).expect("decode should succeed");

    assert_eq!(restored.rom_hash, state.rom_hash);
    assert_eq!(restored.hardware_mode, state.hardware_mode);
    assert_eq!(restored.bus.vram, bus.vram);
    assert_eq!(restored.bus.wram, bus.wram);
    assert_eq!(restored.frame_count, Some(7));
    assert_eq!(
        restored.lcd_framebuffer.as_deref(),
        Some(bus.ppu_lcd_framebuffer())
    );
    assert_eq!(
        restored.bus.game_boy_serial_device(),
        crate::hardware::GameBoySerialDevice::Printer
    );
    assert!(restored.bus.ppu_framebuffer().iter().all(|&b| b == 0));
}

#[test]
fn encoded_state_has_spec_compliant_bess_footer_and_end() {
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
        boot_rom_enabled: false,
        frame_count: 8,
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
    assert_eq!(&bytes[footer_offset - 8..footer_offset], b"END \0\0\0\0");

    let extension = super::bess::required_zeff_extension(&bytes, 0).unwrap();
    assert_eq!(extension.frame_count, 8);
    assert_eq!(
        extension.lcd_framebuffer.as_ref(),
        bus.ppu_lcd_framebuffer()
    );
}

#[test]
fn replay_projection_is_byte_identical_to_legacy_v12_encoding() {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    let mut bus = Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
    let framebuffer = (0..super::bess::ZEFF_EXTENSION_FRAMEBUFFER_LEN)
        .map(|index| index as u8)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    bus.restore_ppu_lcd_framebuffer(framebuffer);
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xBC; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::DMG,
        cycle_count: 456,
        last_opcode: 0x76,
        last_opcode_pc: 0x1234,
        boot_rom_enabled: false,
        frame_count: 0x0102_0304_0506_0708,
    };

    let mut projected = encode_state_bytes(&state).unwrap();
    super::project_replay_state_bytes(&mut projected).unwrap();
    let legacy = encode_legacy_v12_state_bytes(&state).unwrap();

    assert_bytes_equal("legacy v12 projection", &projected, &legacy);
    assert_eq!(u32::from_le_bytes(projected[8..12].try_into().unwrap()), 12);
    let restored = decode_state(&projected).unwrap();
    assert_eq!(restored.frame_count, None);
    assert_eq!(restored.lcd_framebuffer, None);
}

#[test]
fn sgb_continuation_projection_is_byte_identical_to_legacy_v12_encoding() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    let mut bus = Bus::new(rom, &header, HardwareMode::SGB1).expect("test bus should initialize");
    let mut first = [0; 16];
    first[0] = (0x04 << 3) | 3;
    let _ = bus.write_byte(0xFF00, 0x00);
    for bit_index in 0..128 {
        let bit = first[bit_index / 8] >> (bit_index % 8) & 1;
        let _ = bus.write_byte(0xFF00, if bit == 0 { 0x20 } else { 0x10 });
    }
    let second = [0xA5; 16];
    let _ = bus.write_byte(0xFF00, 0x00);
    for bit_index in 0..37 {
        let bit = second[bit_index / 8] >> (bit_index % 8) & 1;
        let _ = bus.write_byte(0xFF00, if bit == 0 { 0x20 } else { 0x10 });
    }
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0x5B; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::ForceSgb,
        hardware_mode: HardwareMode::SGB1,
        cycle_count: 91,
        last_opcode: 0,
        last_opcode_pc: 0x100,
        boot_rom_enabled: false,
        frame_count: 2,
    };

    let mut projected = encode_state_bytes(&state).unwrap();
    super::project_replay_state_bytes(&mut projected).unwrap();
    let legacy = encode_legacy_v12_state_bytes(&state).unwrap();

    assert_bytes_equal(
        "legacy v12 SGB continuation projection",
        &projected,
        &legacy,
    );
}

#[test]
fn replay_projection_matches_independent_legacy_v12_mbc3_rtc_hash() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x10;
    rom[0x149] = 0x03;
    let header = RomHeader::from_rom(&rom).expect("MBC3 RTC test ROM header should parse");
    let bus = Bus::new(rom, &header, HardwareMode::CGBNormal)
        .expect("MBC3 RTC test bus should initialize");
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xD3; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::CGBNormal,
        cycle_count: 789,
        last_opcode: 0x00,
        last_opcode_pc: 0x0100,
        boot_rom_enabled: false,
        frame_count: 44,
    };

    let mut projected = encode_state_bytes(&state).unwrap();
    let mut legacy = encode_legacy_v12_state_bytes(&state).unwrap();
    set_bess_rtc_timestamp(&mut projected, 111);
    set_bess_rtc_timestamp(&mut legacy, 222);
    super::project_replay_state_bytes(&mut projected).unwrap();
    super::canonicalize_replay_hash_bytes(&mut projected);
    super::canonicalize_replay_hash_bytes(&mut legacy);

    assert_bytes_equal("legacy v12 RTC projection", &projected, &legacy);
    assert_eq!(Sha256::digest(&projected), Sha256::digest(&legacy));
}

#[test]
fn replay_projection_matches_independent_legacy_v12_pocket_camera_state() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0xFC;
    rom[0x149] = 0x04;
    let header = RomHeader::from_rom(&rom).expect("Pocket Camera test ROM header should parse");
    let mut bus = Bus::new(rom, &header, HardwareMode::DMG)
        .expect("Pocket Camera test bus should initialize");
    let _ = bus.write_byte(0x0000, 0x0A);
    let _ = bus.write_byte(0x4000, 0x10);
    let _ = bus.write_byte(0xA002, 0x00);
    let _ = bus.write_byte(0xA003, 0x01);
    bus.cartridge.set_camera_frame(
        &(0..(128 * 112))
            .map(|index| (index as u8).wrapping_mul(17))
            .collect::<Vec<_>>(),
    );
    let _ = bus.write_byte(0xA000, 0x01);
    bus.cartridge.step(321);
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xCA; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::DMG,
        cycle_count: 321,
        last_opcode: 0x00,
        last_opcode_pc: 0x0100,
        boot_rom_enabled: false,
        frame_count: 5,
    };

    let mut projected = encode_state_bytes(&state).unwrap();
    super::project_replay_state_bytes(&mut projected).unwrap();
    let legacy = encode_legacy_v12_state_bytes(&state).unwrap();

    assert_bytes_equal("legacy v12 Pocket Camera projection", &projected, &legacy);
    assert_eq!(Sha256::digest(&projected), Sha256::digest(&legacy));
}

fn set_bess_rtc_timestamp(bytes: &mut [u8], timestamp: u64) {
    let footer_start = bytes.len() - 8;
    let mut block_start =
        u32::from_le_bytes(bytes[footer_start..footer_start + 4].try_into().unwrap()) as usize;
    loop {
        let length = u32::from_le_bytes(bytes[block_start + 4..block_start + 8].try_into().unwrap())
            as usize;
        if &bytes[block_start..block_start + 4] == b"RTC " {
            assert_eq!(length, super::bess::RTC_BLOCK_LEN as usize);
            let timestamp_start = block_start + 8 + 0x28;
            bytes[timestamp_start..timestamp_start + 8].copy_from_slice(&timestamp.to_le_bytes());
            return;
        }
        assert_ne!(&bytes[block_start..block_start + 4], b"END ");
        block_start += 8 + length;
    }
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
        boot_rom_enabled: false,
        frame_count: 9,
    };

    let bytes = encode_state_bytes(&state).expect("encode should succeed");
    let restored = decode_state(&bytes).expect("decode should succeed with BESS trailing data");

    assert_eq!(restored.rom_hash, [0xCD; 32]);
    assert_eq!(restored.cycle_count, 42);
    assert_eq!(restored.last_opcode, 0x76);
    assert_eq!(restored.last_opcode_pc, 0x0200);
}

#[test]
fn native_decode_restores_cgb_dmg_compat_state_without_serialized_rom() {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    assert!(!header.is_cgb_compatible);

    let bus = Bus::new(rom, &header, HardwareMode::CGBNormal).expect("test bus should initialize");
    let cpu = Cpu::new();
    let state = SaveStateRef {
        version: SAVE_STATE_VERSION,
        rom_hash: [0xEF; 32],
        cpu: &cpu,
        bus: &bus,
        hardware_mode_preference: HardwareModePreference::Auto,
        hardware_mode: HardwareMode::CGBNormal,
        cycle_count: 99,
        last_opcode: 0x10,
        last_opcode_pc: 0x0300,
        boot_rom_enabled: false,
        frame_count: 10,
    };

    let bytes = encode_state_bytes(&state).expect("encode should succeed");
    let restored = decode_state(&bytes).expect("decode should not validate the empty saved ROM");

    assert_eq!(restored.hardware_mode, HardwareMode::CGBNormal);
    assert_eq!(restored.rom_hash, [0xEF; 32]);
    assert_eq!(restored.cycle_count, 99);
}
