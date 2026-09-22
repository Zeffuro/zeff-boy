use serde_json::json;

use super::*;

fn report(system: &str, sha256: &str, len: u64) -> Value {
    json!({
        "schema": "zeff-audio-driver-evidence/1",
        "media": {"system": system, "sha256": sha256, "byte_len": len},
        "limits": {"max_work": 100, "max_candidates": 10},
        "status": {"kind": "complete"},
        "findings": [], "driver_candidates": []
    })
}

fn evidence(system: &str) -> Value {
    let source = json!({
        "kind": "direct_cartridge_file", "sha256": "a".repeat(64), "len": 32768,
        "container": null, "selected_member": null
    });
    let report = report(system, source["sha256"].as_str().unwrap(), 32768);
    wrap(
        report.clone(),
        source.clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1", "source": source,
            "transforms": [], "media": report["media"]
        }),
        report["limits"].clone(),
    )
    .unwrap()
}

fn bound_row(evidence: &Value, system: &str) -> Value {
    let source = &evidence["source_identity"];
    let requested = if source["container"].is_null() {
        json!({"sha256": source["sha256"], "byte_len": source["len"]})
    } else {
        json!({
            "sha256": source["container"]["sha256"],
            "byte_len": source["container"]["len"]
        })
    };
    let selected_member = if source["selected_member"].is_null() {
        Value::Null
    } else {
        json!({
            "name": source["selected_member"]["name"],
            "sha256": source["selected_member"]["sha256"],
            "byte_len": source["selected_member"]["len"]
        })
    };
    let settings = if system == "gb" {
        json!({"sample_rate_hz": 48000, "mode_preference": "Auto"})
    } else {
        json!({"sample_rate_hz": 48000})
    };
    let events = json!([]);
    let context = json!({
        "system": system,
        "source": {
            "requested_file": requested,
            "loaded_media": {"sha256": source["sha256"], "byte_len": source["len"]},
            "selected_member": selected_member
        },
        "input": {"player_1": [], "player_2": []}, "frames_run": 720,
        "requested_frames": 720, "settings": settings, "firmware": null,
        "persistent_save_files": "not_loaded_or_written", "sample_generation": true
    });
    let pcm = json!({
        "sample_rate": 48000, "frames": 12, "pcm_sha256": "c".repeat(64),
        "active_frames": 0, "activity_intervals": [], "activity_intervals_truncated": false
    });
    json!({
        "plan": "baseline", "status": "success", "requested_steps": 720,
        "applied_input": {"player_1": events, "press": null},
        "candidate_evidence": reference(evidence, true),
        "capture": {
            "archive_sha256": "d".repeat(64), "trace_sha256": "e".repeat(64),
            "context": context
        },
        "validation": {
            "status": "integrity_verified", "archive_sha256": "d".repeat(64), "trace_sha256": "e".repeat(64),
            "capture_manifest": {"context": context},
            "playback": {"status": "rendered", "evidence": {
                "fresh_render_matches": true, "reset_render_matches": true,
                "validation_duration_capped": false, "session_duration_frames": 12,
                "silent": true, "silence_threshold_i16": 8, "activity_gap_frames": 36000,
                "pcm": pcm
            }},
            "native_reference": {"status": "matched", "evidence": {
                "projected_pcm_matches": true, "pcm": pcm
            }}
        }
    })
}

#[test]
fn canonical_report_hash_ignores_object_order() {
    let first = json!({"z": [3, {"b": 2, "a": 1}], "a": true});
    let second = json!({"a": true, "z": [3, {"a": 1, "b": 2}]});
    assert_eq!(hash_report(&first).unwrap(), hash_report(&second).unwrap());
}

#[test]
fn wrapper_retains_candidate_spans_and_refuses_a_mutated_source() {
    let source = json!({
        "kind": "direct_cartridge_file", "sha256": "a".repeat(64), "len": 32768,
        "container": null, "selected_member": null
    });
    let mut report = report("gb", source["sha256"].as_str().unwrap(), 32768);
    report["driver_candidates"] = json!([{
        "family": "fixture", "variant": "span", "qualification": "static_code",
        "evidence": [{"span": {"offset": 4660, "len": 3}, "sha256": "b".repeat(64)}]
    }]);
    let wrapped = wrap(
        report.clone(),
        source.clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1", "source": source,
            "transforms": [], "media": report["media"]
        }),
        report["limits"].clone(),
    )
    .unwrap();
    assert_eq!(
        wrapped.pointer("/report/driver_candidates/0/evidence/0/span/offset"),
        Some(&json!(4660))
    );
    let mut changed = report;
    changed["media"]["sha256"] = json!("c".repeat(64));
    assert!(
        wrap(
            changed.clone(),
            wrapped["source_identity"].clone(),
            json!({
                "analysis_profile": "standalone-unmodified-v1",
                "source": wrapped["source_identity"], "transforms": [], "media": changed["media"]
            }),
            changed["limits"].clone(),
        )
        .is_err()
    );
    let mut profile = wrapped.clone();
    profile["analysis_source"]["analysis_profile"] = json!("modified");
    assert!(validate(&profile).is_err());
    let mut transforms = wrapped;
    transforms["analysis_source"]["transforms"] = json!([{"kind": "patch"}]);
    assert!(validate(&transforms).is_err());
}

#[test]
fn bound_gb_and_nes_rows_require_matching_context_and_validation() {
    for system in ["gb", "nes"] {
        let evidence = evidence(system);
        let row = bound_row(&evidence, system);
        assert!(binding_matches(&evidence, &row));
        let mut changed = row.clone();
        changed["capture"]["context"]["source"]["loaded_media"]["sha256"] = json!("b".repeat(64));
        assert!(!binding_matches(&evidence, &changed));
        let mut unvalidated = row;
        unvalidated["validation"]["native_reference"]["status"] = json!("mismatch");
        assert!(!binding_matches(&evidence, &unvalidated));
    }
}

#[test]
fn serialized_driver_report_uses_a_complete_status_binding() {
    let bytes = vec![0; 32768];
    let hash = zeff_firmware::sha256_hex(&bytes);
    let report = serde_json::to_value(zeff_audio_discovery::drivers::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        zeff_audio_discovery::ScanLimits::default(),
        &std::sync::atomic::AtomicBool::new(false),
    ))
    .unwrap();
    assert_eq!(report.pointer("/status/kind"), Some(&json!("complete")));
    let source = json!({
        "kind": "direct_cartridge_file", "sha256": hash, "len": bytes.len(),
        "container": null, "selected_member": null
    });
    let wrapped = wrap(
        report.clone(),
        source.clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1", "source": source,
            "transforms": [], "media": report["media"]
        }),
        report["limits"].clone(),
    )
    .unwrap();
    assert!(binding_matches(&wrapped, &bound_row(&wrapped, "gb")));
}

#[test]
fn limits_cancellation_and_source_mutation_cannot_bind() {
    let evidence = evidence("gb");
    let mut changed_limits = evidence.clone();
    changed_limits["analysis_limits"]["max_work"] = json!(99);
    assert!(validate(&changed_limits).is_err());
    let source = evidence["source_identity"].clone();
    let mut incomplete_report = evidence["report"].clone();
    incomplete_report["status"] = json!({"kind": "incomplete", "reason": "cancelled"});
    let cancelled = wrap(
        incomplete_report.clone(),
        source.clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1", "source": source,
            "transforms": [], "media": incomplete_report["media"]
        }),
        incomplete_report["limits"].clone(),
    )
    .unwrap();
    assert!(validate(&cancelled).is_ok());
    assert!(!binding_matches(&cancelled, &bound_row(&cancelled, "gb")));
    assert_eq!(
        reference_for_row(&cancelled, &bound_row(&cancelled, "gb"), false)["status"],
        "not_bound"
    );
    let mut row = bound_row(&evidence, "gb");
    row["status"] = json!("source_changed");
    row["candidate_evidence"] = reference(&evidence, false);
    assert!(!binding_matches(&evidence, &row));
}

#[test]
fn malformed_success_rows_are_emitted_as_not_bound() {
    let evidence = evidence("gb");
    let mut row = bound_row(&evidence, "gb");
    row["validation"]["playback"]["evidence"]["pcm"] = Value::Null;
    assert_eq!(
        reference_for_row(&evidence, &row, false)["status"],
        "not_bound"
    );
}
