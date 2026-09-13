use super::*;

fn parse(extra: &[&str]) -> anyhow::Result<AudioDiscoveryRequest> {
    let mut args = vec!["--audio-discover", "scan.json", "game.gbc"];
    args.extend_from_slice(extra);
    Ok(parse_audio_discovery_args(args.into_iter().map(OsString::from))?.unwrap())
}

#[test]
fn driver_evidence_requires_a_unique_path_and_discovery_request() {
    let request = parse(&["--audio-driver-evidence", "drivers.json"]).unwrap();
    assert_eq!(request.driver_evidence, Some(PathBuf::from("drivers.json")));
    assert!(request.export.is_none());
    assert!(request.relations.is_none());
    assert!(parse(&["--audio-driver-evidence"]).is_err());
    assert!(parse(&["--audio-driver-evidence", "--audio-scan-work", "100"]).is_err());
    assert!(
        parse(&[
            "--audio-driver-evidence",
            "one.json",
            "--audio-driver-evidence",
            "two.json",
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args(["--audio-driver-evidence", "drivers.json"].map(OsString::from))
            .is_err()
    );
}

#[test]
fn evidence_preserves_selected_source_and_existing_scan_bytes() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-driver-evidence-zip")?;
    let source = zeff_audio_discovery::gb_native::cgb_fixture_rom();
    let archive = directory.path().join("source.zip");
    crate::test_support::write_zip(
        &archive,
        &[("selected.gbc", &source), ("ignored.gbc", b"other bytes")],
    )?;
    let original = std::fs::read(&archive)?;
    let mut request = parse(&["--audio-driver-evidence", "drivers.json"])?;
    request.input_path = archive.clone();
    request.archive_member = Some("selected.gbc".into());
    request.output_path = directory.path().join("with-evidence.json");
    let evidence_path = directory.path().join("drivers.json");
    request.driver_evidence = Some(evidence_path.clone());
    run_request(&request)?;
    let first = std::fs::read(&request.output_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&first)?;
    let evidence: serde_json::Value = serde_json::from_slice(&std::fs::read(&evidence_path)?)?;
    assert_eq!(evidence["source"], manifest["source"]);
    assert_eq!(evidence["transforms"], manifest["transforms"]);
    assert_eq!(
        evidence["driver_evidence"]["media"],
        manifest["scan"]["media"]
    );
    assert_eq!(
        evidence["driver_evidence"]["findings"],
        serde_json::json!([])
    );
    assert_eq!(
        manifest["scan"]["gb_native_songs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    request.output_path = directory.path().join("without-evidence.json");
    request.driver_evidence = None;
    run_request(&request)?;
    assert_eq!(std::fs::read(&request.output_path)?, first);
    assert_eq!(std::fs::read(archive)?, original);
    Ok(())
}

#[test]
fn evidence_rejects_output_aliases_and_existing_files_before_writing() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-driver-evidence-paths")?;
    let source = zeff_audio_discovery::gb_native::cgb_fixture_rom();
    let input = directory.path().join("source.gbc");
    std::fs::write(&input, &source)?;
    let scan = directory.path().join("scan.json");
    let mut request = parse(&[])?;
    request.input_path = input.clone();
    request.output_path = scan.clone();
    for path in [&input, &scan] {
        request.driver_evidence = Some(path.clone());
        assert!(run_request(&request).is_err());
        assert!(!scan.exists());
    }
    let existing = directory.path().join("existing.json");
    std::fs::write(&existing, b"keep")?;
    request.driver_evidence = Some(existing.clone());
    assert!(run_request(&request).is_err());
    assert_eq!(std::fs::read(existing)?, b"keep");
    assert!(!scan.exists());
    assert_eq!(std::fs::read(input)?, source);
    Ok(())
}

#[test]
fn evidence_budget_stop_is_explicit_and_does_not_change_song_scan() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-driver-evidence-budget")?;
    let input = directory.path().join("source.gbc");
    std::fs::write(&input, zeff_audio_discovery::gb_native::cgb_fixture_rom())?;
    let mut request = parse(&[])?;
    request.input_path = input;
    request.output_path = directory.path().join("scan.json");
    let output = directory.path().join("evidence.json");
    request.driver_evidence = Some(output.clone());
    request.max_work = Some(0);
    run_request(&request)?;
    let evidence: serde_json::Value = serde_json::from_slice(&std::fs::read(output)?)?;
    assert_eq!(
        evidence["driver_evidence"]["status"],
        serde_json::json!({"kind":"incomplete", "reason":"work_limit"})
    );
    assert_eq!(evidence["driver_evidence"]["work_used"], 0);
    assert_eq!(
        evidence["driver_evidence"]["findings"],
        serde_json::json!([])
    );
    Ok(())
}
