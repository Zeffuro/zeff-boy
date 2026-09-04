use super::*;

#[test]
fn tas_state_compatibility_id_tracks_current_native_format() {
    assert_eq!(
        TAS_STATE_FORMAT_COMPATIBILITY_ID,
        format!("zeff-nes-native-state-v{NES_SAVE_STATE_FORMAT_VERSION}")
    );
    assert!(!TAS_DETERMINISM_ABI_ID.is_empty());
}
use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::constants::OAM_DMA;
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::CpuState;
use crate::hardware::timing::NesTiming;
use sha2::{Digest, Sha256};

fn build_test_rom() -> Vec<u8> {
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

fn make_emulator() -> crate::emulator::Emulator {
    let rom = build_test_rom();
    crate::emulator::Emulator::new(&rom, 44_100.0).expect("test ROM should load")
}

fn make_emulator_with_timing(timing: NesTiming) -> crate::emulator::Emulator {
    let rom = build_test_rom();
    let cartridge = Cartridge::load(&rom).expect("test ROM should load");
    let mut emu = make_emulator();
    emu.bus = Bus::new_with_timing(cartridge, 44_100.0, timing);
    emu.cpu = Cpu::new();
    emu.cpu.power_on(&mut emu.bus);
    emu
}

fn make_jam_emulator() -> crate::emulator::Emulator {
    let mut rom = build_test_rom();
    rom[16] = 0x02;
    crate::emulator::Emulator::new(&rom, 44_100.0).expect("test ROM should load")
}

fn make_mmc3_emulator() -> crate::emulator::Emulator {
    let mut rom = vec![0u8; 16 + 2 * 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 1;
    rom[6] = 0x40;
    rom[16 + 0x7FFC] = 0x00;
    rom[16 + 0x7FFD] = 0x80;
    crate::emulator::Emulator::new(&rom, 44_100.0).expect("test ROM should load")
}

fn make_fds_emulator() -> crate::emulator::Emulator {
    make_fds_emulator_with_sides(1)
}

fn make_fds_emulator_with_sides(side_count: usize) -> crate::emulator::Emulator {
    use crate::hardware::cartridge::mappers::{FDS_BIOS_SIZE, FDS_SIDE_SIZE};

    let mut disk = vec![0u8; FDS_SIDE_SIZE * side_count];
    disk[0] = 0x01;
    let mut bios = vec![0xEA; FDS_BIOS_SIZE];
    bios[FDS_BIOS_SIZE - 4] = 0x00;
    bios[FDS_BIOS_SIZE - 3] = 0xE0;
    crate::emulator::Emulator::new_fds(&disk, bios, 44_100.0).expect("test FDS image should load")
}

fn write_legacy_ppu_runtime_state(emu: &crate::emulator::Emulator, payload: &mut StateWriter) {
    let mut sprite_a12 = 0u8;
    for (index, high) in emu.bus.sprite_fetch_a12.iter().copied().enumerate() {
        sprite_a12 |= u8::from(high) << index;
    }
    payload.write_u8(sprite_a12);
    emu.bus.cartridge.write_ppu_runtime_state(payload);
}

fn replace_current_payload_byte(state: &[u8], offset: usize, value: u8) -> Vec<u8> {
    let mut payload = lz4_flex::decompress_size_prepended(&state[12..]).expect("state payload");
    payload[offset] = value;
    let compressed = lz4_flex::compress_prepend_size(&payload);
    let mut replaced = state[..12].to_vec();
    replaced.extend_from_slice(&compressed);
    replaced
}

fn decode_compressed_payload(state: &[u8]) -> Vec<u8> {
    lz4_flex::decompress_size_prepended(&state[12..]).expect("state payload")
}

fn encode_compressed_payload(version: u32, payload: &[u8]) -> Vec<u8> {
    let compressed = lz4_flex::compress_prepend_size(payload);
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&version.to_le_bytes());
    state.extend_from_slice(&compressed);
    state
}

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

fn seed_framebuffer(framebuffer: &mut [u8], seed: u8) {
    for (index, pixel) in framebuffer.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let value = seed.wrapping_add(index as u8);
        pixel.copy_from_slice(&[value, value.rotate_left(2), value.rotate_left(5), 0xFF]);
    }
}

fn mutate_fds_media(emu: &mut crate::emulator::Emulator) {
    let pristine = emu
        .dump_persistent_data()
        .expect("FDS media should persist");
    emu.bus.cpu_write(0x4023, 0x01);
    emu.bus.cpu_write(0x4025, 0xC1);
    emu.bus.cpu_write(0x4024, 0xA5);
    for _ in 0..700_000 {
        emu.bus.cartridge.clock_cpu();
        if emu.bus.cartridge.irq_pending() {
            break;
        }
    }
    assert!(
        emu.dump_persistent_data()
            .expect("FDS media should persist")
            != pristine,
        "fixture must contain a legal FDS disk write"
    );
}

fn assert_public_load_rejection_is_atomic(emu: &mut crate::emulator::Emulator, invalid: &[u8]) {
    let state = emu.encode_state().unwrap();
    let framebuffer = emu.framebuffer().to_vec();
    let frame_ready = emu.frame_ready();
    let suppress_vblank_edge = emu.bus.ppu.suppress_vblank_edge;
    let samples = emu.bus.apu.sample_buffer.clone();
    let sample_rate = emu.bus.apu.output_sample_rate;
    let bus_ephemera = emu.bus.capture_state_load_rollback();
    let suspended = emu.is_cpu_suspended();
    let recent_opcodes = emu.recent_opcodes(32);
    let instruction_trace = emu.instruction_trace().iter().copied().collect::<Vec<_>>();
    let instruction_trace_enabled = emu.instruction_trace().is_enabled();
    let call_stack = emu.call_stack.clone();
    let breakpoints = emu.iter_breakpoints().collect::<Vec<_>>();
    let watchpoints = emu
        .debug_watchpoints()
        .iter()
        .map(|watchpoint| {
            (
                watchpoint.address,
                watchpoint.end_address,
                watchpoint.watch_type,
                watchpoint.last_value,
            )
        })
        .collect::<Vec<_>>();

    assert!(emu.load_state(invalid).is_err());
    assert_bytes_equal(
        "state after rejected load",
        &emu.encode_state().unwrap(),
        &state,
    );
    assert_bytes_equal(
        "framebuffer after rejected load",
        emu.framebuffer(),
        &framebuffer,
    );
    assert_eq!(emu.frame_ready(), frame_ready);
    assert_eq!(emu.bus.ppu.suppress_vblank_edge, suppress_vblank_edge);
    assert_eq!(emu.bus.apu.sample_buffer, samples);
    assert_eq!(emu.bus.apu.output_sample_rate, sample_rate);
    assert_eq!(emu.bus.capture_state_load_rollback(), bus_ephemera);
    assert_eq!(emu.is_cpu_suspended(), suspended);
    assert_eq!(emu.recent_opcodes(32), recent_opcodes);
    assert_eq!(
        emu.instruction_trace().iter().copied().collect::<Vec<_>>(),
        instruction_trace
    );
    assert_eq!(
        emu.instruction_trace().is_enabled(),
        instruction_trace_enabled
    );
    assert_eq!(emu.call_stack, call_stack);
    assert_eq!(emu.iter_breakpoints().collect::<Vec<_>>(), breakpoints);
    assert_eq!(
        emu.debug_watchpoints()
            .iter()
            .map(|watchpoint| (
                watchpoint.address,
                watchpoint.end_address,
                watchpoint.watch_type,
                watchpoint.last_value,
            ))
            .collect::<Vec<_>>(),
        watchpoints
    );
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
fn save_state_v11_restores_frame_ready_and_framebuffer_immediately() {
    let mut source = make_emulator();
    source.step_frame();
    source.bus.ppu.frame_ready = true;
    seed_framebuffer(&mut source.bus.ppu.framebuffer[..], 0x39);
    let expected = source.framebuffer().to_vec();
    let state = encode_state(&source).unwrap();

    let mut restored = make_emulator();
    restored.bus.ppu.framebuffer.fill(0xE7);
    restored.bus.ppu.frame_ready = false;
    restored.load_state(&state).unwrap();

    assert!(restored.frame_ready());
    assert_eq!(restored.frame_count(), source.frame_count());
    assert_bytes_equal("restored framebuffer", restored.framebuffer(), &expected);
}

#[test]
fn save_state_v11_restores_partial_frame_continuation_for_all_timings() {
    for timing in [NesTiming::Ntsc, NesTiming::Pal, NesTiming::Dendy] {
        let mut control = make_emulator_with_timing(timing);
        while control.bus.ppu.scanline < 32 {
            control.step_instruction();
        }
        assert!(
            (1..240).contains(&control.bus.ppu.scanline),
            "fixture must be inside visible output for {timing:?}"
        );
        seed_framebuffer(
            &mut control.bus.ppu.framebuffer[..],
            0x51_u8.wrapping_add(timing.state_tag()),
        );
        let historical_pixel = control.framebuffer()[0..4].to_vec();
        let state = encode_state(&control).unwrap();

        let mut restored = make_emulator_with_timing(timing);
        restored.load_state(&state).unwrap();
        assert_bytes_equal(
            &format!("{timing:?} restored framebuffer"),
            restored.framebuffer(),
            control.framebuffer(),
        );
        assert_eq!(restored.frame_ready(), control.frame_ready(), "{timing:?}");
        assert_eq!(restored.frame_count(), control.frame_count(), "{timing:?}");

        control.step_frame();
        restored.step_frame();

        assert_bytes_equal(
            &format!("{timing:?} next-frame framebuffer"),
            restored.framebuffer(),
            control.framebuffer(),
        );
        assert_eq!(control.framebuffer()[0..4], historical_pixel, "{timing:?}");
        assert_bytes_equal(
            &format!("{timing:?} next-frame state"),
            &restored.encode_state().unwrap(),
            &control.encode_state().unwrap(),
        );
        assert_eq!(restored.frame_count(), control.frame_count(), "{timing:?}");
    }
}

#[test]
fn save_state_v11_restores_framebuffer_used_by_zapper_brightness() {
    let mut source = make_emulator();
    source.bus.ppu.framebuffer.fill(0);
    let (x, y) = (80_u16, 60_u16);
    let offset = (usize::from(y) * 256 + usize::from(x)) * 4;
    source.bus.ppu.framebuffer[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
    source.bus.ppu.scanline = y + 2;
    source.bus.ppu.dot = x + 4;
    source.bus.set_zapper_light_sensor(Some((x, y)), false);
    assert!(source.bus.current_zapper_light_detected());
    let state = encode_state(&source).unwrap();

    let mut restored = make_emulator();
    restored.bus.ppu.framebuffer.fill(0);
    restored.bus.set_zapper_light_sensor(Some((x, y)), false);
    restored.load_state(&state).unwrap();

    assert!(restored.bus.current_zapper_light_detected());
}

#[test]
fn save_state_v10_load_preserves_legacy_best_effort_output_policy() {
    let mut source = make_emulator();
    source.step_frame();
    let legacy = encode_legacy_v10_state(&source).unwrap();
    assert_eq!(u32::from_le_bytes(legacy[8..12].try_into().unwrap()), 10);

    let mut restored = make_emulator();
    seed_framebuffer(&mut restored.bus.ppu.framebuffer[..], 0xB4);
    let target_framebuffer = restored.framebuffer().to_vec();
    restored.bus.ppu.frame_ready = true;
    restored.load_state(&legacy).unwrap();

    assert!(!restored.frame_ready());
    assert_bytes_equal(
        "legacy target framebuffer",
        restored.framebuffer(),
        &target_framebuffer,
    );
}

#[test]
fn replay_projection_matches_independent_legacy_v10_nrom_bytes_and_hash() {
    let mut emu = make_emulator();
    emu.step_frame();
    let mut projected = encode_state(&emu).unwrap();
    project_replay_state_bytes(&mut projected).unwrap();
    let legacy = encode_legacy_v10_state(&emu).unwrap();

    assert_bytes_equal("NROM legacy v10 projection", &projected, &legacy);
    assert_eq!(Sha256::digest(&projected), Sha256::digest(&legacy));
}

#[test]
fn replay_projection_matches_independent_legacy_v10_variable_fds_media() {
    for side_count in [1, 2, 4] {
        let mut emu = make_fds_emulator_with_sides(side_count);
        mutate_fds_media(&mut emu);
        emu.step_frame();
        let mut projected = encode_state(&emu).unwrap();
        project_replay_state_bytes(&mut projected).unwrap();
        let legacy = encode_legacy_v10_state(&emu).unwrap();

        assert_bytes_equal(
            &format!("{side_count}-side FDS legacy v10 projection"),
            &projected,
            &legacy,
        );
        assert_eq!(Sha256::digest(&projected), Sha256::digest(&legacy));
    }
}

#[test]
fn replay_projection_rejects_invalid_v11_suffix_without_unbounded_allocation() {
    let emu = make_emulator();
    let state = encode_state(&emu).unwrap();
    let mut payload = decode_compressed_payload(&state);
    let suffix_start = payload.len() - OUTPUT_SUFFIX_LEN;
    payload[suffix_start] = 2;
    let mut invalid_bool = encode_compressed_payload(NES_SAVE_STATE_FORMAT_VERSION, &payload);
    assert!(project_replay_state_bytes(&mut invalid_bool).is_err());

    let mut oversized = Vec::from(NES_SAVE_STATE_MAGIC);
    oversized.extend_from_slice(&NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes());
    oversized.extend_from_slice(&((MAX_REPLAY_PROJECTION_PAYLOAD_LEN + 1) as u32).to_le_bytes());
    assert!(project_replay_state_bytes(&mut oversized).is_err());

    let short_payload = vec![0; OUTPUT_SUFFIX_LEN - 1];
    let mut short = encode_compressed_payload(NES_SAVE_STATE_FORMAT_VERSION, &short_payload);
    assert!(project_replay_state_bytes(&mut short).is_err());

    let mut invalid_version = state;
    invalid_version[8..12].copy_from_slice(&0_u32.to_le_bytes());
    assert!(project_replay_state_bytes(&mut invalid_version).is_err());
}

#[test]
fn save_state_v11_preserves_pal_cpu_ppu_phase_and_continuation() {
    let mut emu = make_emulator_with_timing(NesTiming::Pal);
    emu.bus.tick_peripherals(2);
    assert_eq!(emu.bus.ppu_clock.master_phase(), 2);
    let state = encode_state(&emu).expect("PAL state should encode");

    emu.bus.tick_peripherals(37);
    let expected = encode_state(&emu).expect("continued PAL state should encode");

    let mut restored = make_emulator_with_timing(NesTiming::Pal);
    decode_state(&mut restored, &state).expect("PAL state should decode");
    assert_eq!(restored.bus.ppu_clock.master_phase(), 2);
    restored.bus.tick_peripherals(37);

    assert_eq!(
        encode_state(&restored).expect("restored PAL continuation should encode"),
        expected
    );
}

#[test]
fn save_state_v11_rejects_region_mismatch_before_machine_state_mutation() {
    let mut saved = make_emulator_with_timing(NesTiming::Pal);
    saved.bus.tick_peripherals(2);
    let state = encode_state(&saved).expect("PAL state should encode");

    let mut restored = make_emulator_with_timing(NesTiming::Dendy);
    restored.bus.ram[0] = 0x5A;
    let error = decode_state(&mut restored, &state).expect_err("region mismatch should reject");

    assert!(error.to_string().contains("timing"));
    assert_eq!(restored.bus.ram[0], 0x5A);
}

#[test]
fn save_state_v11_rejects_invalid_timing_tag_before_machine_state_mutation() {
    let emu = make_emulator();
    let state = encode_state(&emu).expect("state should encode");
    let corrupted = replace_current_payload_byte(&state, 32, 0xFF);
    let mut restored = make_emulator();
    restored.bus.ram[0] = 0x5A;

    let error = decode_state(&mut restored, &corrupted).expect_err("timing tag should reject");

    assert!(error.to_string().contains("timing tag"));
    assert_eq!(restored.bus.ram[0], 0x5A);
}

#[test]
fn save_state_v11_rejects_invalid_clock_phase_before_machine_state_mutation() {
    let emu = make_emulator();
    let state = encode_state(&emu).expect("state should encode");
    let corrupted = replace_current_payload_byte(&state, 33, 1);
    let mut restored = make_emulator();
    restored.bus.ram[0] = 0x5A;

    let error = decode_state(&mut restored, &corrupted).expect_err("clock phase should reject");

    assert!(error.to_string().contains("master-clock phase"));
    assert_eq!(restored.bus.ram[0], 0x5A);
}

#[test]
fn save_state_v9_migrates_as_exact_ntsc_clock_phase() {
    let mut emu = make_emulator();
    emu.bus.tick_peripherals(7);
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    emu.bus.write_ppu_runtime_state(&mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V9_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_emulator();
    decode_state(&mut restored, &state).expect("V9 NTSC state should load");

    assert_eq!(restored.bus.timing, NesTiming::Ntsc);
    assert_eq!(restored.bus.ppu_clock.master_phase(), 0);
}

#[test]
fn save_state_v9_rejects_non_ntsc_destination() {
    let emu = make_emulator();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    emu.bus.write_ppu_runtime_state(&mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V9_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_emulator_with_timing(NesTiming::Pal);
    let error = decode_state(&mut restored, &state).expect_err("legacy PAL state should reject");

    assert!(error.to_string().contains("legacy"));
}

#[test]
fn save_state_v5_preserves_pending_dma() {
    let mut emu = make_emulator();
    emu.bus.cpu_write(crate::hardware::constants::OAM_DMA, 0x00);
    let state = encode_state(&emu).expect("DMA state should encode");

    let expected_cycles = emu.step_instruction().2;
    let expected_oam = emu.bus.ppu.oam;

    let mut restored = make_emulator();
    decode_state(&mut restored, &state).expect("DMA state should decode");

    assert_eq!(restored.step_instruction().2, expected_cycles);
    assert_eq!(restored.bus.ppu.oam, expected_oam);
}

#[test]
fn save_state_v5_preserves_pending_frame_counter_write() {
    let mut emu = make_emulator();
    emu.bus.apu.write_register(0x4015, 0x01, false);
    emu.bus.apu.write_register(0x4003, 0x18, false);
    emu.bus.cpu_write(0x4017, 0x80);
    let state = encode_state(&emu).expect("APU runtime state should encode");

    for _ in 0..3 {
        emu.bus.apu.tick();
    }
    let expected = encode_state(&emu).expect("continued state should encode");

    let mut restored = make_emulator();
    decode_state(&mut restored, &state).expect("APU runtime state should decode");
    for _ in 0..3 {
        restored.bus.apu.tick();
    }

    assert_eq!(
        encode_state(&restored).expect("restored continuation should encode"),
        expected
    );
}

#[test]
fn save_state_v4_load_defaults_frame_counter_runtime() {
    let emu = make_emulator();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V4_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_emulator();
    restored.bus.cpu_write(0x4017, 0x80);
    decode_state(&mut restored, &state).expect("V4 state should load");

    assert_eq!(restored.bus.apu.frame_reset_delay, 0);
}

#[test]
fn save_state_v5_load_defaults_ppu_runtime() {
    let emu = make_mmc3_emulator();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V5_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_mmc3_emulator();
    restored.bus.cartridge.notify_ppu_a12(true, 100);
    restored.bus.cartridge.notify_ppu_a12(false, 200);
    decode_state(&mut restored, &state).expect("V5 state should load");
    restored.bus.cartridge.cpu_write(0xC000, 0);
    restored.bus.cartridge.cpu_write(0xC001, 0);
    restored.bus.cartridge.cpu_write(0xE001, 0);
    restored.bus.cartridge.notify_ppu_a12(true, 7);

    assert!(!restored.bus.cartridge.irq_pending());
    assert_eq!(restored.bus.sprite_fetch_a12, [false; 8]);
}

#[test]
fn save_state_v6_roundtrips_mmc3_ppu_runtime() {
    let mut emu = make_mmc3_emulator();
    emu.bus.sprite_fetch_a12 = [true, false, true, false, true, false, true, false];
    emu.bus.cartridge.notify_ppu_a12(true, 192);
    emu.bus.cartridge.notify_ppu_a12(false, 200);
    emu.bus.cartridge.cpu_write(0xC000, 0);
    emu.bus.cartridge.cpu_write(0xC001, 0);
    emu.bus.cartridge.cpu_write(0xE000, 0);
    emu.bus.cartridge.cpu_write(0xE001, 0);
    let state = encode_state(&emu).expect("V6 state should encode");

    let mut restored = make_mmc3_emulator();
    decode_state(&mut restored, &state).expect("V6 state should load");
    assert_eq!(
        restored.bus.sprite_fetch_a12,
        [true, false, true, false, true, false, true, false]
    );

    restored.bus.cartridge.notify_ppu_a12(true, 207);
    assert!(!restored.bus.cartridge.irq_pending());
    restored.bus.cartridge.notify_ppu_a12(false, 208);
    restored.bus.cartridge.notify_ppu_a12(true, 216);
    assert!(restored.bus.cartridge.irq_pending());
}

#[test]
fn save_state_v9_roundtrips_sprite_evaluation_runtime() {
    let mut emu = make_emulator();
    emu.bus.ppu.sprite_eval_oam_addr = 0x25;
    emu.bus.ppu.sprite_eval_secondary_addr = 17;
    emu.bus.ppu.sprite_eval_latch = 0x9A;
    emu.bus.ppu.sprite_eval_in_range = true;
    emu.bus.ppu.sprite_eval_done = false;
    emu.bus.ppu.sprite_eval_sprite_zero = true;
    emu.bus.ppu.sprite_eval_overflow_remaining = 2;

    let state = encode_state(&emu).expect("sprite evaluation state should encode");
    let mut restored = make_emulator();
    decode_state(&mut restored, &state).expect("sprite evaluation state should decode");

    assert_eq!(restored.bus.ppu.sprite_eval_oam_addr, 0x25);
    assert_eq!(restored.bus.ppu.sprite_eval_secondary_addr, 17);
    assert_eq!(restored.bus.ppu.sprite_eval_latch, 0x9A);
    assert!(restored.bus.ppu.sprite_eval_in_range);
    assert!(!restored.bus.ppu.sprite_eval_done);
    assert!(restored.bus.ppu.sprite_eval_sprite_zero);
    assert_eq!(restored.bus.ppu.sprite_eval_overflow_remaining, 2);
}

#[test]
fn save_state_v8_defaults_sprite_evaluation_runtime() {
    let emu = make_emulator();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    write_legacy_ppu_runtime_state(&emu, &mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V8_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_emulator();
    restored.bus.ppu.sprite_eval_oam_addr = 0x25;
    restored.bus.ppu.sprite_eval_secondary_addr = 17;
    restored.bus.ppu.sprite_eval_in_range = true;
    decode_state(&mut restored, &state).expect("V8 state should load");

    assert_eq!(restored.bus.ppu.sprite_eval_oam_addr, 0);
    assert_eq!(restored.bus.ppu.sprite_eval_secondary_addr, 0);
    assert!(restored.bus.ppu.sprite_eval_done);
    assert!(!restored.bus.ppu.sprite_eval_in_range);
}

#[test]
fn current_save_state_roundtrips_mutated_fds_media() {
    let mut emu = make_fds_emulator();
    mutate_fds_media(&mut emu);
    let mutated = emu
        .dump_persistent_data()
        .expect("FDS media should persist");

    let state = encode_state(&emu).expect("FDS state should encode");
    let mut restored = make_fds_emulator();
    let mut competing_media = restored.dump_persistent_data().unwrap();
    *competing_media.last_mut().unwrap() = 0x7C;
    restored.load_persistent_data(&competing_media).unwrap();
    assert_ne!(restored.dump_persistent_data().unwrap(), mutated);
    decode_state(&mut restored, &state).expect("FDS state should decode");

    assert_eq!(restored.dump_persistent_data().unwrap(), mutated);
}

#[test]
fn current_save_state_roundtrips_fds_media_attachment_and_protection() {
    use zeff_emu_common::media::MediaEvent;

    let mut protected = make_fds_emulator();
    let snapshot = protected.media_slot_snapshot().unwrap();
    protected
        .apply_media_event(&MediaEvent::SetWriteProtected {
            slot: snapshot.state.slot.clone(),
            write_protected: true,
        })
        .unwrap();
    let protected_state = encode_state(&protected).unwrap();
    let mut restored = make_fds_emulator();
    decode_state(&mut restored, &protected_state).unwrap();
    let restored_snapshot = restored.media_slot_snapshot().unwrap();
    assert!(restored_snapshot.inserted());
    assert!(restored_snapshot.state.write_protected);

    protected
        .apply_media_event(&MediaEvent::Eject {
            slot: snapshot.state.slot,
        })
        .unwrap();
    let ejected_state = encode_state(&protected).unwrap();
    decode_state(&mut restored, &ejected_state).unwrap();
    let restored_snapshot = restored.media_slot_snapshot().unwrap();
    assert!(!restored_snapshot.inserted());
    assert_eq!(restored_snapshot.state.side, None);
    assert!(!restored_snapshot.state.write_protected);
    assert!(restored_snapshot.source_media_id.is_some());
}

#[test]
fn save_state_v7_with_mutable_media_remains_readable() {
    let emu = make_fds_emulator();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    write_legacy_ppu_runtime_state(&emu, &mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V7_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_fds_emulator();
    decode_state(&mut restored, &state).expect("V7 FDS state should load");
    assert!(restored.media_slot_snapshot().unwrap().inserted());
}

#[test]
fn save_state_v6_fds_load_preserves_source_media() {
    use zeff_emu_common::media::MediaEvent;

    let mut emu = make_fds_emulator_with_sides(2);
    emu.apply_media_event(&MediaEvent::SelectSide {
        slot: "fds.drive0".into(),
        side: 1,
    })
    .unwrap();
    let expected_media = emu.dump_persistent_data().unwrap();
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    write_legacy_ppu_runtime_state(&emu, &mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V6_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_fds_emulator_with_sides(2);
    let mut competing_media = restored.dump_persistent_data().unwrap();
    *competing_media.last_mut().unwrap() = 0x7C;
    restored.load_persistent_data(&competing_media).unwrap();
    assert_ne!(restored.dump_persistent_data().unwrap(), expected_media);
    decode_state(&mut restored, &state).expect("V6 FDS state should load");
    assert_eq!(restored.dump_persistent_data().unwrap(), expected_media);
    assert_eq!(restored.media_slot_snapshot().unwrap().state.side, Some(1));
}

#[test]
fn save_state_v3_load_resets_dma_controller() {
    let mut emu = make_emulator();
    emu.bus.cpu_write(crate::hardware::constants::OAM_DMA, 0x00);

    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V3_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    let mut restored = make_emulator();
    restored.bus.dma_stall_cycles = 99;
    decode_state(&mut restored, &state).expect("V3 state should load");
    assert_eq!(restored.bus.dma_stall_cycles, 0);
    assert!(restored.step_instruction().2 < 500);
}

#[test]
fn save_state_v3_preserves_jam_phase_continuation() {
    for (completed_idle_steps, expected_addr) in
        [(0, 0xFFFF), (1, 0xFFFE), (2, 0xFFFE), (3, 0xFFFF)]
    {
        let mut emu = make_jam_emulator();
        assert_eq!(emu.step_instruction().2, 2);
        for _ in 0..completed_idle_steps {
            assert_eq!(emu.step_instruction().2, 1);
        }
        if completed_idle_steps == 2 {
            emu.debug_suspend();
        }

        let state = encode_state(&emu).expect("JAM state should encode");
        assert_eq!(
            u32::from_le_bytes(state[8..12].try_into().expect("save-state version")),
            NES_SAVE_STATE_FORMAT_VERSION
        );

        let mut restored = make_jam_emulator();
        decode_state(&mut restored, &state).expect("JAM state should decode");
        assert!(restored.cpu.is_jammed());
        if completed_idle_steps == 2 {
            assert_eq!(restored.cpu.state, CpuState::Suspended);
            restored.debug_continue();
        }
        assert_eq!(restored.cpu.state, CpuState::Halted);

        let (_, _, cycles, events) = restored.step_instruction_with_bus_trace();
        assert_eq!(cycles, 1);
        assert_eq!(
            events.iter().map(|event| event.addr()).collect::<Vec<_>>(),
            [expected_addr]
        );
        assert_eq!(restored.cpu.state, CpuState::Halted);
    }
}

#[test]
fn save_state_v2_halted_cpu_migrates_to_stable_jam_phase() {
    let mut emu = make_jam_emulator();
    assert_eq!(emu.step_instruction().2, 2);

    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());
    let mut state = Vec::with_capacity(12 + compressed.len());
    state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    state.extend_from_slice(&FORMAT_VERSION_V2_COMPRESSED.to_le_bytes());
    state.extend_from_slice(&compressed);

    emu.reset();
    decode_state(&mut emu, &state).expect("legacy JAM state should decode");
    assert_eq!(emu.cpu.state, CpuState::Halted);
    assert!(emu.cpu.is_jammed());

    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();
    assert_eq!(cycles, 1);
    assert_eq!(
        events.iter().map(|event| event.addr()).collect::<Vec<_>>(),
        [0xFFFF]
    );
}

#[test]
fn decode_preserves_runtime_audio_config() {
    let mut saved = make_emulator();
    saved.set_sample_rate(44_100);
    saved.set_apu_sample_generation_enabled(true);
    let state_bytes = encode_state(&saved).expect("encode should succeed");

    let mut restored = make_emulator();
    restored.set_sample_rate(96_000);
    restored.set_apu_sample_generation_enabled(false);

    decode_state(&mut restored, &state_bytes).expect("decode should succeed");

    assert_eq!(restored.bus.apu.output_sample_rate, 96_000.0);
    restored.step_frame();

    let mut samples = Vec::new();
    restored.drain_audio_samples_into(&mut samples);
    assert!(
        samples.is_empty(),
        "saved sample-generation mode should not override runtime skip-audio state"
    );
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
fn public_load_rejects_trailing_payload_without_mutation() {
    let mut emu = make_emulator();
    emu.set_input(0x03, 0x05);
    emu.step_frame();
    emu.set_opcode_log_enabled(true);
    emu.set_instruction_trace_enabled(true);
    emu.step_instruction();
    emu.bus.cpu_nmi_line_sampled = true;
    emu.cpu_write(OAM_DMA, 0x00);
    emu.bus.sprite_fetch_a12[3] = true;
    emu.add_breakpoint(0x8123);
    emu.add_watchpoint(0x0042, crate::debug::WatchType::ReadWrite);
    emu.call_stack.push(crate::debug::CallStackEntry {
        target: 0x8123,
        return_address: 0x8042,
        target_rom_offset: Some(0x123),
        return_rom_offset: Some(0x42),
        kind: crate::debug::CallStackKind::Call,
    });
    emu.debug_suspend();
    let before = emu.encode_state().unwrap();
    let mut payload = decode_compressed_payload(&before);
    payload.push(0xA5);
    let invalid = encode_compressed_payload(NES_SAVE_STATE_FORMAT_VERSION, &payload);

    assert_public_load_rejection_is_atomic(&mut emu, &invalid);
}

#[test]
fn public_load_rejects_malformed_v11_output_without_mutation() {
    let mut emu = make_emulator();
    emu.step_frame();
    emu.bus.ppu.suppress_vblank_edge = true;
    let state = emu.encode_state().unwrap();
    let mut payload = decode_compressed_payload(&state);
    let suffix_start = payload.len() - OUTPUT_SUFFIX_LEN;
    payload[suffix_start] = 2;
    let invalid_bool = encode_compressed_payload(NES_SAVE_STATE_FORMAT_VERSION, &payload);
    assert_public_load_rejection_is_atomic(&mut emu, &invalid_bool);

    payload[suffix_start] = u8::from(emu.frame_ready());
    payload.pop();
    let truncated = encode_compressed_payload(NES_SAVE_STATE_FORMAT_VERSION, &payload);
    assert_public_load_rejection_is_atomic(&mut emu, &truncated);
}

#[test]
fn public_load_rejects_wrong_rom_v11_state_without_mutation() {
    let mut other_rom = build_test_rom();
    other_rom[16 + 8] ^= 0x5A;
    let mut source = crate::emulator::Emulator::new(&other_rom, 44_100.0).unwrap();
    source.step_frame();
    let wrong_rom_state = source.encode_state().unwrap();

    let mut target = make_emulator();
    target.step_frame();
    target.bus.cpu_nmi_line_sampled = true;
    target.bus.dma_stall_cycles = 9;
    target.bus.ppu.suppress_vblank_edge = true;
    assert_public_load_rejection_is_atomic(&mut target, &wrong_rom_state);
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
