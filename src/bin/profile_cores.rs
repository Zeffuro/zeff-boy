use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::mem::size_of;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use zeff_emu_common::time::FrameLifecycle;

const DEFAULT_FRAMES: u32 = 3_000;

struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        REALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn reset_allocation_counts() {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    REALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

fn allocation_counts() -> (u64, u64, u64) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        REALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

fn hash_string(hash: &[u8]) -> String {
    hash.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>()
}

fn print_accuracy_hashes(framebuffer: &[u8], state: &[u8], audio: &[f32]) {
    use sha2::{Digest, Sha256};

    let framebuffer_hash = Sha256::digest(framebuffer);
    let state_hash = Sha256::digest(state);
    let mut audio_hasher = Sha256::new();
    for sample in audio {
        audio_hasher.update(sample.to_le_bytes());
    }
    let audio_hash = audio_hasher.finalize();
    println!(
        "  framebuffer {}  state {}  audio {}",
        hash_string(&framebuffer_hash),
        hash_string(&state_hash),
        hash_string(&audio_hash),
    );
}

fn profile_frames<M: FrameLifecycle>(label: &str, frames: u32, machine: &mut M) {
    profile_frames_with_prepare(label, frames, machine, |_| {});
}

fn profile_frames_with_prepare<M: FrameLifecycle>(
    label: &str,
    frames: u32,
    machine: &mut M,
    prepare: impl FnOnce(&mut M),
) {
    for _ in 0..10 {
        machine.step_frame();
    }

    let start_ticks = machine.timing_snapshot().now().get();
    prepare(machine);
    reset_allocation_counts();
    let start = Instant::now();
    for _ in 0..frames {
        machine.step_frame();
    }
    let elapsed = start.elapsed();
    let (allocations, reallocations, allocated_bytes) = allocation_counts();
    let elapsed_ticks = machine
        .timing_snapshot()
        .now()
        .get()
        .wrapping_sub(start_ticks);
    let fps = f64::from(frames) / elapsed.as_secs_f64();
    let million_ticks_per_second = elapsed_ticks as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!(
        "{label:30} {frames:5} frames  {elapsed:>9.2?}  {fps:>8.0} fps  {million_ticks_per_second:>8.2} M master ticks / s"
    );
    println!(
        "{:30} {:9} alloc  {:7} realloc  {:9.1} KiB",
        "",
        allocations,
        reallocations,
        allocated_bytes as f64 / 1024.0
    );
}

fn profile_wonderswan_frames(
    label: &str,
    frames: u32,
    machine: &mut zeff_ws_core::emulator::Emulator,
) {
    profile_frames_with_prepare(label, frames, machine, |machine| machine.reset_profiling());
    let snapshot = machine.profiling_snapshot();
    println!(
        "  WonderSwan calls: bus {}  UART {}  APU {}  sound DMA {}  PPU {}",
        snapshot.bus_step_calls,
        snapshot.uart_step_calls,
        snapshot.apu_step_calls,
        snapshot.sound_dma_step_calls,
        snapshot.ppu_step_calls,
    );
    println!(
        "  WonderSwan transitions: {} cycles  {} scanlines  {} vblank  {} line compare  {} hblank timer  {} vblank timer",
        snapshot.master_cycles,
        snapshot.completed_scanlines,
        snapshot.vblank_starts,
        snapshot.line_compare_events,
        snapshot.hblank_timer_advances,
        snapshot.vblank_timer_advances,
    );
}

fn profile_gb_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    let mut gb =
        zeff_gb_core::emulator::Emulator::from_rom_data(&gb_rom(), HardwareModePreference::Auto)
            .expect("synthetic GB ROM");
    gb.set_apu_sample_generation_enabled(sample_generation_enabled);
    gb.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("GB synthetic{suffix}"), frames, &mut gb);

    let state = gb.encode_state_bytes().expect("encode synthetic GB state");
    let mut audio = Vec::new();
    gb.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(gb.framebuffer(), &state, &audio);
}

fn profile_gba_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    for (label, rom) in [
        ("GBA synthetic", gba_rom()),
        ("GBA RAM writes", gba_write_rom()),
    ] {
        let mut gba =
            zeff_gba_core::emulator::Emulator::from_rom_data(&rom).expect("synthetic GBA ROM");
        gba.set_apu_sample_generation_enabled(sample_generation_enabled);
        gba.set_apu_debug_capture_enabled(false);
        gba.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_frames(&format!("{label}{suffix}"), frames, &mut gba);

        let state = gba.encode_state().expect("encode synthetic GBA state");
        let mut audio = Vec::new();
        gba.drain_audio_samples_into(&mut audio);
        print_accuracy_hashes(gba.framebuffer(), &state, &audio);
    }
}

fn profile_gba_active_video(frames: u32, sample_generation_enabled: bool) {
    let mut gba =
        zeff_gba_core::emulator::Emulator::from_rom_data(&gba_rom()).expect("synthetic GBA ROM");
    let mut pattern = 0xA5A5_5A5A_u32;
    for offset in (0..0x4000_u32).step_by(2) {
        pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        gba.cpu_write16(0x0600_0000 + offset, (pattern >> 16) as u16);
    }
    for index in 0..(32 * 32_u32) {
        let tile = (index * 13) & 0x01FF;
        let attributes = ((index & 0x0F) << 12) | ((index & 1) << 10) | ((index & 2) << 10);
        gba.cpu_write16(0x0600_4000 + index * 2, (tile | attributes) as u16);
    }
    for color in 0..256_u32 {
        let r = color & 0x1F;
        let g = (color * 3) & 0x1F;
        let b = (color * 7) & 0x1F;
        gba.cpu_write16(0x0500_0000 + color * 2, (r | (g << 5) | (b << 10)) as u16);
    }

    gba.cpu_write16(0x0200_0000, 0x03FF);
    gba.cpu_write32(0x0400_00B0, 0x0200_0000);
    gba.cpu_write32(0x0400_00B4, 0x0500_0002);
    gba.cpu_write16(0x0400_00B8, 1);
    gba.cpu_write16(0x0400_00BA, 0xA340);
    gba.cpu_write16(0x0400_0100, 0xFFC0);
    gba.cpu_write16(0x0400_0102, 0x0081);
    gba.cpu_write16(0x0400_0104, 0xFFF0);
    gba.cpu_write16(0x0400_0106, 0x0084);
    gba.cpu_write16(0x0400_0008, 8 << 8);
    gba.cpu_write16(0x0400_0000, 1 << 8);

    gba.set_apu_sample_generation_enabled(sample_generation_enabled);
    gba.set_apu_debug_capture_enabled(false);
    profile_frames(
        if sample_generation_enabled {
            "GBA active video + DMA + timers + audio"
        } else {
            "GBA active video + DMA + timers"
        },
        frames,
        &mut gba,
    );
    assert_eq!(
        gba.cpu_peek16(0x0500_0002),
        0x03FF,
        "synthetic GBA HBlank DMA did not update palette RAM"
    );
    let first_pixel = &gba.framebuffer()[..4];
    assert!(
        gba.framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != first_pixel),
        "synthetic GBA active-video fixture produced a flat frame"
    );
    let state = gba.encode_state().expect("encode GBA active-video state");
    let mut audio = Vec::new();
    gba.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(gba.framebuffer(), &state, &audio);
}

fn profile_sega8_video(frames: u32, sample_generation_enabled: bool) {
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    let mut sega = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &sega8_rom(),
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("synthetic Sega 8-bit video ROM");
    let vdp = sega.bus_mut().vdp_mut();
    for (register, value) in [(0, 0x04), (1, 0x40), (2, 0x0E), (5, 0x7E), (8, 13), (9, 29)] {
        vdp.write_control(value);
        vdp.write_control(0x80 | register);
    }
    vdp.write_control(0);
    vdp.write_control(0x40);
    let mut pattern = 0xA5A5_5A5A_u32;
    for _ in 0..0x2000 {
        pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        vdp.write_data((pattern >> 24) as u8);
    }
    vdp.write_control(0);
    vdp.write_control(0x78);
    for index in 0..(32 * 28) {
        let tile = (index % 256) as u16;
        let attributes = match index % 4 {
            0 => 0,
            1 => 1 << 10,
            2 => 1 << 11,
            _ => (1 << 9) | (1 << 12),
        };
        for byte in (tile | attributes).to_le_bytes() {
            vdp.write_data(byte);
        }
    }
    vdp.write_control(0);
    vdp.write_control(0x7F);
    vdp.write_data(0xD0);
    vdp.write_control(0);
    vdp.write_control(0xC0);
    for color in 0..32 {
        vdp.write_data((color * 11) as u8);
    }

    sega.set_apu_sample_generation_enabled(sample_generation_enabled);
    profile_frames(
        if sample_generation_enabled {
            "Sega 8-bit Mode 4 video + audio"
        } else {
            "Sega 8-bit Mode 4 video"
        },
        frames,
        &mut sega,
    );
    assert!(
        sega.framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0, 0, 0]),
        "synthetic Sega 8-bit video fixture did not produce visible pixels"
    );
    let state = sega
        .encode_state()
        .expect("encode synthetic Sega 8-bit state");
    let mut audio = Vec::new();
    sega.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(sega.framebuffer(), &state, &audio);
}

fn profile_sega8_sprites(frames: u32, sample_generation_enabled: bool) {
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    let mut sega = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &sega8_rom(),
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("synthetic Sega 8-bit sprite ROM");
    let vdp = sega.bus_mut().vdp_mut();
    for (register, value) in [(0, 0x04), (1, 0x40), (2, 0x0E), (5, 0x7E), (6, 0x00)] {
        vdp.write_control(value);
        vdp.write_control(0x80 | register);
    }

    vdp.write_control(0x20);
    vdp.write_control(0x40);
    let mut pattern = 0xA5A5_5A5A_u32;
    for _ in 0..(64 * 32) {
        pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        vdp.write_data((pattern >> 24) as u8);
    }

    vdp.write_control(0);
    vdp.write_control(0x7F);
    for sprite in 0..64_u8 {
        vdp.write_data((sprite / 8) * 24);
    }
    vdp.write_control(0x80);
    vdp.write_control(0x7F);
    for sprite in 0..64_u8 {
        vdp.write_data((sprite % 8) * 28);
        vdp.write_data(sprite + 1);
    }

    vdp.write_control(0);
    vdp.write_control(0xC0);
    vdp.write_data(0);
    for color in 1..32 {
        vdp.write_data((color * 11) as u8);
    }

    sega.set_apu_sample_generation_enabled(sample_generation_enabled);
    profile_frames(
        if sample_generation_enabled {
            "Sega 8-bit Mode 4 sprites + audio"
        } else {
            "Sega 8-bit Mode 4 sprites"
        },
        frames,
        &mut sega,
    );
    let background = &sega.framebuffer()[..4];
    assert!(
        sega.framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != background),
        "synthetic Sega 8-bit sprite fixture produced a flat frame"
    );
    let state = sega
        .encode_state()
        .expect("encode synthetic Sega 8-bit sprite state");
    let mut audio = Vec::new();
    sega.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(sega.framebuffer(), &state, &audio);
}

fn profile_pce_frames(label: &str, frames: u32, machine: &mut zeff_pce_core::hardware::PceMachine) {
    for _ in 0..10 {
        machine.run_until_frame().expect("synthetic PCE frame");
    }

    reset_allocation_counts();
    let start = Instant::now();
    let mut master_ticks = 0_u64;
    for _ in 0..frames {
        master_ticks += machine
            .run_until_frame()
            .expect("synthetic PCE frame")
            .master_ticks();
    }
    let elapsed = start.elapsed();
    let (allocations, reallocations, allocated_bytes) = allocation_counts();
    let fps = f64::from(frames) / elapsed.as_secs_f64();
    let million_ticks_per_second = master_ticks as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!(
        "{label:30} {frames:5} frames  {elapsed:>9.2?}  {fps:>8.0} fps  {million_ticks_per_second:>8.2} M master ticks / s"
    );
    println!(
        "{:30} {:9} alloc  {:7} realloc  {:9.1} KiB",
        "",
        allocations,
        reallocations,
        allocated_bytes as f64 / 1024.0
    );
}

fn print_pce_accuracy_hashes(machine: &mut zeff_pce_core::hardware::PceMachine) {
    let state = zeff_pce_core::hardware::save_state::encode_state(machine)
        .expect("encode synthetic PCE state");
    let mut audio = Vec::new();
    machine.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(machine.framebuffer(), &state, &audio);
}

fn profile_pce_state(iterations: u32) {
    let mut machine =
        zeff_pce_core::hardware::PceMachine::new(pce_rom()).expect("synthetic PCE ROM");
    for _ in 0..10 {
        machine.run_until_frame().expect("synthetic PCE frame");
    }

    let warm_state =
        zeff_pce_core::hardware::save_state::encode_state(&machine).expect("encode PCE state");
    let state_size = warm_state.len();
    black_box(warm_state);

    reset_allocation_counts();
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(
            zeff_pce_core::hardware::save_state::encode_state(&machine).expect("encode PCE state"),
        );
    }
    let elapsed = start.elapsed();
    let (allocations, reallocations, allocated_bytes) = allocation_counts();
    let states_per_second = f64::from(iterations) / elapsed.as_secs_f64();
    let gib_per_second = state_size as f64 * states_per_second / 1024.0 / 1024.0 / 1024.0;
    println!(
        "PC Engine state               {iterations:5} states  {elapsed:>9.2?}  {states_per_second:>8.1} states / s  {gib_per_second:>6.2} GiB / s  {state_size} bytes"
    );
    println!(
        "{:30} {:9} alloc  {:7} realloc  {:9.1} MiB",
        "",
        allocations,
        reallocations,
        allocated_bytes as f64 / 1024.0 / 1024.0
    );

    let seconds = iterations.div_ceil(60) as usize;
    let mut rewind = zeff_emu_common::rewind::RewindBuffer::new(seconds, 1);
    let warm_state =
        zeff_pce_core::hardware::save_state::encode_state(&machine).expect("encode PCE state");
    rewind.push(&warm_state, &[]);
    rewind.clear();

    reset_allocation_counts();
    let start = Instant::now();
    for _ in 0..iterations {
        let state =
            zeff_pce_core::hardware::save_state::encode_state(&machine).expect("encode PCE state");
        rewind.push(&state, &[]);
    }
    let elapsed = start.elapsed();
    let (allocations, reallocations, allocated_bytes) = allocation_counts();
    let captures_per_second = f64::from(iterations) / elapsed.as_secs_f64();
    println!(
        "PC Engine rewind              {iterations:5} captures {elapsed:>9.2?}  {captures_per_second:>8.1} captures / s"
    );
    println!(
        "{:30} {:9} alloc  {:7} realloc  {:9.1} MiB",
        "",
        allocations,
        reallocations,
        allocated_bytes as f64 / 1024.0 / 1024.0
    );
    black_box(rewind);
}

fn profile_trace_store() {
    use std::hint::black_box;
    use zeff_emu_common::debug::{
        InstructionTraceRecord, InstructionTraceStore, RegisterDelta, TraceExecMode,
    };

    const RECORDS: u64 = 2_000_000;
    let mut store = InstructionTraceStore::default();
    store.set_enabled(true);

    let start = Instant::now();
    for sequence in 0..RECORDS {
        let mut record = InstructionTraceRecord::new(
            TraceExecMode::Z80,
            sequence as u32,
            Some(sequence),
            sequence / 20_000,
            sequence,
            &[0x00],
        );
        record.push_register_delta(RegisterDelta {
            register: 0,
            value: sequence as u32,
        });
        black_box(store.push(record));
    }
    let elapsed = start.elapsed();
    let ns_per_record = elapsed.as_secs_f64() * 1_000_000_000.0 / RECORDS as f64;
    println!(
        "trace store                    {RECORDS:9} records  {elapsed:>9.2?}  {ns_per_record:>8.2} ns / record  {} bytes / record",
        size_of::<InstructionTraceRecord>()
    );
    black_box(store);
}

fn load_manifest(name: &str) -> Vec<(String, String)> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-roms")
        .join(name);
    let Ok(contents) = std::fs::read_to_string(&manifest) else {
        eprintln!("manifest not found: {}", manifest.display());
        return Vec::new();
    };
    contents
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (label, path) = line.split_once('\t')?;
            Some((label.trim().to_owned(), path.trim().to_owned()))
        })
        .collect()
}

fn gb_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x150..0x154].copy_from_slice(&[0x00, 0x00, 0x18, 0xFC]);
    let mut checksum = 0_u8;
    for &byte in &rom[0x134..=0x14C] {
        checksum = checksum.wrapping_sub(byte).wrapping_sub(1);
    }
    rom[0x14D] = checksum;
    rom
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
    rom[0xA0..0xA7].copy_from_slice(b"PROFILE");
    rom[0xB2] = 0x96;
    rom
}

fn gba_write_rom() -> Vec<u8> {
    let mut rom = gba_rom();
    for (offset, instruction) in [0xE3A0_0402_u32, 0xE580_1000, 0xE281_1001, 0xEAFF_FFFC]
        .into_iter()
        .enumerate()
    {
        let start = offset * 4;
        rom[start..start + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    rom
}

fn nes_rom() -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg..prg + 4].copy_from_slice(&[0xEA, 0xEA, 0x4C, 0x00]);
    rom[prg + 4] = 0x80;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn nes_ram_write_rom() -> Vec<u8> {
    let mut rom = nes_rom();
    let prg = 16;
    rom[prg..prg + 5].copy_from_slice(&[0xE6, 0x00, 0x4C, 0x00, 0x80]);
    rom
}

fn nes_video_rom() -> Vec<u8> {
    let mut rom = nes_rom();
    let prg = 16;
    rom[prg..prg + 9].copy_from_slice(&[0xA9, 0x1E, 0x8D, 0x01, 0x20, 0xEA, 0x4C, 0x05, 0x80]);
    rom
}

fn sega8_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[..3].copy_from_slice(&[0x00, 0x18, 0xFD]);
    rom
}

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn pce_write_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..13].copy_from_slice(&[
        0xD4, 0xA9, 0xF8, 0x53, 0x02, 0xA9, 0x00, 0x8D, 0x00, 0x20, 0x1A, 0x80, 0xFA,
    ]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn ws_rom() -> Vec<u8> {
    use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

    let mut rom = vec![0x90; 0x10000];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn ws_color_rom() -> Vec<u8> {
    use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

    let mut rom = ws_rom();
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn profile_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    profile_gb_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    profile_gba_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    profile_nes_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    profile_pce_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    profile_sega8_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    profile_ws_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );
}

fn profile_nes_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    for (label, rom) in [
        ("NES synthetic", nes_rom()),
        ("NES RAM writes", nes_ram_write_rom()),
        ("NES rendering", nes_video_rom()),
    ] {
        let mut nes =
            zeff_nes_core::emulator::Emulator::from_rom_data(&rom).expect("synthetic NES ROM");
        nes.set_apu_sample_generation_enabled(sample_generation_enabled);
        nes.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_frames(&format!("{label}{suffix}"), frames, &mut nes);

        let state = nes.encode_state().expect("encode synthetic NES state");
        let mut audio = Vec::new();
        nes.drain_audio_samples_into(&mut audio);
        print_accuracy_hashes(nes.framebuffer(), &state, &audio);
    }
}

fn profile_sega8_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    let mut sega = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &sega8_rom(),
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("synthetic Sega ROM");
    sega.set_apu_sample_generation_enabled(sample_generation_enabled);
    sega.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("Sega 8-bit synthetic{suffix}"), frames, &mut sega);

    let state = sega
        .encode_state()
        .expect("encode synthetic Sega 8-bit state");
    let mut audio = Vec::new();
    sega.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(sega.framebuffer(), &state, &audio);
}

fn profile_ws_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    let mut ws = zeff_ws_core::emulator::Emulator::from_rom_data(&ws_rom())
        .expect("synthetic WonderSwan ROM");
    ws.set_apu_sample_generation_enabled(sample_generation_enabled);
    ws.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_wonderswan_frames(&format!("WonderSwan synthetic{suffix}"), frames, &mut ws);

    let state = ws
        .encode_state()
        .expect("encode synthetic WonderSwan state");
    let mut audio = Vec::new();
    ws.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(ws.framebuffer(), &state, &audio);
}

fn profile_ws_video(frames: u32, sample_generation_enabled: bool) {
    let mut ws = zeff_ws_core::emulator::Emulator::from_rom_data(&ws_color_rom())
        .expect("synthetic WonderSwan Color ROM");
    let mut pattern = 0xA5A5_5A5A_u32;
    for address in 0..=0xFFFF {
        pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ws.cpu_write8(address, (pattern >> 24) as u8);
    }

    ws.io_write8(0x00, 0x07);
    ws.io_write8(0x04, 0x30);
    ws.io_write8(0x05, 0);
    ws.io_write8(0x06, 64);
    ws.io_write8(0x07, 0x21);
    ws.io_write8(0x10, 13);
    ws.io_write8(0x11, 29);
    ws.io_write8(0x12, 47);
    ws.io_write8(0x13, 71);
    ws.io_write8(0x14, 0x01);
    ws.io_write8(0x60, 0xC0);
    for sprite in 0..64_u32 {
        let entry = 0x6000 + sprite * 4;
        let tile = (sprite * 7) & 0x01FF;
        let attributes = ((sprite & 7) << 9)
            | ((sprite & 1) << 13)
            | ((sprite & 2) << 13)
            | ((sprite & 4) << 13);
        let tile_data = (tile | attributes) as u16;
        let [lo, hi] = tile_data.to_le_bytes();
        ws.cpu_write8(entry, lo);
        ws.cpu_write8(entry + 1, hi);
        ws.cpu_write8(entry + 2, ((sprite * 19) % 144) as u8);
        ws.cpu_write8(entry + 3, ((sprite * 37) % 224) as u8);
    }

    ws.set_apu_sample_generation_enabled(sample_generation_enabled);
    profile_wonderswan_frames(
        if sample_generation_enabled {
            "WonderSwan color video + audio"
        } else {
            "WonderSwan color video"
        },
        frames,
        &mut ws,
    );
    let first_pixel = &ws.framebuffer()[..4];
    assert!(
        ws.framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != first_pixel),
        "synthetic WonderSwan video fixture produced a flat frame"
    );
    let state = ws.encode_state().expect("encode WonderSwan video state");
    let mut audio = Vec::new();
    ws.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(ws.framebuffer(), &state, &audio);
}

fn profile_pce_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    let video_only = std::env::var("ZEFF_PROFILE_PCE_VIDEO_ONLY").as_deref() == Ok("1");
    if !video_only {
        let mut pce =
            zeff_pce_core::hardware::PceMachine::new(pce_rom()).expect("synthetic PCE ROM");
        pce.set_sample_generation_enabled(sample_generation_enabled);
        pce.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_pce_frames(&format!("PC Engine synthetic{suffix}"), frames, &mut pce);
        print_pce_accuracy_hashes(&mut pce);

        let mut pce_writes = zeff_pce_core::hardware::PceMachine::new(pce_write_rom())
            .expect("synthetic PCE write ROM");
        pce_writes.set_sample_generation_enabled(sample_generation_enabled);
        pce_writes.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_pce_frames(
            &format!("PC Engine RAM writes{suffix}"),
            frames,
            &mut pce_writes,
        );
        print_pce_accuracy_hashes(&mut pce_writes);
    }

    if video_only || std::env::var("ZEFF_PROFILE_PCE_VIDEO").as_deref() == Ok("1") {
        use zeff_pce_core::hardware::VdcRegister;
        use zeff_pce_core::hardware::cpu::VdcPort;

        let mut pce_video =
            zeff_pce_core::hardware::PceMachine::new(pce_rom()).expect("synthetic PCE video ROM");
        let vdc = pce_video.devices_mut().vdc_mut();
        for (register, value) in [
            (VdcRegister::Control, 0x0080),
            (VdcRegister::HorizontalDisplay, 31),
            (VdcRegister::VerticalSync, 0x0F02),
            (VdcRegister::VerticalDisplay, 0x00EF),
            (VdcRegister::VerticalDisplayEnd, 0x0004),
        ] {
            vdc.write_port(VdcPort::SelectOrStatus, register as u8);
            vdc.write_port(VdcPort::DataLow, value as u8);
            vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
        }
        pce_video.set_sample_generation_enabled(sample_generation_enabled);
        pce_video.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_pce_frames(
            &format!("PC Engine 240-line video{suffix}"),
            frames,
            &mut pce_video,
        );
        print_pce_accuracy_hashes(&mut pce_video);
    }

    if std::env::var("ZEFF_PROFILE_PCE_SPRITES").as_deref() == Ok("1") {
        use zeff_pce_core::hardware::cpu::VdcPort;
        use zeff_pce_core::hardware::{VcePort, VdcDmaChannel, VdcDmaProgress, VdcRegister};

        let mut pce_sprites =
            zeff_pce_core::hardware::PceMachine::new(pce_rom()).expect("synthetic PCE sprite ROM");
        let vdc = pce_sprites.devices_mut().vdc_mut();
        for (register, value) in [
            (VdcRegister::Control, 0x0040),
            (VdcRegister::HorizontalDisplay, 31),
            (VdcRegister::VerticalSync, 0x0F02),
            (VdcRegister::VerticalDisplay, 0x00EF),
            (VdcRegister::VerticalDisplayEnd, 0x0004),
            (VdcRegister::SatbSource, 0x7F00),
        ] {
            vdc.write_port(VdcPort::SelectOrStatus, register as u8);
            vdc.write_port(VdcPort::DataLow, value as u8);
            vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
        }
        let mut pattern = 0xA5A5_5A5A_u32;
        for word in vdc.vram_mut().iter_mut() {
            pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *word = (pattern >> 16) as u16;
        }
        for group in 0..4 {
            for column in 0..16 {
                let entry = (group * 16 + column) * 4;
                vdc.vram_mut()[0x7F00 + entry..0x7F00 + entry + 4].copy_from_slice(&[
                    (64 + group * 64) as u16,
                    (32 + column * 32) as u16,
                    (2 + column * 2) as u16,
                    0x3180,
                ]);
            }
        }
        assert!(vdc.start_satb_dma_for_vertical_blank());
        for _ in 0..255 {
            assert!(matches!(
                vdc.service_dma_slot(VdcDmaChannel::Satb),
                Ok(VdcDmaProgress::Transferred { .. })
            ));
        }
        assert_eq!(
            vdc.service_dma_slot(VdcDmaChannel::Satb),
            Ok(VdcDmaProgress::Complete)
        );
        let vce = pce_sprites.devices_mut().vce_mut();
        vce.write_port(VcePort::from_offset(2), 1);
        vce.write_port(VcePort::from_offset(3), 1);
        for color in 1..=15 {
            let raw = (color * 0x11) as u16;
            vce.write_port(VcePort::from_offset(4), raw as u8);
            vce.write_port(VcePort::from_offset(5), (raw >> 8) as u8);
        }
        pce_sprites.set_sample_generation_enabled(sample_generation_enabled);
        pce_sprites.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_pce_frames(
            &format!("PC Engine 240-line sprites{suffix}"),
            frames,
            &mut pce_sprites,
        );
        assert!(
            pce_sprites
                .framebuffer()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3] != [0, 0, 0]),
            "synthetic PCE sprite fixture did not produce visible pixels"
        );
        print_pce_accuracy_hashes(&mut pce_sprites);
    }
}

fn profile_manifest_roms(frames: u32) {
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    let test_roms = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-roms");
    for (label, rom_path) in load_manifest("gb-bench-roms.txt") {
        let Ok(data) = std::fs::read(test_roms.join(rom_path)) else {
            eprintln!("skip {label}: not found");
            continue;
        };
        let Ok(mut emulator) =
            zeff_gb_core::emulator::Emulator::from_rom_data(&data, HardwareModePreference::Auto)
        else {
            eprintln!("skip {label}: load failed");
            continue;
        };
        emulator.set_apu_sample_generation_enabled(false);
        profile_frames(&label, frames, &mut emulator);
    }

    for (label, rom_path) in load_manifest("nes-bench-roms.txt") {
        let Ok(data) = std::fs::read(test_roms.join(rom_path)) else {
            eprintln!("skip {label}: not found");
            continue;
        };
        let Ok(mut emulator) = zeff_nes_core::emulator::Emulator::from_rom_data(&data) else {
            eprintln!("skip {label}: load failed");
            continue;
        };
        emulator.set_apu_sample_generation_enabled(false);
        profile_frames(&label, frames, &mut emulator);
    }
}

fn main() {
    let frames = std::env::var("ZEFF_PROFILE_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&value| value > 0)
        .unwrap_or(DEFAULT_FRAMES);

    if std::env::var("ZEFF_PROFILE_PCE_STATE").as_deref() == Ok("1") {
        let iterations = std::env::var("ZEFF_PROFILE_STATE_ITERATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|&value| value > 0)
            .unwrap_or(100);
        profile_pce_state(iterations);
        return;
    }

    if std::env::var("ZEFF_PROFILE_WS_VIDEO_ONLY").as_deref() == Ok("1") {
        profile_ws_video(
            frames,
            std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1"),
        );
        return;
    }

    if std::env::var("ZEFF_PROFILE_SEGA8_SPRITES_ONLY").as_deref() == Ok("1") {
        profile_sega8_sprites(
            frames,
            std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1"),
        );
        return;
    }

    if std::env::var("ZEFF_PROFILE_GBA_ACTIVE_VIDEO_ONLY").as_deref() == Ok("1") {
        profile_gba_active_video(
            frames,
            std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1"),
        );
        return;
    }

    if let Ok(core) = std::env::var("ZEFF_PROFILE_CORE") {
        let sample_generation_enabled = std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1");
        let instruction_trace_enabled = std::env::var("ZEFF_PROFILE_TRACE").as_deref() == Ok("1");
        let suffix = match (sample_generation_enabled, instruction_trace_enabled) {
            (false, false) => "",
            (true, false) => " + audio",
            (false, true) => " + trace",
            (true, true) => " + audio + trace",
        };

        match core.as_str() {
            "all" => profile_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "gb" => profile_gb_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "gba" => profile_gba_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "nes" => profile_nes_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "pce" => profile_pce_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "sega8" => profile_sega8_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            "ws" => profile_ws_synthetic(
                frames,
                sample_generation_enabled,
                instruction_trace_enabled,
                suffix,
            ),
            _ => panic!("unknown core {core:?}; expected all, gb, gba, nes, pce, sega8, or ws"),
        }
        return;
    }

    if std::env::var("ZEFF_PROFILE_PCE_ONLY").as_deref() == Ok("1") {
        profile_pce_synthetic(frames, false, false, "");
        if std::env::var("ZEFF_PROFILE_COMPARE_AUDIO").as_deref() == Ok("1") {
            profile_pce_synthetic(frames, true, false, " + audio");
        }
        if std::env::var("ZEFF_PROFILE_COMPARE_TRACE").as_deref() == Ok("1") {
            profile_pce_synthetic(frames, false, true, " + trace");
        }
        return;
    }

    if std::env::var("ZEFF_PROFILE_GB_ONLY").as_deref() == Ok("1") {
        let sample_generation_enabled = std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1");
        profile_gb_synthetic(
            frames,
            sample_generation_enabled,
            false,
            if sample_generation_enabled {
                " + audio"
            } else {
                ""
            },
        );
        return;
    }

    if std::env::var("ZEFF_PROFILE_SEGA8_VIDEO_ONLY").as_deref() == Ok("1") {
        profile_sega8_video(
            frames,
            std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1"),
        );
        return;
    }

    if std::env::var("ZEFF_PROFILE_GBA_ONLY").as_deref() == Ok("1") {
        let sample_generation_enabled = std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1");
        profile_gba_synthetic(
            frames,
            sample_generation_enabled,
            false,
            if sample_generation_enabled {
                " + audio"
            } else {
                ""
            },
        );
        return;
    }

    println!("=== Synthetic core baseline ===");
    profile_synthetic(frames, false, false, "");
    if std::env::var("ZEFF_PROFILE_COMPARE_AUDIO").as_deref() == Ok("1") {
        println!("\n=== Synthetic audio-output comparison ===");
        profile_synthetic(frames, true, false, " + audio");
    }
    if std::env::var("ZEFF_PROFILE_COMPARE_TRACE").as_deref() == Ok("1") {
        println!("\n=== Synthetic instruction-trace comparison ===");
        profile_synthetic(frames, false, true, " + trace");
    }
    if std::env::var("ZEFF_PROFILE_TRACE_STORE").as_deref() == Ok("1") {
        println!("\n=== Instruction trace store ===");
        profile_trace_store();
    }
    if std::env::var("ZEFF_PROFILE_MANIFESTS").as_deref() == Ok("1") {
        println!("\n=== Manifest ROM baseline ===");
        profile_manifest_roms(frames);
    }
}
