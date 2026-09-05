use super::*;

fn flash_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"FLASH1M_V103");
    rom
}

fn unlock(emu: &mut Emulator) {
    emu.bus.write8(0x0E00_5555, 0xAA);
    emu.bus.write8(0x0E00_2AAA, 0x55);
}

fn command(emu: &mut Emulator, value: u8) {
    unlock(emu);
    emu.bus.write8(0x0E00_5555, value);
}

fn select_bank(emu: &mut Emulator, bank: u8) {
    command(emu, 0xB0);
    emu.bus.write8(0x0E00_0000, bank);
}

fn program(emu: &mut Emulator, value: u8) {
    command(emu, 0xA0);
    emu.bus.write8(0x0E00_1234, value);
}

#[test]
fn current_state_restores_flash_bank_and_partial_command() {
    let rom = flash_rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    program(&mut source, 0x11);
    select_bank(&mut source, 1);
    program(&mut source, 0x22);
    source.bus.write8(0x0E00_5555, 0xAA);
    let state = encode_state(&source).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    select_bank(&mut restored, 1);
    restored.bus.write8(0x0E00_5555, 0xAA);
    decode_state(&mut restored, &state).unwrap();
    assert_eq!(restored.bus.read8(0x0E00_1234), 0x22);
    restored.bus.write8(0x0E00_2AAA, 0x55);
    restored.bus.write8(0x0E00_5555, 0xA0);
    restored.bus.write8(0x0E00_1234, 0x03);
    assert_eq!(restored.bus.read8(0x0E00_1234), 0x02);
    select_bank(&mut restored, 0);
    assert_eq!(restored.bus.read8(0x0E00_1234), 0x11);
}

#[test]
fn version_9_migration_resets_dirty_target_backup_execution_state() {
    let rom = flash_rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    program(&mut source, 0x11);
    select_bank(&mut source, 1);
    program(&mut source, 0x22);
    super::psg_state_tests::seed_psg(&mut source, 5);
    let wave_ram = source.apu_psg_wave_ram_snapshot();
    let mut state = encode_state(&source).unwrap();
    let backup_start = state.len()
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_12_PSG_STATE_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE;
    state.drain(
        backup_start
            ..backup_start + VERSION_10_BACKUP_EXECUTION_STATE_SIZE + VERSION_12_PSG_STATE_SIZE,
    );
    state[8..12].copy_from_slice(&9u32.to_le_bytes());

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    super::psg_state_tests::seed_psg(&mut restored, 9);
    select_bank(&mut restored, 1);
    restored.bus.write8(0x0E00_5555, 0xAA);
    decode_state(&mut restored, &state).unwrap();
    assert_eq!(restored.bus.read8(0x0E00_1234), 0x11);
    select_bank(&mut restored, 1);
    assert_eq!(restored.bus.read8(0x0E00_1234), 0x22);
    super::psg_state_tests::assert_legacy_psg_reconstructed(&restored, 5, wave_ram);
}
