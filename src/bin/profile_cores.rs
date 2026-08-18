use std::path::Path;
use std::time::Instant;

const DEFAULT_FRAMES: u32 = 3_000;

trait ProfileMachine {
    fn profile_step_frame(&mut self);
    fn profile_ticks(&self) -> u64;
    fn tick_label() -> &'static str;
}

impl ProfileMachine for zeff_gb_core::emulator::Emulator {
    fn profile_step_frame(&mut self) {
        self.step_frame();
    }

    fn profile_ticks(&self) -> u64 {
        self.timing_snapshot().now().get()
    }

    fn tick_label() -> &'static str {
        "master ticks"
    }
}

impl ProfileMachine for zeff_gba_core::emulator::Emulator {
    fn profile_step_frame(&mut self) {
        self.step_frame();
    }

    fn profile_ticks(&self) -> u64 {
        self.timing_snapshot().now().get()
    }

    fn tick_label() -> &'static str {
        "master ticks"
    }
}

impl ProfileMachine for zeff_nes_core::emulator::Emulator {
    fn profile_step_frame(&mut self) {
        self.step_frame();
    }

    fn profile_ticks(&self) -> u64 {
        self.timing_snapshot().now().get()
    }

    fn tick_label() -> &'static str {
        "master ticks"
    }
}

impl ProfileMachine for zeff_sega8_core::emulator::Emulator {
    fn profile_step_frame(&mut self) {
        self.step_frame();
    }

    fn profile_ticks(&self) -> u64 {
        self.timing_snapshot().now().get()
    }

    fn tick_label() -> &'static str {
        "master ticks"
    }
}

impl ProfileMachine for zeff_ws_core::emulator::Emulator {
    fn profile_step_frame(&mut self) {
        self.step_frame();
    }

    fn profile_ticks(&self) -> u64 {
        self.timing_snapshot().now().get()
    }

    fn tick_label() -> &'static str {
        "master ticks"
    }
}

fn profile_frames<M: ProfileMachine>(label: &str, frames: u32, machine: &mut M) {
    for _ in 0..10 {
        machine.profile_step_frame();
    }

    let start_ticks = machine.profile_ticks();
    let start = Instant::now();
    for _ in 0..frames {
        machine.profile_step_frame();
    }
    let elapsed = start.elapsed();
    let elapsed_ticks = machine.profile_ticks().wrapping_sub(start_ticks);
    let fps = f64::from(frames) / elapsed.as_secs_f64();
    let million_ticks_per_second = elapsed_ticks as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!(
        "{label:30} {frames:5} frames  {elapsed:>9.2?}  {fps:>8.0} fps  {million_ticks_per_second:>8.2} M {} / s",
        M::tick_label()
    );
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
    profile_frames(&format!("WonderSwan synthetic{suffix}"), frames, &mut ws);
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
    if std::env::var("ZEFF_PROFILE_MANIFESTS").as_deref() == Ok("1") {
        println!("\n=== Manifest ROM baseline ===");
        profile_manifest_roms(frames);
    }
}
