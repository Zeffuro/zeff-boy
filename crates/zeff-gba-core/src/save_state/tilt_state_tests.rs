use super::{
    LEGACY_TILT_VERSION, TILT_VERSION, VERSION_9_ROM_HASH_SIZE,
    VERSION_11_TILT_EXECUTION_STATE_SIZE, VERSION_12_PSG_STATE_SIZE, encode_state,
    inspect_current_native_gba_tilt_tas_state,
};
use crate::emulator::Emulator;
use crate::hardware::cartridge::{SensorKind, TiltState};

fn tilt_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"KYGE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"EEPROM_V122");
    rom
}

fn dirty_tilt(emu: &mut Emulator) {
    assert!(emu.set_tilt_input(0.25, -0.5));
    emu.cpu_write8(0x0E00_8000, 0x55);
    emu.cpu_write8(0x0E00_8100, 0xAA);
    assert!(emu.set_tilt_input(-0.75, 0.5));
    emu.cpu_write8(0x0E00_8000, 0x55);
}

#[test]
fn v13_roundtrips_complete_tilt_execution_state() {
    let mut source = Emulator::new(&tilt_rom(), 48_000).unwrap();
    dirty_tilt(&mut source);
    let state = encode_state(&source).unwrap();
    assert_eq!(
        u32::from_le_bytes(state[8..12].try_into().unwrap()),
        TILT_VERSION
    );

    let inspection = inspect_current_native_gba_tilt_tas_state(&source, &state).unwrap();
    assert_eq!(inspection.sensor_kind, SensorKind::Tilt);
    assert_eq!(inspection.tilt_state, source.tilt_state());

    let mut restored = Emulator::new(&tilt_rom(), 48_000).unwrap();
    restored.load_state(&state).unwrap();
    assert_eq!(restored.tilt_state(), source.tilt_state());
    assert_eq!(encode_state(&restored).unwrap(), state);
}

#[test]
fn v10_migration_resets_all_tilt_execution_state() {
    let mut source = Emulator::new(&tilt_rom(), 48_000).unwrap();
    dirty_tilt(&mut source);
    let mut legacy = encode_state(&source).unwrap();
    let psg_start = legacy.len()
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_11_TILT_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE;
    legacy.drain(
        psg_start..psg_start + VERSION_12_PSG_STATE_SIZE + VERSION_11_TILT_EXECUTION_STATE_SIZE,
    );
    legacy[8..12].copy_from_slice(&10u32.to_le_bytes());

    let mut target = Emulator::new(&tilt_rom(), 48_000).unwrap();
    dirty_tilt(&mut target);
    target.load_state(&legacy).unwrap();
    assert_eq!(
        target.tilt_state(),
        Some(TiltState {
            host_x_bits: 0,
            host_y_bits: 0,
            x_latch: 0x0FFF,
            y_latch: 0x0FFF,
            latch_ready: false,
        })
    );
}

#[test]
fn legacy_v11_tilt_state_remains_loadable() {
    let mut source = Emulator::new(&tilt_rom(), 48_000).unwrap();
    dirty_tilt(&mut source);
    super::psg_state_tests::seed_psg(&mut source, 5);
    let source_tilt = source.tilt_state();
    let wave_ram = source.apu_psg_wave_ram_snapshot();
    let mut legacy = encode_state(&source).unwrap();
    let psg_start = legacy.len()
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_11_TILT_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE;
    legacy.drain(psg_start..psg_start + VERSION_12_PSG_STATE_SIZE);
    legacy[8..12].copy_from_slice(&LEGACY_TILT_VERSION.to_le_bytes());

    let mut target = Emulator::new(&tilt_rom(), 48_000).unwrap();
    super::psg_state_tests::seed_psg(&mut target, 9);
    target.load_state(&legacy).unwrap();

    assert_eq!(target.tilt_state(), source_tilt);
    super::psg_state_tests::assert_legacy_psg_reconstructed(&target, 5, wave_ram);
}

#[test]
fn malformed_v13_rejects_without_mutating_target() {
    let source = Emulator::new(&tilt_rom(), 48_000).unwrap();
    let state = encode_state(&source).unwrap();
    let tilt_start = state.len() - 32 - VERSION_11_TILT_EXECUTION_STATE_SIZE;
    let mut target = Emulator::new(&tilt_rom(), 48_000).unwrap();
    dirty_tilt(&mut target);
    let before = encode_state(&target).unwrap();

    for mut malformed in [state[..state.len() - 1].to_vec(), state.clone()] {
        if malformed.len() == state.len() {
            malformed[tilt_start + 9..tilt_start + 11].copy_from_slice(&0x1000u16.to_le_bytes());
        }
        assert!(target.load_state(&malformed).is_err());
        assert_eq!(encode_state(&target).unwrap(), before);
    }

    let mut wrong_presence = state;
    wrong_presence[tilt_start] = 0;
    assert!(target.load_state(&wrong_presence).is_err());
    assert_eq!(encode_state(&target).unwrap(), before);
}
