use super::*;

fn rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom
}

fn write_psg(emu: &mut Emulator, offset: u32, value: u8) {
    emu.bus.write8(0x0400_0000 | offset, value);
}

pub(super) fn seed_psg(emu: &mut Emulator, variant: u8) {
    write_psg(emu, 0x084, 0x80);
    write_psg(emu, 0x080, 0x77);
    write_psg(emu, 0x081, 0xFF);
    write_psg(emu, 0x060, 0x16 ^ variant);
    write_psg(emu, 0x062, 0x80);
    write_psg(emu, 0x063, 0xF3);
    write_psg(emu, 0x064, 0xC3 ^ variant);
    write_psg(emu, 0x065, 0xC7);
    write_psg(emu, 0x068, 0x40);
    write_psg(emu, 0x069, 0xA2);
    write_psg(emu, 0x06C, 0x6D ^ variant);
    write_psg(emu, 0x06D, 0xC5);
    write_psg(emu, 0x070, 0x80);
    write_psg(emu, 0x071, 0x20);
    write_psg(emu, 0x072, 0x20);
    write_psg(emu, 0x073, 0x91 ^ variant);
    write_psg(emu, 0x074, 0xC3);
    write_psg(emu, 0x078, 0x20);
    write_psg(emu, 0x079, 0xD3);
    write_psg(emu, 0x07C, 0x19 ^ variant);
    write_psg(emu, 0x07D, 0xC0);
    for index in 0..16 {
        write_psg(emu, 0x090 + index, (index as u8).wrapping_mul(variant | 1));
    }
    emu.bus.step_cycles(12_345 + u32::from(variant));
}

pub(super) fn assert_legacy_psg_reconstructed(emu: &Emulator, variant: u8, wave_ram: [u8; 0x10]) {
    let mut expected_regs = [0; 0x17];
    for (index, value) in [
        (0, 0x16 ^ variant),
        (1, 0x80),
        (2, 0xF3),
        (3, 0xC3 ^ variant),
        (4, 0x47),
        (6, 0x40),
        (7, 0xA2),
        (8, 0x6D ^ variant),
        (9, 0x45),
        (10, 0x80),
        (11, 0x20),
        (12, 0x20),
        (13, 0x91 ^ variant),
        (14, 0x43),
        (16, 0x20),
        (17, 0xD3),
        (18, 0x19 ^ variant),
        (19, 0x40),
        (20, 0x77),
        (21, 0xFF),
    ] {
        expected_regs[index] = value;
    }
    assert_eq!(emu.apu_psg_regs_snapshot(), expected_regs);
    assert_eq!(emu.apu_psg_wave_ram_snapshot(), wave_ram);
    assert_eq!(emu.apu_psg_nr52_raw(), 0x80);
    assert_eq!(emu.apu_debug_snapshot().psg_enabled, [false; 4]);
}

fn legacy_v10(mut state: Vec<u8>) -> Vec<u8> {
    let psg_start = state.len() - VERSION_9_ROM_HASH_SIZE - VERSION_12_PSG_STATE_SIZE;
    state.drain(psg_start..psg_start + VERSION_12_PSG_STATE_SIZE);
    state[8..12].copy_from_slice(&10u32.to_le_bytes());
    state
}

#[test]
fn psg_state_removes_cross_history_continuation() {
    let rom = rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut source, 3);
    let state = encode_state(&source).unwrap();

    let mut first = Emulator::new(&rom, 48_000).unwrap();
    let mut second = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut first, 1);
    seed_psg(&mut second, 7);
    assert_ne!(
        encode_state(&first).unwrap(),
        encode_state(&second).unwrap()
    );

    first.load_state(&state).unwrap();
    second.load_state(&state).unwrap();
    assert_eq!(encode_state(&first).unwrap(), state);
    assert_eq!(encode_state(&second).unwrap(), state);

    for cycles in [1, 3, 4, 127, 8_191, 16_384] {
        first.bus.step_cycles(cycles);
        second.bus.step_cycles(cycles);
        assert_eq!(
            encode_state(&first).unwrap(),
            encode_state(&second).unwrap()
        );
    }
    let mut first_audio = Vec::new();
    let mut second_audio = Vec::new();
    first.drain_audio_samples_into(&mut first_audio);
    second.drain_audio_samples_into(&mut second_audio);
    assert_eq!(first_audio, second_audio);
    assert!(first_audio.iter().any(|sample| *sample != 0.0));
}

#[test]
fn psg_state_matches_uninterrupted_machine_continuation() {
    let rom = rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut source, 3);
    let state = encode_state(&source).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut restored, 7);
    restored.load_state(&state).unwrap();

    for cycles in [1, 3, 4, 127, 8_191, 16_384] {
        source.bus.step_cycles(cycles);
        restored.bus.step_cycles(cycles);
        assert_eq!(
            encode_state(&source).unwrap(),
            encode_state(&restored).unwrap()
        );
    }
}

#[test]
fn legacy_psg_migration_is_history_independent_and_masks_triggers() {
    let rom = rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut source, 5);
    let wave_ram = source.apu_psg_wave_ram_snapshot();
    let legacy = legacy_v10(encode_state(&source).unwrap());

    let mut first = Emulator::new(&rom, 48_000).unwrap();
    let mut second = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut first, 1);
    seed_psg(&mut second, 9);
    first.load_state(&legacy).unwrap();
    second.load_state(&legacy).unwrap();

    assert_eq!(
        encode_state(&first).unwrap(),
        encode_state(&second).unwrap()
    );
    assert_legacy_psg_reconstructed(&first, 5, wave_ram);
}

#[test]
fn psg_state_rejects_truncated_and_malformed_data_transactionally() {
    let rom = rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut source, 3);
    let state = encode_state(&source).unwrap();
    let psg_start = state.len() - VERSION_9_ROM_HASH_SIZE - VERSION_12_PSG_STATE_SIZE;
    let mut target = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut target, 7);
    let before = encode_state(&target).unwrap();

    let mut malformed_bool = state.clone();
    malformed_bool[psg_start + 40] = 2;
    assert!(target.load_state(&malformed_bool).is_err());
    assert_eq!(encode_state(&target).unwrap(), before);

    let mut malformed_phase = state.clone();
    malformed_phase[psg_start + 120] = 8;
    assert!(target.load_state(&malformed_phase).is_err());
    assert_eq!(encode_state(&target).unwrap(), before);

    let mut truncated = state;
    truncated.remove(psg_start + VERSION_12_PSG_STATE_SIZE - 1);
    assert!(target.load_state(&truncated).is_err());
    assert_eq!(encode_state(&target).unwrap(), before);
}

#[test]
fn psg_codec_is_fixed_and_excludes_host_audio_state() {
    let rom = rom();
    let mut source = Emulator::new(&rom, 48_000).unwrap();
    seed_psg(&mut source, 3);
    let state = encode_state(&source).unwrap();
    assert_eq!(
        u32::from_le_bytes(state[8..12].try_into().unwrap()),
        VERSION
    );

    let legacy = legacy_v10(state.clone());
    assert_eq!(state.len() - legacy.len(), VERSION_12_PSG_STATE_SIZE);

    let mut changed_host = source.clone();
    changed_host.set_sample_rate(96_000);
    changed_host.set_apu_sample_generation_enabled(false);
    changed_host.set_apu_channel_mutes([true, false, true, false, true, false]);
    changed_host.set_apu_debug_capture_enabled(true);
    changed_host.bus.apu.seed_host_output_for_state_load_test();
    assert_eq!(encode_state(&changed_host).unwrap(), state);
}
