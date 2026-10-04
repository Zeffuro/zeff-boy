use super::*;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};

mod core_coverage;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}

fn payload() -> Value {
    json!({
        "format": 1, "contract": "56".repeat(32), "evidence": "78".repeat(32),
        "fp_controls": REQUIRED_CONTROLS,
        "members": [
            {"artifact": "12".repeat(32), "source": "34".repeat(32), "version": "1.0", "target": WINDOWS,
                "test": true, "profile": "debug", "opt_level": "0", "debug": "true"},
            {"artifact": "23".repeat(32), "source": "34".repeat(32), "version": "1.0", "target": LINUX,
                "test": false, "profile": "release", "opt_level": "3", "debug": "false"}
        ],
        "games": [
            {"source": "45".repeat(32), "mapper": 0, "submapper": 0, "timing": 0},
            {"source": "46".repeat(32), "mapper": 34, "submapper": 0, "timing": 1},
            {"source": "47".repeat(32), "mapper": 34, "submapper": 0, "timing": 2},
            {"source": "48".repeat(32), "mapper": 2, "submapper": 0, "timing": 0},
            {"source": "49".repeat(32), "mapper": 4, "submapper": 0, "timing": 0}
        ]
    })
}

fn facts() -> LocalFacts<'static> {
    LocalFacts {
        artifact: [0x12; 32],
        source: "3434343434343434343434343434343434343434343434343434343434343434",
        version: "1.0",
        target: WINDOWS,
        test: true,
        profile: "debug",
        opt_level: "0",
        debug: "true",
    }
}

fn signed(payload: &str) -> Vec<u8> {
    let signature = key().sign(&[DOMAIN, payload.as_bytes()].concat());
    serde_json::to_vec(&json!({"format": 1, "payload": payload, "signature": const_hex::encode(signature.to_bytes())})).unwrap()
}

fn parse(payload: &Value, facts: &LocalFacts<'_>) -> Result<VerifiedPair> {
    verify_with_key(&signed(&payload.to_string()), facts, &key().verifying_key())
}

#[test]
fn exact_members_select_mutual_peer_and_only_listed_games() {
    let pair = parse(&payload(), &facts()).unwrap();
    let summary = pair.summary();
    assert_eq!(summary.peer_build, [0x23; 32]);
    assert_eq!(summary.contract, [0x56; 32]);
    assert_eq!(summary.evidence, [0x78; 32]);
    assert!(!pair.different_versions());
    for (source, mapper, timing) in [
        (0x45, 0, 0),
        (0x46, 34, 1),
        (0x47, 34, 2),
        (0x48, 2, 0),
        (0x49, 4, 0),
    ] {
        assert!(pair.permits_game([source; 32], mapper, 0, timing));
        assert!(!pair.permits_game([source; 32], mapper, 1, timing));
        assert!(!pair.permits_game([source; 32], mapper, 0, (timing + 1) % 3));
        assert!(!pair.permits_game([source; 32], mapper + 1, 0, timing));
    }
    for mapper in [0, 2, 4, 34] {
        assert!(!pair.permits_game([0x50; 32], mapper, 0, 0));
    }
    let linux = LocalFacts {
        artifact: [0x23; 32],
        target: LINUX,
        test: false,
        profile: "release",
        opt_level: "3",
        debug: "false",
        ..facts()
    };
    let remote = parse(&payload(), &linux).unwrap();
    assert_eq!(remote.summary().certificate, summary.certificate);
    assert_eq!(remote.summary().peer_build, [0x12; 32]);
    let mut different = payload();
    different["members"][1]["version"] = json!("2.0");
    assert!(parse(&different, &facts()).unwrap().different_versions());
}

#[test]
fn certificate_identity_binds_raw_payload_and_evidence_separately_from_contract() {
    let payload = payload();
    let encoded = payload.to_string();
    let pair = verify_with_key(&signed(&encoded), &facts(), &key().verifying_key()).unwrap();
    let padded = format!(" {encoded}");
    let alternate = verify_with_key(&signed(&padded), &facts(), &key().verifying_key()).unwrap();
    assert_eq!(pair.summary().contract, alternate.summary().contract);
    assert_ne!(pair.summary().certificate, alternate.summary().certificate);
    let mut changed = payload;
    changed["evidence"] = json!("79".repeat(32));
    assert_ne!(
        pair.summary().certificate,
        parse(&changed, &facts()).unwrap().summary().certificate
    );
}

#[test]
fn signature_payload_key_and_envelope_tampering_are_refused() {
    let bytes = signed(&payload().to_string());
    assert!(verify(&bytes, &facts()).is_err());
    let mut envelope: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, value) in [
        ("format", json!(2)),
        ("signature", json!("00".repeat(64))),
        ("signature", json!("AB".repeat(64))),
        ("payload", json!("{}")),
        ("unknown", json!(true)),
    ] {
        let mut changed = envelope.clone();
        changed[field] = value;
        assert!(
            verify_with_key(
                &serde_json::to_vec(&changed).unwrap(),
                &facts(),
                &key().verifying_key()
            )
            .is_err()
        );
    }
    envelope.as_object_mut().unwrap().remove("signature");
    assert!(
        verify_with_key(
            &serde_json::to_vec(&envelope).unwrap(),
            &facts(),
            &key().verifying_key()
        )
        .is_err()
    );
    let duplicate = String::from_utf8(bytes)
        .unwrap()
        .replacen("{", "{\"format\":1,", 1);
    assert!(verify_with_key(duplicate.as_bytes(), &facts(), &key().verifying_key()).is_err());
}

#[test]
fn every_actual_artifact_and_compiled_fact_must_match() {
    for field in 0..8 {
        let mut changed = facts();
        match field {
            0 => changed.artifact = [0x99; 32],
            1 => {
                changed.source = "9999999999999999999999999999999999999999999999999999999999999999"
            }
            2 => changed.version = "2.0",
            3 => changed.target = LINUX,
            4 => changed.test = false,
            5 => changed.profile = "release",
            6 => changed.opt_level = "3",
            7 => changed.debug = "false",
            _ => unreachable!(),
        }
        assert!(parse(&payload(), &changed).is_err(), "field {field}");
    }
}

#[test]
fn signed_invalid_duplicate_unknown_and_zero_fields_fail_closed() {
    for (pointer, value) in [
        ("/format", json!(2)),
        ("/contract", json!("00".repeat(32))),
        ("/evidence", json!("00".repeat(32))),
        ("/fp_controls", json!(REQUIRED_CONTROLS ^ 0x2000)),
        ("/members/0/artifact", json!("00".repeat(32))),
        ("/members/0/source", json!("AB".repeat(32))),
        ("/members/1/source", json!("35".repeat(32))),
        ("/members/1/artifact", json!("12".repeat(32))),
        ("/members/1/target", json!(WINDOWS)),
        ("/members/1/target", json!("aarch64-unknown-linux-gnu")),
        ("/members/0/version", json!("")),
        ("/members/0/version", json!("1.0 beta")),
        ("/members/0/profile", json!("")),
        ("/members/0/opt_level", json!("4")),
        ("/members/0/debug", json!(true)),
        ("/games/0/source", json!("00".repeat(32))),
        ("/games/0/mapper", json!(1)),
        ("/games/0/mapper", json!(7)),
        ("/games/0/submapper", json!(1)),
        ("/games/0/timing", json!(3)),
        ("/games", json!([])),
    ] {
        let mut changed = payload();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&changed, &facts()).is_err(), "{pointer}");
    }
    for pointer in ["", "/members/0", "/games/0"] {
        let mut changed = payload();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(parse(&changed, &facts()).is_err());
    }
    for field in ["members", "games"] {
        let mut changed = payload();
        let duplicate = changed[field][0].clone();
        changed[field].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&changed, &facts()).is_err());
    }
    let mut too_many = payload();
    too_many["games"] = Value::Array((0..17).map(|index| json!({"source": format!("{index:064x}"), "mapper": 0, "submapper": 0, "timing": 0})).collect());
    assert!(parse(&too_many, &facts()).is_err());
    let duplicate = payload().to_string().replacen("{", "{\"format\":1,", 1);
    assert!(verify_with_key(&signed(&duplicate), &facts(), &key().verifying_key()).is_err());
}

#[test]
fn reads_are_bounded_and_a_missing_sidecar_is_optional() {
    let mut reader = std::io::Cursor::new(vec![b'x'; MAX_SIZE + 100]);
    assert!(read_bounded(&mut reader).is_err());
    assert_eq!(reader.position(), (MAX_SIZE + 1) as u64);
    assert!(read_bounded(std::io::Cursor::new(vec![0; MAX_SIZE])).is_ok());
    assert!(verify_with_key(&vec![0; MAX_SIZE + 1], &facts(), &key().verifying_key()).is_err());
    let directory = crate::test_support::test_directory("netplay-certificate-sidecar").unwrap();
    let path = directory.path().join("zeff-boy.netplay.json");
    assert!(read_sidecar(&path).unwrap().is_none());
    let bytes = signed(&payload().to_string());
    std::fs::write(&path, &bytes).unwrap();
    assert!(
        verify_with_key(
            &read_sidecar(&path).unwrap().unwrap(),
            &facts(),
            &key().verifying_key()
        )
        .is_ok()
    );
    std::fs::write(&path, b"{}").unwrap();
    assert!(
        verify_with_key(
            &read_sidecar(&path).unwrap().unwrap(),
            &facts(),
            &key().verifying_key()
        )
        .is_err()
    );
}
