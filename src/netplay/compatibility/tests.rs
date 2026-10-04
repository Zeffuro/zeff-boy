use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

fn table() -> String {
    serde_json::json!({"format": 1, "builds": [{
        "source": "12".repeat(32), "receipt": "34".repeat(32),
        "platform": 1, "test": true, "hardware": HARDWARE, "contract": "56".repeat(32)
    }]})
    .to_string()
}

#[test]
fn qualification_requires_exact_source_receipt_platform_and_execution_kind() {
    let source = "12".repeat(32);
    let receipt = "34".repeat(32);
    assert_eq!(
        qualified_contract(&table(), &source, Some(&receipt), 1, true),
        Some([0x56; 32])
    );
    for (source, receipt, platform, test) in [
        ("98".repeat(32), Some(receipt.clone()), 1, true),
        (source.clone(), Some("98".repeat(32)), 1, true),
        (source.clone(), None, 1, true),
        (source.clone(), Some(receipt.clone()), 2, true),
        (source.clone(), Some(receipt.clone()), 1, false),
        (source.clone(), Some(receipt.clone()), 0, true),
        (source.clone(), Some(receipt.clone()), 3, true),
    ] {
        assert_eq!(
            qualified_contract(&table(), &source, receipt.as_deref(), platform, test),
            None
        );
    }
}

#[test]
fn malformed_unknown_and_duplicate_qualification_rows_fail_closed() {
    let source = "12".repeat(32);
    let receipt = "34".repeat(32);
    let mut row: serde_json::Value = serde_json::from_str(&table()).unwrap();
    for (field, value) in [
        ("source", serde_json::json!("not a digest")),
        ("receipt", serde_json::json!("CD".repeat(32))),
        ("contract", serde_json::json!("00".repeat(32))),
        ("platform", serde_json::json!(3)),
        ("hardware", serde_json::json!("any-nes")),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut changed = row.clone();
        changed["builds"][0][field] = value;
        assert_eq!(
            qualified_contract(&changed.to_string(), &source, Some(&receipt), 1, true),
            None
        );
    }
    let duplicate = row["builds"][0].clone();
    row["builds"].as_array_mut().unwrap().push(duplicate);
    assert_eq!(
        qualified_contract(&row.to_string(), &source, Some(&receipt), 1, true),
        None
    );
    for invalid in [
        "{}".to_owned(),
        table().replace("\"format\":1", "\"format\":2"),
        "x".repeat(32 * 1024 + 1),
    ] {
        assert_eq!(
            qualified_contract(&invalid, &source, Some(&receipt), 1, true),
            None
        );
    }
}

#[test]
fn unqualified_hardware_keeps_actual_version_and_explicit_consent_without_contract() {
    let directory = crate::test_support::test_directory("netplay-compatibility-hardware").unwrap();
    let mut rom = crate::test_support::build_nes_test_rom();
    let path = directory.path().join("game.nes");
    std::fs::write(&path, &rom).unwrap();
    let nrom = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(supported_hardware(&nrom));
    assert!(!available());
    let ordinary = describe(&nrom, true);
    assert_eq!(ordinary.contract, [0; 32]);
    assert_eq!(ordinary.platform, 0);
    assert!(ordinary.allow_different_versions);
    rom[9] = 1;
    std::fs::write(&path, &rom).unwrap();
    let pal = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(!supported_hardware(&pal));
    let build = describe(&pal, true);
    assert_eq!(build.contract, [0; 32]);
    assert_eq!(build.platform, 0);
    rom[9] = 0;
    rom[7] = 0x08;
    rom[12] = 3;
    std::fs::write(&path, &rom).unwrap();
    let dendy = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(!supported_hardware(&dendy));
    let build = describe(&dendy, true);
    assert_eq!(build.contract, [0; 32]);
    assert_eq!(build.platform, 0);
    rom[7] = 0;
    rom[12] = 0;
    rom[6] |= 0x10;
    std::fs::write(&path, &rom).unwrap();
    let mmc1 = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(!supported_hardware(&mmc1));
    let build = describe(&mmc1, true);
    assert_eq!(build.version, env!("CARGO_PKG_VERSION"));
    assert!(build.allow_different_versions);
    assert_eq!(build.contract, [0; 32]);
    assert_eq!(build.platform, 0);
}
