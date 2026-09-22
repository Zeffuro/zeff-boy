use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "zeff-audio-candidate-evidence/1";
const QUALIFICATION: &str = "static_only";

pub(super) fn wrap(
    report: Value,
    source_identity: Value,
    analysis_source: Value,
    analysis_limits: Value,
) -> Result<Value> {
    let evidence = json!({
        "schema": SCHEMA,
        "qualification": QUALIFICATION,
        "sha256": hash_report(&report)?,
        "source_identity": source_identity,
        "analysis_source": analysis_source,
        "analysis_limits": analysis_limits,
        "report": report,
    });
    validate(&evidence)?;
    Ok(evidence)
}

pub(super) fn reference(evidence: &Value, bound: bool) -> Value {
    json!({
        "schema": evidence["schema"],
        "qualification": evidence["qualification"],
        "sha256": evidence["sha256"],
        "status": if bound { "bound" } else { "not_bound" },
    })
}

pub(super) fn reference_for_row(evidence: &Value, row: &Value, source_changed: bool) -> Value {
    if source_changed {
        return reference(evidence, false);
    }
    let mut tentative = row.clone();
    tentative["candidate_evidence"] = reference(evidence, true);
    reference(evidence, binding_matches(evidence, &tentative))
}

pub(super) fn binding_matches(evidence: &Value, row: &Value) -> bool {
    if !is_complete(evidence)
        || row["status"] != "success"
        || row["candidate_evidence"] != reference(evidence, true)
        || !validation_matches(row)
    {
        return false;
    }
    let source = &evidence["source_identity"];
    let report = &evidence["report"];
    let system = match report["media"]["system"].as_str() {
        Some(system) => system,
        None => return false,
    };
    let requested_sha256 = if source["container"].is_null() {
        match source["sha256"].as_str() {
            Some(hash) => hash,
            None => return false,
        }
    } else {
        match source.pointer("/container/sha256").and_then(Value::as_str) {
            Some(hash) => hash,
            None => return false,
        }
    };
    let selectors = match system {
        "gb" => match row.pointer("/capture/context/settings/mode_preference") {
            Some(Value::String(value)) if value == "Auto" => json!({"gb_mode": "auto"}),
            Some(Value::String(value)) if value == "ForceDmg" => json!({"gb_mode": "dmg"}),
            Some(Value::String(value)) if value == "ForceCgb" => json!({"gb_mode": "cgb"}),
            _ => return false,
        },
        "nes" => json!({}),
        _ => return false,
    };
    super::report::validate_context_value(
        &row["capture"]["context"],
        source,
        requested_sha256,
        system,
        &row["applied_input"]["player_1"],
        super::report::ExpectedSettings {
            sample_rate: 48_000,
            selectors: &selectors,
            requested_steps: match row["requested_steps"].as_u64() {
                Some(steps) => steps,
                None => return false,
            },
        },
    )
    .is_ok()
}

pub(super) fn is_complete(evidence: &Value) -> bool {
    validate(evidence).is_ok()
        && matches!(
            evidence
                .pointer("/report/media/system")
                .and_then(Value::as_str),
            Some("gb" | "nes")
        )
        && evidence
            .pointer("/report/status/kind")
            .and_then(Value::as_str)
            == Some("complete")
}

fn validation_matches(row: &Value) -> bool {
    let capture = &row["capture"];
    let validation = &row["validation"];
    validation["capture_manifest"]["context"] == capture["context"]
        && validation["status"] == "integrity_verified"
        && validation["archive_sha256"] == capture["archive_sha256"]
        && validation["trace_sha256"] == capture["trace_sha256"]
        && validation["playback"]["status"] == "rendered"
        && validation["playback"]["evidence"]["fresh_render_matches"] == true
        && validation["playback"]["evidence"]["reset_render_matches"] == true
        && validation["playback"]["evidence"]["validation_duration_capped"] == false
        && validation["native_reference"]["status"] == "matched"
        && validation["native_reference"]["evidence"]["projected_pcm_matches"] == true
        && super::comparison::valid_row(row)
}

pub(super) fn validate(evidence: &Value) -> Result<()> {
    ensure!(
        evidence["schema"] == SCHEMA && evidence["qualification"] == QUALIFICATION,
        "candidate evidence has an unsupported schema or qualification"
    );
    let source = &evidence["source_identity"];
    let report = &evidence["report"];
    ensure!(
        evidence["sha256"] == hash_report(report)?,
        "candidate evidence report hash does not match its canonical report"
    );
    ensure!(
        report["schema"] == "zeff-audio-driver-evidence/1"
            && report["media"]["system"].is_string()
            && report["media"]["sha256"] == source["sha256"]
            && report["media"]["byte_len"] == source["len"]
            && evidence.pointer("/analysis_source/source") == Some(source)
            && evidence.pointer("/analysis_source/media") == Some(&report["media"])
            && evidence.pointer("/analysis_source/analysis_profile")
                == Some(&Value::String("standalone-unmodified-v1".to_owned()))
            && evidence.pointer("/analysis_source/transforms") == Some(&Value::Array(Vec::new()))
            && evidence["analysis_limits"] == report["limits"],
        "candidate evidence does not describe the analyzed source and limits"
    );
    Ok(())
}

fn hash_report(report: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(&canonical(report))
        .context("could not serialize canonical driver evidence report")?;
    Ok(const_hex::encode(Sha256::digest(bytes)))
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect::<Map<_, _>>(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
#[path = "evidence/tests.rs"]
mod tests;
