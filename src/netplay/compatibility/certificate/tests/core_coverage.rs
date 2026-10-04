use super::*;

fn core_payload() -> Value {
    let mut value = payload();
    value["format"] = json!(2);
    value["games"] = json!([]);
    value["core"] = json!("nes-standard-portable-rollback-v1");
    value
}

#[test]
fn core_coverage_accepts_unlisted_content_only_with_portable_hardware() {
    let pair = parse(&core_payload(), &facts()).unwrap();
    for mapper in [0, 1, 3, 5, 7, 16, 19, 69, 206] {
        for submapper in [0, 1, 4] {
            for timing in 0..3 {
                assert!(pair.permits_content([0xaa; 32], mapper, submapper, timing, true));
                assert!(!pair.permits_content([0xaa; 32], mapper, submapper, timing, false));
            }
        }
    }
    assert!(!pair.permits_content([0xaa; 32], 0, 0, 3, true));
    assert!(!pair.permits_game([0xaa; 32], 0, 0, 0));
    let summary = serde_json::to_value(pair.summary()).unwrap();
    assert_eq!(summary["core"], "nes-standard-portable-rollback-v1");
    assert_eq!(summary["games"], json!([]));
}

#[test]
fn core_coverage_does_not_relax_artifact_source_or_signature_checks() {
    let value = core_payload();
    for change in 0..3 {
        let mut local = facts();
        match change {
            0 => local.artifact = [0xff; 32],
            1 => local.source = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            _ => local.target = LINUX,
        }
        assert!(parse(&value, &local).is_err());
    }
    let mut changed = value.clone();
    changed["members"][1]["source"] = json!("ff".repeat(32));
    assert!(parse(&changed, &facts()).is_err());
    let mut envelope: Value = serde_json::from_slice(&signed(&value.to_string())).unwrap();
    envelope["payload"] = json!(value.to_string().replace("portable", "unrestricted"));
    assert!(
        verify_with_key(
            &serde_json::to_vec(&envelope).unwrap(),
            &facts(),
            &key().verifying_key(),
        )
        .is_err()
    );
}

#[test]
fn core_and_game_coverage_cannot_be_combined_or_silently_upgraded() {
    for (field, replacement) in [
        ("format", json!(1)),
        ("format", json!(3)),
        ("core", Value::Null),
        ("core", json!("any-core")),
        ("games", payload()["games"].clone()),
    ] {
        let mut value = core_payload();
        value[field] = replacement;
        assert!(parse(&value, &facts()).is_err(), "{field}");
    }
    let mut legacy = payload();
    legacy["core"] = core_payload()["core"].clone();
    assert!(parse(&legacy, &facts()).is_err());
    let pair = parse(&payload(), &facts()).unwrap();
    assert!(!pair.permits_content([0xaa; 32], 1, 0, 0, true));
    assert!(pair.permits_content([0x45; 32], 0, 0, 0, false));
    assert!(
        serde_json::to_value(pair.summary())
            .unwrap()
            .get("core")
            .is_none()
    );
}
