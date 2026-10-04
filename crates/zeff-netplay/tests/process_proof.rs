#![cfg(all(not(target_arch = "wasm32"), feature = "native-proof"))]

use std::process::Command;

#[test]
fn remote_proof_rejects_invalid_roles_addresses_and_identity_override() {
    let secret = "ab".repeat(32);
    for args in [
        vec!["--listen", "0.0.0.0:0"],
        vec!["--listen", "8.8.8.8:1234"],
        vec!["--network-peer", "192.168.1.1:0"],
        vec!["--network-peer", "[::ffff:192.168.1.1]:1234"],
        vec!["--listen", "127.0.0.1:0", "--network-peer", "127.0.0.1:1"],
        vec!["--test-build", &secret],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zeff-netplay-proof"))
            .args(args)
            .env("ZEFF_NETPLAY_PROOF_SECRET", &secret)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&secret));
    }
}

#[test]
fn separate_processes_qualify_lockstep_and_fault_boundaries() {
    let output = Command::new(env!("CARGO_BIN_EXE_zeff-netplay-proof"))
        .args(["--frames", "24", "--scenario", "all"])
        .env("ZEFF_MUTE_AUDIO", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reports: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(reports.len(), 13);
    for report in reports {
        assert_eq!(report["passed"], true, "{report}");
        assert_eq!(report["persistence"], "leased-discard");
        match report["scenario"].as_str().unwrap() {
            "normal" | "jitter" | "duplicate" | "stale" => {
                assert_eq!(report["frames"], 24);
                assert_eq!(report["reference_checked_frames"], 24);
            }
            "identity" | "wrong-secret" => assert_eq!(report["frames"], 0),
            "desync" | "conflict" | "flood" => assert_eq!(report["frames"], 6),
            _ => assert_eq!(report["frames"], 5),
        }
    }
}
