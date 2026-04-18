use std::path::Path;
use std::time::Instant;

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
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (label, path) = l.split_once('\t')?;
            Some((label.trim().to_string(), path.trim().to_string()))
        })
        .collect()
}

const FRAMES: u32 = 3000;

fn profile_gb() {
    use zeff_gb_core::emulator::Emulator;
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    let test_roms = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-roms");
    for (label, rom_path) in load_manifest("gb-bench-roms.txt") {
        let full = test_roms.join(&rom_path);
        let Ok(data) = std::fs::read(&full) else {
            eprintln!("skip {label}: not found");
            continue;
        };
        let Ok(mut emu) = Emulator::from_rom_data(&data, HardwareModePreference::Auto) else {
            eprintln!("skip {label}: load failed");
            continue;
        };

        let start = Instant::now();
        for _ in 0..FRAMES {
            emu.step_frame();
        }
        let elapsed = start.elapsed();
        let fps = FRAMES as f64 / elapsed.as_secs_f64();
        println!("{label:30} {FRAMES} frames in {elapsed:.2?}  ({fps:.0} fps)");
    }
}

fn profile_nes() {
    use zeff_nes_core::emulator::Emulator;

    let test_roms = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-roms");
    for (label, rom_path) in load_manifest("nes-bench-roms.txt") {
        let full = test_roms.join(&rom_path);
        let Ok(data) = std::fs::read(&full) else {
            eprintln!("skip {label}: not found");
            continue;
        };
        let Ok(mut emu) = Emulator::new(&data, 44_100.0) else {
            eprintln!("skip {label}: load failed");
            continue;
        };

        let start = Instant::now();
        for _ in 0..FRAMES {
            emu.step_frame();
        }
        let elapsed = start.elapsed();
        let fps = FRAMES as f64 / elapsed.as_secs_f64();
        println!("{label:30} {FRAMES} frames in {elapsed:.2?}  ({fps:.0} fps)");
    }
}

fn main() {
    println!("=== GB/GBC Profiling ({FRAMES} frames each) ===");
    profile_gb();
    println!();
    println!("=== NES Profiling ({FRAMES} frames each) ===");
    profile_nes();
}
