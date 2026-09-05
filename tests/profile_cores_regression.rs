#![cfg(all(feature = "profile-cores", not(target_arch = "wasm32")))]

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FRAMES: u32 = 120;
const PCE_AUDIO_FRAMES: u32 = 360;

#[test]
fn synthetic_workloads_stay_allocation_free() {
    let binary = std::env::var_os("CARGO_BIN_EXE_profile_cores")
        .expect("Cargo should provide the profile_cores binary path");
    let output = Command::new(binary)
        .env("ZEFF_PROFILE_FRAMES", FRAMES.to_string())
        .env("ZEFF_MUTE_AUDIO", "1")
        .env_remove("ZEFF_PROFILE_COMPARE_AUDIO")
        .env_remove("ZEFF_PROFILE_COMPARE_TRACE")
        .env_remove("ZEFF_PROFILE_MANIFESTS")
        .env_remove("ZEFF_PROFILE_TRACE_STORE")
        .output()
        .expect("profile_cores should run");

    assert!(
        output.status.success(),
        "profile_cores failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("profile_cores stdout should be UTF-8");
    for label in [
        "GB synthetic",
        "GBA synthetic",
        "NES synthetic",
        "ColecoVision synthetic",
        "Sega 8-bit synthetic",
        "WonderSwan synthetic",
    ] {
        assert_zero_allocations(&stdout, label);
    }
    assert_wonderswan_counters(&stdout);
}

#[test]
fn pce_audio_profile_drains_each_frame() {
    let binary = std::env::var_os("CARGO_BIN_EXE_profile_cores")
        .expect("Cargo should provide the profile_cores binary path");
    let output = Command::new(binary)
        .env("ZEFF_PROFILE_CORE", "pce")
        .env("ZEFF_PROFILE_AUDIO", "1")
        .env("ZEFF_PROFILE_FRAMES", PCE_AUDIO_FRAMES.to_string())
        .env("ZEFF_MUTE_AUDIO", "1")
        .env_remove("ZEFF_PROFILE_PCE_STATE")
        .env_remove("ZEFF_PROFILE_PCE_VIDEO_ONLY")
        .env_remove("ZEFF_PROFILE_PCE_VIDEO")
        .env_remove("ZEFF_PROFILE_PCE_SPRITES")
        .env_remove("ZEFF_PROFILE_PCE_ROM_PATH")
        .output()
        .expect("profile_cores should run");

    assert!(
        output.status.success(),
        "profile_cores failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("profile_cores stdout should be UTF-8");
    for label in [
        "PC Engine synthetic + audio",
        "PC Engine RAM writes + audio",
    ] {
        assert_zero_allocations(&stdout, label);
    }
}

#[test]
fn pce_profile_accepts_a_supplied_rom() {
    let binary = std::env::var_os("CARGO_BIN_EXE_profile_cores")
        .expect("Cargo should provide the profile_cores binary path");
    let path = std::env::temp_dir().join(format!(
        "zeff-profile-pce-{}-{}.pce",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos()
    ));
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    std::fs::write(&path, rom).expect("write supplied PCE profile ROM");
    let path_text = path.to_string_lossy().into_owned();
    let file_name = path.file_name().unwrap().to_string_lossy().into_owned();

    let output = Command::new(binary)
        .env("ZEFF_PROFILE_CORE", "pce")
        .env("ZEFF_PROFILE_FRAMES", FRAMES.to_string())
        .env("ZEFF_PROFILE_WARMUP_FRAMES", "1")
        .env("ZEFF_PROFILE_PCE_ROM_PATH", &path)
        .env("ZEFF_MUTE_AUDIO", "1")
        .env_remove("ZEFF_PROFILE_AUDIO")
        .env_remove("ZEFF_PROFILE_PCE_STATE")
        .env_remove("ZEFF_PROFILE_PCE_VIDEO_ONLY")
        .env_remove("ZEFF_PROFILE_PCE_VIDEO")
        .env_remove("ZEFF_PROFILE_PCE_SPRITES")
        .env_remove("ZEFF_PROFILE_TRACE")
        .output()
        .expect("profile_cores should run");
    std::fs::remove_file(&path).expect("remove supplied PCE profile ROM");

    assert!(
        output.status.success(),
        "profile_cores failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("profile_cores stdout should be UTF-8");
    assert_zero_allocations(&stdout, "PC Engine supplied ROM");
    assert!(!stdout.contains("PC Engine RAM writes"));
    assert!(!stdout.contains(&path_text));
    assert!(!stdout.contains(&file_name));
}

fn assert_zero_allocations(output: &str, label: &str) {
    let (_, output) = output
        .split_once(label)
        .unwrap_or_else(|| panic!("missing {label} profile output"));
    let line = output
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("missing {label} allocation output"));
    let fields: Vec<_> = line.split_whitespace().collect();

    assert_eq!(fields, ["0", "alloc", "0", "realloc", "0.0", "KiB"]);
}

fn assert_wonderswan_counters(output: &str) {
    let calls = number_fields(output, "  WonderSwan calls:");
    assert_eq!(calls, vec![4_883_962; 5]);

    let transitions = number_fields(output, "  WonderSwan transitions:");
    assert_eq!(
        transitions,
        [
            4_884_480,
            19_080,
            FRAMES.into(),
            FRAMES.into(),
            19_080,
            FRAMES.into()
        ]
    );
}

fn number_fields(output: &str, prefix: &str) -> Vec<u64> {
    output
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} profile output"))
        .split_whitespace()
        .filter_map(|field| field.parse().ok())
        .collect()
}
