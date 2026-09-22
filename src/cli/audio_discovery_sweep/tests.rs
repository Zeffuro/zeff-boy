use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn settings(selectors: &Value) -> report::ExpectedSettings<'_> {
    report::ExpectedSettings {
        sample_rate: 48000,
        selectors,
        requested_steps: 720,
    }
}

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zeff-audio-sweep-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn sweep_parser_requires_a_complete_leading_request() -> Result<()> {
    assert!(input::parse(&args(&["game.gb"]))?.is_none());
    assert!(input::parse(&args(&["game.gb", "--audio-capture-sweep", "out"])).is_err());
    assert!(
        input::parse(&args(&[
            "--audio-capture-sweep",
            "out",
            "game.gb",
            "--mode",
            "sgb"
        ]))
        .is_err()
    );
    let request = input::parse(&args(&[
        "--audio-capture-sweep",
        "out",
        "game.gb",
        "--mode",
        "cgb",
    ]))?
    .unwrap();
    assert_eq!(request.gb_mode.as_deref(), Some("cgb"));
    assert_eq!(request.output, PathBuf::from("out"));
    Ok(())
}

#[test]
fn selectors_are_typed_and_forwarded_only_to_their_systems() -> Result<()> {
    let request = input::parse(&args(&[
        "--audio-capture-sweep",
        "out",
        "game.sms",
        "--sega8-video-standard",
        "pal",
        "--sega8-console-region",
        "japan",
    ]))?
    .unwrap();
    let mut forwarded = Vec::new();
    forward_options(
        &mut forwarded,
        &request,
        zeff_emu_common::system::System::Sms,
    );
    assert_eq!(
        forwarded,
        args(&[
            "--sega8-video-standard",
            "pal",
            "--sega8-console-region",
            "japanese",
        ])
    );
    let mut discarded = Vec::new();
    forward_options(
        &mut discarded,
        &request,
        zeff_emu_common::system::System::Gb,
    );
    assert_eq!(discarded, args(&["--mode", "auto"]));
    assert!(
        input::parse(&args(&[
            "--audio-capture-sweep",
            "out",
            "game.gb",
            "--sega8-video-standard",
            "invalid",
        ]))
        .is_err()
    );
    Ok(())
}

#[test]
fn fixed_plans_have_exact_inclusive_input_schedules() {
    let plans = plans::defaults();
    assert_eq!(
        plans
            .iter()
            .map(|plan| plan.name.as_str())
            .collect::<Vec<_>>(),
        ["baseline", "start", "start_then_a"]
    );
    assert_eq!(plans[0].events, json!([]));
    assert_eq!(
        plans[1].events,
        json!([{"start_frame":180,"end_frame":181,"buttons":8,"dpad":0,"coleco_keypad":null,"reset":false}])
    );
    assert_eq!(
        plans[2].events,
        json!([
            {"start_frame":180,"end_frame":181,"buttons":8,"dpad":0,"coleco_keypad":null,"reset":false},
            {"start_frame":300,"end_frame":301,"buttons":1,"dpad":0,"coleco_keypad":null,"reset":false}
        ])
    );
}

#[test]
fn custom_plan_document_replaces_the_default_schedule_and_is_retained() -> Result<()> {
    let path = temporary_path("plans.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema": "zeff-audio-capture-plans/1",
            "plans": [
                {"name": "quiet", "steps": 1},
                {"name": "go", "steps": 721, "press": "start@720-720"}
            ]
        }))?,
    )?;
    let request = input::parse(&[
        OsString::from("--audio-capture-sweep"),
        OsString::from("out"),
        OsString::from("game.gb"),
        OsString::from("--audio-capture-plans"),
        path.as_os_str().to_owned(),
    ])?
    .unwrap();
    assert_eq!(request.plans.len(), 2);
    assert_eq!(request.plans[1].name, "go");
    assert_eq!(request.plans[1].steps, 721);
    assert_eq!(
        request.plans[1].events,
        json!([{"start_frame":720,"end_frame":720,"buttons":8,"dpad":0,"coleco_keypad":null,"reset":false}])
    );
    assert!(request.plan_document_sha256.is_some());
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn custom_plan_option_is_unique_and_only_accepted_with_a_sweep() -> Result<()> {
    assert!(input::parse(&args(&["game.gb", "--audio-capture-plans", "plans.json",]))?.is_none());
    let path = temporary_path("duplicate-plans.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema":"zeff-audio-capture-plans/1", "plans":[{"name":"one","steps":1}]
        }))?,
    )?;
    let result = input::parse(&[
        OsString::from("--audio-capture-sweep"),
        OsString::from("out"),
        OsString::from("game.gb"),
        OsString::from("--audio-capture-plans"),
        path.as_os_str().to_owned(),
        OsString::from("--audio-capture-plans"),
        path.as_os_str().to_owned(),
    ]);
    std::fs::remove_file(path)?;
    assert!(result.is_err());
    Ok(())
}

#[test]
fn zip_ambiguity_counts_every_headless_candidate() -> Result<()> {
    let path = temporary_path("ambiguous.zip");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path)?);
    archive.start_file("one.gb", zip::write::SimpleFileOptions::default())?;
    archive.write_all(&[0; 512])?;
    archive.start_file("other.gba", zip::write::SimpleFileOptions::default())?;
    archive.write_all(&[0; 512])?;
    archive.finish()?;
    let candidates = input::zip_candidates(&path)?;
    assert_eq!(candidates.candidates, 2);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn candidate_evidence_binds_the_extracted_zip_member_not_the_container() -> Result<()> {
    let path = temporary_path("candidate-evidence.zip");
    let member = vec![0; 32768];
    let member_sha256 = zeff_firmware::sha256_hex(&member);
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path)?);
    archive.start_file("fixture.gb", zip::write::SimpleFileOptions::default())?;
    archive.write_all(&member)?;
    archive.finish()?;
    let archive_sha256 = zeff_firmware::sha256_hex(&std::fs::read(&path)?);
    let request = input::parse(&[
        OsString::from("--audio-capture-sweep"),
        OsString::from("unused-output"),
        path.as_os_str().to_owned(),
    ])?
    .unwrap();
    let loaded = input::load(&request, &AtomicBool::new(false))?;
    assert_eq!(loaded.requested_sha256, archive_sha256);
    assert_ne!(loaded.requested_sha256, member_sha256);
    assert_eq!(loaded.source_identity["sha256"], member_sha256);
    assert_eq!(
        loaded.candidate_evidence.pointer("/report/media/sha256"),
        Some(&loaded.source_identity["sha256"])
    );
    assert_eq!(
        loaded
            .candidate_evidence
            .pointer("/source_identity/container/sha256"),
        Some(&json!(archive_sha256))
    );
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn source_change_and_baseboard_gates_are_observable() -> Result<()> {
    let path = temporary_path("source.gb");
    std::fs::write(&path, [1, 2, 3])?;
    let hash = zeff_firmware::sha256_hex(&std::fs::read(&path)?);
    assert!(input::source_is_unchanged(&path, &hash));
    std::fs::write(&path, [3, 2, 1])?;
    assert!(!input::source_is_unchanged(&path, &hash));
    std::fs::remove_file(path)?;
    assert!(!input::nes_trace_admitted(b"NES\x1a"));
    let mut rom = vec![0; 16 + 16_384 + 8192];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 1;
    rom[5] = 1;
    assert!(input::nes_trace_admitted(&rom));
    rom[7] = 8;
    rom[8] = 1;
    assert!(!input::nes_trace_admitted(&rom));
    Ok(())
}

#[test]
fn context_requires_identity_schedule_steps_and_matching_rate() -> Result<()> {
    let source = json!({
        "sha256": "a".repeat(64),
        "len": 32768,
        "container": null,
        "selected_member": null,
    });
    let events = json!([{"start_frame":180,"end_frame":181,"buttons":8,"dpad":0,"coleco_keypad":null,"reset":false}]);
    let mut context = json!({
        "system": "gb",
        "source": {
            "requested_file": {"sha256": "a".repeat(64), "byte_len": 32768},
            "loaded_media": {"sha256": "a".repeat(64), "byte_len": 32768},
        },
        "input": {"player_1": events, "player_2": []},
        "frames_run": 720,
        "requested_frames": 720,
        "settings": {"sample_rate_hz": 48000, "mode_preference": "Auto"},
        "firmware": null,
        "persistent_save_files": "not_loaded_or_written",
        "sample_generation": true,
    });
    let result = report::validate_context_value(
        &context,
        &source,
        &"a".repeat(64),
        "gb",
        &events,
        settings(&json!({"gb_mode": "auto"})),
    );
    assert!(result.is_ok(), "{result:?}");
    let valid = context.clone();
    context["settings"]["sample_rate_hz"] = json!(48_000.0);
    assert!(
        report::validate_context_value(
            &context,
            &source,
            &"a".repeat(64),
            "gb",
            &events,
            settings(&json!({"gb_mode": "auto"}))
        )
        .is_ok()
    );
    for (pointer, value) in [
        ("/settings/sample_rate_hz", json!(48_000.5)),
        ("/settings/sample_rate_hz", json!("48000")),
        ("/settings/sample_rate_hz", Value::Null),
        ("/source/requested_file/sha256", json!("b".repeat(64))),
        ("/requested_frames", json!(719)),
        ("/frames_run", json!(719)),
        ("/input/player_2", events.clone()),
        ("/settings/mode_preference", json!("ForceDmg")),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            report::validate_context_value(
                &changed,
                &source,
                &"a".repeat(64),
                "gb",
                &events,
                settings(&json!({"gb_mode": "auto"}))
            )
            .is_err(),
            "{pointer}"
        );
    }
    let mut coleco = valid.clone();
    coleco["system"] = json!("coleco");
    coleco["firmware"] =
        json!({"firmware_id": "coleco.vision.bios", "byte_len": 8192, "sha256": "a".repeat(64)});
    assert!(
        report::validate_context_value(
            &coleco,
            &source,
            &"a".repeat(64),
            "coleco",
            &events,
            settings(&json!({}))
        )
        .is_ok()
    );
    for (key, value) in [
        ("byte_len", json!(1)),
        ("sha256", json!("A".repeat(64))),
        ("sha256", json!("g".repeat(64))),
    ] {
        let mut changed = coleco.clone();
        changed["firmware"][key] = value;
        assert!(
            report::validate_context_value(
                &changed,
                &source,
                &"a".repeat(64),
                "coleco",
                &events,
                settings(&json!({}))
            )
            .is_err()
        );
    }
    context["settings"]["sample_rate_hz"] = json!(44_100);
    assert!(
        report::validate_context_value(
            &context,
            &source,
            &"a".repeat(64),
            "gb",
            &events,
            settings(&json!({"gb_mode": "auto"})),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn duplicate_groups_and_silence_remain_execution_evidence() {
    let rows = vec![
        json!({"plan": "baseline", "status": "success", "validation": {"playback": {"evidence": {"pcm": {"sample_rate": 48000, "frames": 0, "pcm_sha256": "same"}}}}}),
        json!({"plan": "start", "status": "success", "validation": {"playback": {"evidence": {"pcm": {"sample_rate": 48000, "frames": 0, "pcm_sha256": "same"}}}}}),
        json!({"plan": "start_then_a", "status": "success", "validation": {"playback": {"evidence": {"pcm": {"sample_rate": 48000, "frames": 9, "pcm_sha256": "other"}}}}}),
    ];
    assert_eq!(
        report::duplicates(&rows),
        vec![
            json!({"sample_rate": 48000, "frames": 0, "pcm_sha256": "same", "plans": ["baseline", "start"]})
        ]
    );
}

#[test]
fn output_reservation_never_reuses_a_directory() -> Result<()> {
    let path = temporary_path("output");
    reserve_output(&path)?;
    assert!(reserve_output(&path).is_err());
    std::fs::remove_dir(path)?;
    Ok(())
}

#[test]
fn validation_failures_are_not_mislabeled_as_reference_mismatches() -> Result<()> {
    for (termination, status) in [
        (process::Termination::TimedOut, "validation_timeout"),
        (process::Termination::OutputLimit, "output_limit"),
        (process::Termination::Cancelled, "cancelled"),
    ] {
        assert_eq!(validation_status(termination, false), status);
        assert_eq!(validation_status(termination, true), status);
    }
    let path = temporary_path("validation.json");
    for status in ["matched", "not_compared", "reference_error", "mismatch"] {
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "outcome": {"native_reference": {"status": status}}
            }))?,
        )?;
        assert_eq!(
            validation_has_reference_mismatch(&path),
            status == "mismatch"
        );
    }
    std::fs::write(&path, b"{}")?;
    assert!(!validation_has_reference_mismatch(&path));
    std::fs::File::create(&path)?.set_len(4 * 1024 * 1024 + 1)?;
    assert!(report::read_validation(&path).is_err());
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn configuration_refusals_remain_distinct_from_input_errors() -> Result<()> {
    let path = temporary_path("unsupported.gb");
    let request = input::parse(&[
        OsString::from("--audio-capture-sweep"),
        OsString::from("unused-output"),
        path.as_os_str().to_owned(),
    ])?
    .unwrap();
    let mut rom = vec![0; 32768];
    rom[0x147] = 0xfe;
    std::fs::write(&path, rom)?;
    let error = input::load(&request, &AtomicBool::new(false))
        .err()
        .unwrap();
    assert!(error.is::<input::UnsupportedConfiguration>());
    std::fs::remove_file(path)?;
    let error = input::load(&request, &AtomicBool::new(false))
        .err()
        .unwrap();
    assert!(!error.is::<input::UnsupportedConfiguration>());
    Ok(())
}

#[test]
fn validation_requires_the_captured_hashes_and_complete_success_contract() -> Result<()> {
    let path = temporary_path("bound-validation.json");
    let capture = json!({"archive_sha256": "archive", "trace_sha256": "trace"});
    let valid = json!({
        "schema": "zeff-audio-playback-validation/1", "mode": "native_capture",
        "render_options": {"sample_rate": 48000},
        "outcome": {
            "status": "integrity_verified", "archive_sha256": "archive", "trace_sha256": "trace",
            "playback": {"status": "rendered", "evidence": {
                "fresh_render_matches": true, "reset_render_matches": true, "validation_duration_capped": false
            }},
            "native_reference": {"status": "matched", "evidence": {"projected_pcm_matches": true}}
        }
    });
    std::fs::write(&path, serde_json::to_vec(&valid)?)?;
    assert!(report::validation_summary(&path, &capture, 48000).is_ok());
    for (pointer, value) in [
        ("/outcome/archive_sha256", json!("changed")),
        ("/outcome/trace_sha256", json!("changed")),
        ("/render_options/sample_rate", json!(44100)),
        (
            "/outcome/playback/evidence/fresh_render_matches",
            json!(false),
        ),
        (
            "/outcome/playback/evidence/validation_duration_capped",
            json!(true),
        ),
        ("/outcome/native_reference/status", json!("mismatch")),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        std::fs::write(&path, serde_json::to_vec(&changed)?)?;
        assert!(
            report::validation_summary(&path, &capture, 48000).is_err(),
            "{pointer}"
        );
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn changed_source_evidence_is_excluded_from_accepted_counts() {
    let rows = [json!({
        "plan": "baseline", "status": "source_changed", "capture": {},
        "validation": {"playback": {"status": "rendered", "evidence": {"silent": false}}}
    })];
    assert_eq!(
        report::summary(&rows),
        json!({
            "status_histogram": {"source_changed": 1}, "capture_validated": 0,
            "playback_validated": 0, "active": 0, "silent": 0
        })
    );
    assert!(report::duplicates(&rows).is_empty());
}
