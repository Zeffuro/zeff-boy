use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{CorpusReport, InputResult, Limits, Observation, Row};

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct Counts {
    inputs: usize,
    inputs_with_catalog_entries: usize,
    inputs_with_render_support: usize,
    catalog_entries: usize,
    render_supported_entries: usize,
    #[serde(default)]
    pending_runtime_entries: usize,
    driver_candidates: usize,
    driver_families: BTreeMap<String, usize>,
    #[serde(default)]
    inputs_with_structural_inventory: usize,
    #[serde(default)]
    structural_inventories: usize,
    #[serde(default)]
    structural_selectors: usize,
    #[serde(default)]
    held_structural_selectors: usize,
    #[serde(default)]
    inputs_with_code_evidence: usize,
    #[serde(default)]
    code_candidates: usize,
    #[serde(default)]
    sound_write_witnesses: usize,
    #[serde(default)]
    inputs_with_sound_callers: usize,
    #[serde(default)]
    sound_call_witnesses: usize,
    #[serde(default)]
    selector_consumers: usize,
    #[serde(default)]
    unparsed_selector_pointers: usize,
    #[serde(default)]
    held_selector_pointers: usize,
    #[serde(default)]
    record_prefixes: usize,
    #[serde(default)]
    unparsed_stream_pointers: usize,
    #[serde(default)]
    held_stream_pointers: usize,
    #[serde(default)]
    command_dispatches: usize,
    #[serde(default)]
    guarded_command_fetches: usize,
    #[serde(default)]
    stream_fetch_bindings: usize,
    #[serde(default)]
    conditional_head_command_edges: usize,
    stages: BTreeMap<String, usize>,
    blockers: BTreeMap<String, usize>,
    detector_states: BTreeMap<String, BTreeMap<String, usize>>,
}

impl Counts {
    fn add(&mut self, observation: &Observation) {
        self.inputs += 1;
        self.inputs_with_catalog_entries += usize::from(observation.catalog_entries != 0);
        self.inputs_with_render_support += usize::from(observation.render_supported_entries != 0);
        self.catalog_entries += observation.catalog_entries;
        self.render_supported_entries += observation.render_supported_entries;
        self.pending_runtime_entries += observation.pending_runtime_entries;
        self.driver_candidates += observation.driver_candidates;
        if let Some(code) = &observation.code {
            self.inputs_with_code_evidence += 1;
            self.code_candidates += code.candidates;
            self.sound_write_witnesses += code.sound_register_writes;
            self.inputs_with_sound_callers += usize::from(code.direct_calls != 0);
            self.sound_call_witnesses += code.direct_calls;
            self.selector_consumers += code.selector_consumers;
            self.unparsed_selector_pointers += code.unparsed_selector_pointers;
            self.held_selector_pointers += code.held_selector_pointers;
            self.record_prefixes += code.record_prefixes;
            self.unparsed_stream_pointers += code.unparsed_stream_pointers;
            self.held_stream_pointers += code.held_stream_pointers;
            self.command_dispatches += code.command_dispatches;
            self.guarded_command_fetches += code.guarded_command_fetches;
            self.stream_fetch_bindings += code.stream_fetch_bindings;
            self.conditional_head_command_edges += code.conditional_head_command_edges;
        }
        if let Some(structural) = &observation.structural {
            self.inputs_with_structural_inventory += 1;
            self.structural_inventories += structural.inventories;
            self.structural_selectors += structural.selectors;
            self.held_structural_selectors += structural.held_selectors;
        }
        for (family, count) in &observation.driver_families {
            *self.driver_families.entry(family.clone()).or_default() += count;
        }
        increment(&mut self.stages, observation.stage.as_str());
        for reason in observation
            .blockers
            .iter()
            .map(|b| b.as_str())
            .collect::<BTreeSet<_>>()
        {
            increment(&mut self.blockers, reason);
        }
        for detector in &observation.detectors {
            increment(
                self.detector_states.entry(detector.id.clone()).or_default(),
                &state_key(detector.state),
            );
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct DetectorCounts {
    inputs: usize,
    inputs_with_catalog_entries: usize,
    inputs_with_render_support: usize,
    retained_matches: u64,
    catalog_entries: usize,
    render_supported_entries: usize,
    #[serde(default)]
    pending_runtime_entries: usize,
    states: BTreeMap<String, usize>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct EngineCounts {
    inputs: usize,
    catalog_entries: usize,
    render_supported_entries: usize,
    #[serde(default)]
    pending_runtime_entries: usize,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct Summary {
    total_inputs: usize,
    input_errors: usize,
    scanned: Counts,
    systems: BTreeMap<String, Counts>,
    detectors: BTreeMap<String, DetectorCounts>,
    engines: BTreeMap<String, EngineCounts>,
    input_error_reasons: BTreeMap<String, usize>,
}

impl Summary {
    pub(super) fn from_rows(rows: &[Row]) -> Self {
        let mut summary = Self {
            total_inputs: rows.len(),
            ..Self::default()
        };
        for row in rows {
            match &row.result {
                InputResult::InputError { reason, .. } => {
                    summary.input_errors += 1;
                    increment(&mut summary.input_error_reasons, reason);
                }
                InputResult::Scanned { observation, .. } => {
                    summary.scanned.add(observation);
                    summary
                        .systems
                        .entry(observation.system.clone())
                        .or_default()
                        .add(observation);
                    for detector in &observation.detectors {
                        let counts = summary.detectors.entry(detector.id.clone()).or_default();
                        counts.inputs += 1;
                        counts.inputs_with_catalog_entries +=
                            usize::from(detector.catalog_entries != 0);
                        counts.inputs_with_render_support +=
                            usize::from(detector.render_supported_entries != 0);
                        counts.retained_matches += u64::from(detector.retained_matches);
                        counts.catalog_entries += detector.catalog_entries;
                        counts.render_supported_entries += detector.render_supported_entries;
                        counts.pending_runtime_entries += detector.pending_runtime_entries;
                        increment(&mut counts.states, &state_key(detector.state));
                    }
                    for engine in &observation.engines {
                        let counts = summary.engines.entry(engine.engine.clone()).or_default();
                        counts.inputs += 1;
                        counts.catalog_entries += engine.catalog_entries;
                        counts.render_supported_entries += engine.render_supported_entries;
                        counts.pending_runtime_entries += engine.pending_runtime_entries;
                    }
                }
            }
        }
        summary
    }
}

fn increment(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_owned()).or_default() += 1;
}

fn state_key(state: zeff_audio_discovery::coverage::DetectorCoverageState) -> String {
    let state = serde_json::to_value(state).expect("serializable detector state");
    match state["reason"].as_str() {
        Some(reason) => format!("{}:{reason}", state["kind"].as_str().unwrap()),
        None => state["kind"].as_str().unwrap().to_owned(),
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct Comparison {
    matched_inputs: usize,
    added_inputs: Vec<String>,
    removed_inputs: Vec<String>,
    incomparable_inputs: BTreeMap<String, String>,
    new_input_errors: Vec<String>,
    recovered_input_errors: Vec<String>,
    newly_render_supported: Vec<String>,
    lost_render_support: Vec<String>,
    fewer_render_supported_entries: Vec<String>,
    fewer_catalog_entries: Vec<String>,
    changed_observations: Vec<String>,
    changed_scan_reports: Vec<String>,
}

impl Comparison {
    pub(super) fn between(old: &CorpusReport, rows: &[Row], limits: Limits) -> Self {
        let previous = old
            .rows
            .iter()
            .map(|row| (row.input.id.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let current = rows
            .iter()
            .map(|row| (row.input.id.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let mut comparison = Self::default();
        for id in previous.keys().filter(|id| !current.contains_key(**id)) {
            comparison.removed_inputs.push((*id).to_owned());
        }
        for (id, row) in current {
            let Some(before) = previous.get(id) else {
                comparison.added_inputs.push(id.to_owned());
                continue;
            };
            match (&before.result, &row.result) {
                (InputResult::Scanned { .. }, InputResult::InputError { .. }) => {
                    comparison.new_input_errors.push(id.to_owned())
                }
                (InputResult::InputError { .. }, InputResult::Scanned { .. }) => {
                    comparison.recovered_input_errors.push(id.to_owned())
                }
                _ => {}
            }
            if old.limits != limits {
                comparison
                    .incomparable_inputs
                    .insert(id.to_owned(), "scan_limits_changed".to_owned());
                continue;
            }
            let (
                InputResult::Scanned {
                    analysis_profile: old_profile,
                    scan_sha256: old_scan,
                    observation: old_observation,
                },
                InputResult::Scanned {
                    analysis_profile: new_profile,
                    scan_sha256: new_scan,
                    observation: new_observation,
                },
            ) = (&before.result, &row.result)
            else {
                comparison.incomparable_inputs.insert(
                    id.to_owned(),
                    "input_error_without_verified_media_identity".to_owned(),
                );
                continue;
            };
            if old_profile != new_profile {
                comparison
                    .incomparable_inputs
                    .insert(id.to_owned(), "analysis_profile_changed".to_owned());
                continue;
            }
            if old_observation.system != new_observation.system
                || old_observation.media_sha256.is_none()
                || old_observation.media_sha256 != new_observation.media_sha256
            {
                comparison.incomparable_inputs.insert(
                    id.to_owned(),
                    "media_identity_changed_or_missing".to_owned(),
                );
                continue;
            }
            comparison.matched_inputs += 1;
            if old_observation.render_supported_entries == 0
                && new_observation.render_supported_entries > 0
            {
                comparison.newly_render_supported.push(id.to_owned());
            }
            if old_observation.render_supported_entries > 0
                && new_observation.render_supported_entries == 0
            {
                comparison.lost_render_support.push(id.to_owned());
            }
            if new_observation.render_supported_entries < old_observation.render_supported_entries {
                comparison
                    .fewer_render_supported_entries
                    .push(id.to_owned());
            }
            if new_observation.catalog_entries < old_observation.catalog_entries {
                comparison.fewer_catalog_entries.push(id.to_owned());
            }
            if old_observation != new_observation {
                comparison.changed_observations.push(id.to_owned());
            }
            if old_scan != new_scan {
                comparison.changed_scan_reports.push(id.to_owned());
            }
        }
        comparison
    }
}
