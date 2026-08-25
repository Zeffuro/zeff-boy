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

fn profile_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    let mut gb =
        zeff_gb_core::emulator::Emulator::from_rom_data(&gb_rom(), HardwareModePreference::Auto)
            .expect("synthetic GB ROM");
    gb.set_apu_sample_generation_enabled(sample_generation_enabled);
    gb.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("GB synthetic{suffix}"), frames, &mut gb);

    let mut gba =
        zeff_gba_core::emulator::Emulator::from_rom_data(&gba_rom()).expect("synthetic GBA ROM");
    gba.set_apu_sample_generation_enabled(sample_generation_enabled);
    gba.set_apu_debug_capture_enabled(false);
    gba.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("GBA synthetic{suffix}"), frames, &mut gba);

    let mut nes =
        zeff_nes_core::emulator::Emulator::from_rom_data(&nes_rom()).expect("synthetic NES ROM");
    nes.set_apu_sample_generation_enabled(sample_generation_enabled);
    nes.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("NES synthetic{suffix}"), frames, &mut nes);

    profile_pce_synthetic(
        frames,
        sample_generation_enabled,
        instruction_trace_enabled,
        suffix,
    );

    let mut sega = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &sega8_rom(),
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("synthetic Sega ROM");
    sega.set_apu_sample_generation_enabled(sample_generation_enabled);
    sega.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_frames(&format!("Sega 8-bit synthetic{suffix}"), frames, &mut sega);

    let mut ws = zeff_ws_core::emulator::Emulator::from_rom_data(&ws_rom())
        .expect("synthetic WonderSwan ROM");
    ws.set_apu_sample_generation_enabled(sample_generation_enabled);
    ws.set_instruction_trace_enabled(instruction_trace_enabled);
    profile_wonderswan_frames(&format!("WonderSwan synthetic{suffix}"), frames, &mut ws);
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

        let mut pce_writes = zeff_pce_core::hardware::PceMachine::new(pce_write_rom())
            .expect("synthetic PCE write ROM");
        pce_writes.set_sample_generation_enabled(sample_generation_enabled);
        pce_writes.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_pce_frames(
            &format!("PC Engine RAM writes{suffix}"),
            frames,
            &mut pce_writes,
        );
    }

    if video_only || std::env::var("ZEFF_PROFILE_PCE_VIDEO").as_deref() == Ok("1") {
        use sha2::{Digest, Sha256};
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
        let framebuffer_hash = Sha256::digest(pce_video.framebuffer());
        let state = zeff_pce_core::hardware::save_state::encode_state(&pce_video)
            .expect("encode synthetic PCE video state");
        let state_hash = Sha256::digest(&state);
        let framebuffer_hash = framebuffer_hash
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let state_hash = state_hash
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        println!("  framebuffer {framebuffer_hash}  state {state_hash}");
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
