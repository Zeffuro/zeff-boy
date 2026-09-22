use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{comparison, evidence};

#[cfg(test)]
#[path = "census/tests.rs"]
mod tests;

#[derive(Default)]
struct Inventory {
    findings: usize,
    candidates: usize,
    structural: usize,
    selectors: usize,
    held_selectors: usize,
    code: usize,
}

pub(super) fn summarize(binding: &Value, observation: &Value, rows: &[Value]) -> Value {
    let report = &binding["report"];
    if evidence::validate(binding).is_err()
        || observation["system"] != report["media"]["system"]
        || observation["media_sha256"] != report["media"]["sha256"]
    {
        return unavailable("invalid_static_identity");
    }
    if rows.len() > 16 {
        return unavailable("too_many_plans");
    }
    let Some(groups) = inventory_groups(report) else {
        return unavailable("invalid_candidate_inventory");
    };
    let bound = rows
        .iter()
        .filter(|row| evidence::binding_matches(binding, row))
        .cloned()
        .collect::<Vec<_>>();
    let mut statuses = BTreeMap::<&str, usize>::new();
    for row in rows {
        *statuses
            .entry(row["status"].as_str().unwrap_or("invalid_status"))
            .or_default() += 1;
    }
    let comparison = comparison::compare(&bound);
    let compared = comparison["comparisons"].as_array();
    let pcm_count = |status: &str| {
        compared.map_or(0, |rows| rows.iter().filter(|r| r["pcm"] == status).count())
    };
    let silent = bound
        .iter()
        .filter(|row| row.pointer("/validation/playback/evidence/silent") == Some(&json!(true)))
        .count();
    let eligibility = if bound.is_empty() {
        "not_validated"
    } else {
        "validated_source_capture"
    };
    let candidate_inputs = usize::from(!groups.is_empty());
    let structural_inputs = usize::from(groups.values().any(|group| group.structural > 0));
    let input_counts = input_counts(observation);
    let groups = groups
        .into_iter()
        .map(|((family, variant, qualification), inventory)| {
            json!({
                "system": report["media"]["system"],
                "family": family, "variant": variant, "qualification": qualification,
                "capture_eligibility": eligibility, "candidate_inputs": 1,
                "structural_inputs": usize::from(inventory.structural > 0),
                "findings": inventory.findings, "candidates": inventory.candidates,
                "structural_inventories": inventory.structural,
                "structural_selectors": inventory.selectors,
                "held_structural_selectors": inventory.held_selectors,
                "code_inventories": inventory.code,
                "input_coverage": &input_counts,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema": "zeff-audio-candidate-census/1",
        "status": "complete", "qualification": "static_only",
        "evidence_sha256": binding["sha256"],
        "media": report["media"], "static_scan_status": report["status"],
        "candidate_inputs": candidate_inputs, "structural_inputs": structural_inputs,
        "input_coverage": input_counts, "groups": groups,
        "plans": {
            "total": rows.len(), "status_histogram": statuses,
            "bound": bound.len(), "not_bound": rows.len() - bound.len(),
            "silent": silent, "active": bound.len() - silent,
            "comparison_status": comparison["status"],
            "same_pcm": pcm_count("same_pcm"), "different_pcm": pcm_count("different_pcm"),
            "duplicate_pcm_groups": super::report::duplicates(&bound).len(),
        },
        "limitations": [
            "Groups associate static evidence with the same input, not runtime callsites or songs.",
            "Input coverage describes the ROM and does not qualify individual candidates; groups may overlap.",
        ],
    })
}

fn input_counts(observation: &Value) -> Value {
    let count = |key: &str| observation[key].as_u64().unwrap_or(0);
    json!({
        "catalogued_inputs": usize::from(count("catalog_entries") > 0),
        "pending_runtime_inputs": usize::from(count("pending_runtime_entries") > 0),
        "render_supported_inputs": usize::from(count("render_supported_entries") > 0),
        "evidence_only_inputs": usize::from(observation["stage"] == "driver_evidence"),
        "catalog_entries": count("catalog_entries"),
        "pending_runtime_entries": count("pending_runtime_entries"),
        "render_supported_entries": count("render_supported_entries"),
    })
}

type Groups = BTreeMap<(String, String, String), Inventory>;

fn inventory_groups(report: &Value) -> Option<Groups> {
    let mut groups = Groups::new();
    let empty = Vec::new();
    let mut kinds = BTreeSet::new();
    for (key, is_candidate) in [("findings", false), ("driver_candidates", true)] {
        let rows = match report.get(key) {
            None => &empty,
            Some(value) => value.as_array()?,
        };
        for row in rows {
            let qualification = if is_candidate {
                row["qualification"].as_str()?
            } else {
                row["qualification"]["kind"].as_str()?
            };
            if !matches!(
                qualification,
                "fingerprint_only" | "static_code" | "structural" | "candidate" | "known_rom"
            ) {
                return None;
            }
            let key = (
                row["family"].as_str()?.to_owned(),
                row["variant"].as_str()?.to_owned(),
                qualification.to_owned(),
            );
            kinds.insert(key.clone());
            if kinds.len() > 4096 {
                return None;
            }
            let group = groups.entry(key).or_default();
            if is_candidate {
                group.candidates += 1;
            } else {
                group.findings += 1;
            }
            if let Some(inventory) = row.get("inventory") {
                group.structural += 1;
                group.selectors += inventory["entries"].as_array()?.len();
                group.held_selectors += inventory["held"].as_array()?.len();
            }
            group.code += usize::from(row.get("code").is_some_and(Value::is_object));
        }
    }
    Some(groups)
}

fn unavailable(reason: &str) -> Value {
    json!({"schema": "zeff-audio-candidate-census/1", "status": "unavailable", "reason": reason, "groups": []})
}
