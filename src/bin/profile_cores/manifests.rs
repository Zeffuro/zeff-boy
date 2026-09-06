use std::path::Path;

use super::{print_accuracy_hashes, profile_frames, profile_gba_frames};

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

pub(super) fn profile_manifest_roms(frames: u32) {
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

    profile_gba_manifest_roms(frames);
}

pub(super) fn profile_gba_manifest_roms(frames: u32) {
    let test_roms = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-roms");
    let gba_bios = std::env::var_os("ZEFF_GBA_BIOS_PATH").and_then(|path| std::fs::read(path).ok());
    let sample_generation_enabled = std::env::var("ZEFF_PROFILE_AUDIO").as_deref() == Ok("1");
    for (label, rom_path) in load_manifest("gba-bench-roms.txt") {
        let Ok(data) = std::fs::read(test_roms.join(rom_path)) else {
            eprintln!("skip {label}: not found");
            continue;
        };
        let emulator = if let Some(bios) = gba_bios.as_deref() {
            zeff_gba_core::emulator::Emulator::new_with_bios(&data, bios, 48_000)
        } else {
            zeff_gba_core::emulator::Emulator::from_rom_data(&data)
        };
        let Ok(mut emulator) = emulator else {
            eprintln!("skip {label}: load failed");
            continue;
        };
        emulator.set_apu_sample_generation_enabled(sample_generation_enabled);
        let label = if sample_generation_enabled {
            format!("{label} + audio")
        } else {
            label
        };
        profile_gba_frames(&label, frames, &mut emulator);
        let state = emulator.encode_state().expect("encode manifest GBA state");
        let mut audio = Vec::new();
        emulator.drain_audio_samples_into(&mut audio);
        print_accuracy_hashes(emulator.framebuffer(), &state, &audio);
    }
}
