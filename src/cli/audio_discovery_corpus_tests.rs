use super::*;

#[test]
fn corpus_counts_structural_inventory_separately_from_playable_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("structural.nes"),
        zeff_audio_discovery::drivers::nes_tose_structure::synthetic_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([
            {"id": "structural", "path": "structural.nes"}
        ]),
    )?;
    let output = directory.path().join("corpus.json");
    run(&output, &inputs, None)?;
    let report: CorpusReport = read_json(&output)?;
    let value = serde_json::to_value(report)?;
    let counts = &value["summary"]["scanned"];
    assert_eq!(counts["inputs_with_catalog_entries"], 0);
    assert_eq!(counts["inputs_with_render_support"], 0);
    assert_eq!(counts["inputs_with_structural_inventory"], 1);
    assert_eq!(counts["structural_inventories"], 1);
    assert!(counts["structural_selectors"].as_u64().unwrap() > 0);
    assert_eq!(counts["inputs_with_code_evidence"], 0);
    Ok(())
}

#[test]
fn corpus_counts_decoded_sound_writes_without_catalog_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("writes.nes"),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id":"writes","path":"writes.nes"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    let counts = &report["summary"]["scanned"];
    assert_eq!(counts["inputs_with_code_evidence"], 1);
    assert_eq!(counts["code_candidates"], 1);
    assert!(counts["sound_write_witnesses"].as_u64().unwrap() >= 2);
    assert_eq!(counts["inputs_with_sound_callers"], 1);
    assert!(counts["sound_call_witnesses"].as_u64().unwrap() > 0);
    assert_eq!(counts["structural_inventories"], 0);
    assert_eq!(counts["catalog_entries"], 0);
    assert_eq!(counts["render_supported_entries"], 0);
    Ok(())
}

#[test]
fn corpus_counts_bounded_selector_consumers_without_cataloguing_them() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("selector.nes"),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_selector_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id":"selector","path":"selector.nes"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    let counts = &report["summary"]["scanned"];
    assert_eq!(counts["inputs_with_code_evidence"], 1);
    assert_eq!(counts["selector_consumers"], 1);
    assert_eq!(counts["unparsed_selector_pointers"], 14);
    assert_eq!(counts["held_selector_pointers"], 3);
    assert_eq!(counts["catalog_entries"], 0);
    assert_eq!(counts["render_supported_entries"], 0);
    Ok(())
}

fn write_inputs(directory: &Path, inputs: serde_json::Value) -> anyhow::Result<PathBuf> {
    let path = directory.join("inputs.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schema": "zeff-audio-corpus-inputs/1",
            "inputs": inputs,
        }))?,
    )?;
    Ok(path)
}

#[test]
fn corpus_counts_record_fields_without_promoting_unparsed_streams() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("records.nes"),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_record_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id":"records","path":"records.nes"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    let counts = &report["summary"]["scanned"];
    assert_eq!(counts["selector_consumers"], 1);
    assert_eq!(counts["record_prefixes"], 16);
    assert_eq!(counts["unparsed_stream_pointers"], 24);
    assert_eq!(counts["held_stream_pointers"], 0);
    assert_eq!(counts["structural_inventories"], 0);
    assert_eq!(counts["catalog_entries"], 0);
    assert_eq!(counts["render_supported_entries"], 0);
    Ok(())
}

#[test]
fn corpus_counts_command_dispatches_without_catalog_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("dispatch.nes"),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_dispatch_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id": "dispatch", "path": "dispatch.nes"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    let counts = &report["summary"]["scanned"];
    assert_eq!(counts["command_dispatches"], 1);
    assert_eq!(counts["guarded_command_fetches"], 2);
    assert_eq!(counts["structural_inventories"], 0);
    assert_eq!(counts["catalog_entries"], 0);
    assert_eq!(counts["render_supported_entries"], 0);
    Ok(())
}

#[test]
fn corpus_counts_conditional_edges_without_catalog_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("bindings.nes"),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_head_edge_rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id": "bindings", "path": "bindings.nes"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    let counts = &report["summary"]["scanned"];
    assert_eq!(counts["stream_fetch_bindings"], 12);
    assert_eq!(counts["conditional_head_command_edges"], 12);
    assert_eq!(counts["structural_inventories"], 0);
    assert_eq!(counts["catalog_entries"], 0);
    assert_eq!(counts["render_supported_entries"], 0);
    Ok(())
}

fn fixture(directory: &Path) -> anyhow::Result<PathBuf> {
    let rom = directory.join("fixture.gba");
    let mut bytes = zeff_audio_discovery::gbass::fixture_rom_banked();
    bytes[0xB2] = 0x96;
    std::fs::write(&rom, bytes)?;
    Ok(rom)
}

#[test]
fn corpus_preserves_errors_capabilities_and_deterministic_comparison() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    fixture(directory.path())?;
    std::fs::write(directory.path().join("empty.gb"), [])?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([
            {"id": "missing", "path": "missing.gb"},
            {"id": "ready", "path": "fixture.gba"},
            {"id": "empty", "path": "empty.gb"},
        ]),
    )?;
    let first = directory.path().join("first.json");
    run(&first, &inputs, None)?;
    let report: CorpusReport = read_json(&first)?;
    assert_eq!(
        report
            .rows
            .iter()
            .map(|r| r.input.id.as_str())
            .collect::<Vec<_>>(),
        ["empty", "missing", "ready"]
    );
    let value = serde_json::to_value(&report)?;
    assert_eq!(value["summary"]["total_inputs"], 3);
    assert_eq!(value["summary"]["input_errors"], 2);
    assert_eq!(value["summary"]["scanned"]["inputs_with_render_support"], 1);
    assert_eq!(value["summary"]["scanned"]["render_supported_entries"], 4);
    assert!(
        matches!(&report.rows[0].result, InputResult::InputError { reason, message } if reason == "empty_source" && !message.is_empty())
    );
    let second = directory.path().join("second.json");
    run(&second, &inputs, Some(&first))?;
    let repeated: CorpusReport = read_json(&second)?;
    assert_eq!(
        serde_json::to_value(&report.rows)?,
        serde_json::to_value(&repeated.rows)?
    );
    let comparison = serde_json::to_value(repeated.comparison)?;
    assert_eq!(comparison["matched_inputs"], 1);
    assert_eq!(comparison["changed_observations"], serde_json::json!([]));
    assert_eq!(comparison["changed_scan_reports"], serde_json::json!([]));
    assert_eq!(
        comparison["incomparable_inputs"].as_object().unwrap().len(),
        2
    );
    let before = std::fs::read(&first)?;
    assert!(run(&first, &inputs, None).is_err());
    assert_eq!(std::fs::read(first)?, before);
    Ok(())
}

#[test]
fn corpus_rejects_invalid_manifests_and_output_aliases_before_scanning() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let rom = fixture(directory.path())?;
    let original = std::fs::read(&rom)?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([
            {"id": "same", "path": "fixture.gba"},
            {"id": "same", "path": "fixture.gba"},
        ]),
    )?;
    let output = directory.path().join("out.json");
    assert!(run(&output, &inputs, None).is_err());
    assert!(!output.exists());
    write_inputs(
        directory.path(),
        serde_json::json!([{"id":"rom", "path":"fixture.gba"}]),
    )?;
    assert!(run(&rom, &inputs, None).is_err());
    assert_eq!(original, std::fs::read(&rom)?);
    let mut value: serde_json::Value = read_json(&inputs)?;
    value["limits"] = serde_json::json!({"max_work": 100000001, "max_candidates": 4096});
    std::fs::write(&inputs, serde_json::to_vec(&value)?)?;
    assert!(run(&output, &inputs, None).is_err());
    assert!(!output.exists());
    value["limits"]["max_work"] = 0.into();
    std::fs::write(&inputs, serde_json::to_vec(&value)?)?;
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    assert_eq!(report["summary"]["input_errors"], 0);
    assert_eq!(
        report["summary"]["scanned"]["blockers"]["scan_incomplete"],
        1
    );
    Ok(())
}

#[test]
fn corpus_cli_rejects_mixed_modes_and_missing_arguments() {
    for args in [
        vec!["--audio-corpus"],
        vec!["--audio-corpus", "out", "inputs", "--audio-discover", "x"],
        vec!["--audio-corpus", "out", "--audio-corpus-baseline"],
        vec!["--audio-corpus-baseline", "old"],
        vec![
            "--audio-discover",
            "out",
            "rom",
            "--audio-corpus",
            "out",
            "inputs",
        ],
    ] {
        assert!(
            run_if_requested(&args.into_iter().map(OsString::from).collect::<Vec<_>>()).is_err()
        );
    }
    assert!(!run_if_requested(&["--headless".into()]).unwrap());
}

#[test]
fn comparison_separates_content_changes_removals_and_capability_regressions() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let path = fixture(directory.path())?;
    let input = CorpusInput {
        id: "stable".to_owned(),
        path,
        archive_member: None,
    };
    let result = scan_input(
        &input,
        &directory.path().join("unused.json"),
        Limits::default(),
    );
    let row = Row { input, result };
    assert!(
        matches!(&row.result, InputResult::Scanned { observation, .. } if observation.render_supported_entries == 4)
    );
    let old = CorpusReport {
        schema: SCHEMA.to_owned(),
        application_version: "test".to_owned(),
        limits: Limits::default(),
        summary: Summary::from_rows(std::slice::from_ref(&row)),
        comparison: None,
        rows: vec![row.clone()],
    };
    let mut changed = row.clone();
    if let InputResult::Scanned {
        observation,
        scan_sha256,
        ..
    } = &mut changed.result
    {
        observation.render_supported_entries = 0;
        observation.catalog_entries = 0;
        *scan_sha256 = "changed".to_owned();
    }
    let comparison =
        serde_json::to_value(Comparison::between(&old, &[changed.clone()], old.limits))?;
    assert_eq!(
        comparison["lost_render_support"],
        serde_json::json!(["stable"])
    );
    assert_eq!(
        comparison["fewer_catalog_entries"],
        serde_json::json!(["stable"])
    );
    assert_eq!(
        comparison["changed_scan_reports"],
        serde_json::json!(["stable"])
    );
    if let InputResult::Scanned { observation, .. } = &mut changed.result {
        observation.media_sha256 = Some("different source".to_owned());
    }
    let comparison =
        serde_json::to_value(Comparison::between(&old, &[changed.clone()], old.limits))?;
    assert_eq!(comparison["matched_inputs"], 0);
    assert_eq!(comparison["lost_render_support"], serde_json::json!([]));
    assert_eq!(
        comparison["incomparable_inputs"]["stable"],
        "media_identity_changed_or_missing"
    );
    changed.input.id = "new".to_owned();
    let comparison = serde_json::to_value(Comparison::between(&old, &[changed], old.limits))?;
    assert_eq!(comparison["added_inputs"], serde_json::json!(["new"]));
    assert_eq!(comparison["removed_inputs"], serde_json::json!(["stable"]));
    let comparison = serde_json::to_value(Comparison::between(
        &old,
        std::slice::from_ref(&row),
        Limits {
            max_work: 0,
            ..old.limits
        },
    ))?;
    assert_eq!(
        comparison["incomparable_inputs"]["stable"],
        "scan_limits_changed"
    );
    let mut changed = row;
    if let InputResult::Scanned {
        analysis_profile, ..
    } = &mut changed.result
    {
        *analysis_profile = "different".to_owned();
    }
    let comparison = serde_json::to_value(Comparison::between(
        &old,
        std::slice::from_ref(&changed),
        old.limits,
    ))?;
    assert_eq!(
        comparison["incomparable_inputs"]["stable"],
        "analysis_profile_changed"
    );
    changed.result = InputResult::InputError {
        reason: "source_not_found".to_owned(),
        message: "missing".to_owned(),
    };
    let comparison =
        serde_json::to_value(Comparison::between(&old, &[changed.clone()], old.limits))?;
    assert_eq!(
        comparison["new_input_errors"],
        serde_json::json!(["stable"])
    );
    assert_eq!(comparison["lost_render_support"], serde_json::json!([]));
    let recovered = old.rows.clone();
    let failed = CorpusReport {
        rows: vec![changed],
        ..old
    };
    let comparison = serde_json::to_value(Comparison::between(&failed, &recovered, failed.limits))?;
    assert_eq!(
        comparison["recovered_input_errors"],
        serde_json::json!(["stable"])
    );
    Ok(())
}

#[test]
fn corpus_loader_failures_have_stable_categories() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("empty.gb"), [])?;
    std::fs::write(directory.path().join("short.gba"), [0; 4])?;
    std::fs::write(directory.path().join("broken.zip"), b"not a ZIP")?;
    for (path, member, expected) in [
        ("missing.gb", None, "source_not_found"),
        ("empty.gb", None, "empty_source"),
        ("short.gba", None, "source_size_out_of_range"),
        ("unsupported.txt", None, "unsupported_input_format"),
        ("broken.zip", None, "archive_member_required"),
        ("broken.zip", Some("game.gb"), "archive_load_failed"),
        (
            "broken.zip",
            Some("../game.gb"),
            "invalid_archive_selection",
        ),
        ("short.gba", Some("game.gb"), "invalid_archive_selection"),
    ] {
        let input = CorpusInput {
            id: "test".to_owned(),
            path: directory.path().join(path),
            archive_member: member.map(str::to_owned),
        };
        let result = scan_input(
            &input,
            &directory.path().join("unused.json"),
            Limits::default(),
        );
        assert!(
            matches!(result, InputResult::InputError { reason, .. } if reason == expected),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn disc_archives_do_not_inherit_cartridge_size_diagnostics() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("large.zip");
    std::fs::File::create(&path)?.set_len(super::super::MAX_ARCHIVE_BYTES + 1)?;
    let mut input = CorpusInput {
        id: "archive".to_owned(),
        path,
        archive_member: Some("disc.cue".to_owned()),
    };
    let error = anyhow::anyhow!("archive load failed");
    assert_eq!(
        input_failure::failure_reason(&input, &error),
        "archive_load_failed"
    );
    input.archive_member = Some("game.gb".to_owned());
    assert_eq!(
        input_failure::failure_reason(&input, &error),
        "source_size_out_of_range"
    );
    Ok(())
}

#[test]
fn corpus_selected_archive_member_preserves_effective_identity() -> anyhow::Result<()> {
    use std::io::Write;
    let directory = tempfile::tempdir()?;
    let rom = fixture(directory.path())?;
    let bytes = std::fs::read(&rom)?;
    let archive = directory.path().join("source.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive)?);
    zip.start_file(
        "nested/fixture.gba",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(&bytes)?;
    zip.finish()?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([
            {"id":"zip", "path":"source.zip", "archive_member":"nested/fixture.gba"},
            {"id":"direct", "path":"fixture.gba"},
            {"id":"missing-selection", "path":"source.zip"},
        ]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: CorpusReport = read_json(&output)?;
    let observations = report
        .rows
        .iter()
        .filter_map(|row| match &row.result {
            InputResult::Scanned { observation, .. } => Some(observation),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0], observations[1]);
    assert_eq!(observations[0].media_sha256, Some(sha256_hex(&bytes)));
    Ok(())
}

#[test]
fn corpus_counts_huge_pending_verification_without_render_support() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("huge.gb"),
        zeff_audio_discovery::huge::fixture::rom(),
    )?;
    let inputs = write_inputs(
        directory.path(),
        serde_json::json!([{"id":"huge","path":"huge.gb"}]),
    )?;
    let output = directory.path().join("report.json");
    run(&output, &inputs, None)?;
    let report: serde_json::Value = read_json(&output)?;
    for counts in [
        &report["summary"]["scanned"],
        &report["summary"]["detectors"]["gb-huge-driver"],
    ] {
        assert_eq!(counts["catalog_entries"], 1);
        assert_eq!(counts["pending_runtime_entries"], 1);
        assert_eq!(counts["render_supported_entries"], 0);
    }
    assert_eq!(
        report["summary"]["scanned"]["blockers"]["catalog_runtime_validation_required"],
        1
    );
    Ok(())
}
