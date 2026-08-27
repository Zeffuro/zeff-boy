#![cfg(all(feature = "profile-cores", not(target_arch = "wasm32")))]

use std::process::Command;

const FRAMES: u32 = 120;

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
