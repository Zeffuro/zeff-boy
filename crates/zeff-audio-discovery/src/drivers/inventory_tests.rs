use std::sync::atomic::AtomicBool;

use zeff_emu_common::system::System;

use crate::{DetectorState, ScanLimits, ScanStatus, ScanStop};

#[test]
fn structural_inventory_is_shared_by_scan_and_sidecar_without_playback() {
    let bytes = super::nes_tose_structure::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let limits = ScanLimits::default();
    let report = crate::scan(System::Nes, &bytes, limits, &cancel);
    let sidecar = super::scan(System::Nes, &bytes, limits, &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(sidecar.status, ScanStatus::Complete);
    assert_eq!(sidecar.driver_candidates, report.driver_candidates);
    assert_eq!(report.driver_candidates.len(), 1);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::Structural
    );
    let inventory = candidate.inventory.as_ref().unwrap();
    assert!(!inventory.entries.is_empty());
    let outcome = report
        .detector_outcomes
        .iter()
        .find(|outcome| outcome.descriptor.id == "nes-tose-structure")
        .unwrap();
    assert_eq!(outcome.state, DetectorState::Complete);
    assert_eq!(outcome.retained_matches, 1);
    assert_eq!(
        report.work_used,
        report
            .detector_outcomes
            .iter()
            .map(|o| o.work_used)
            .sum::<u64>()
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        let evidence = report.candidate_findings().next().unwrap();
        assert_eq!(evidence.inventory.as_ref(), Some(inventory));
        assert_eq!(report.catalog().count(), 0);
        let json = serde_json::to_value(evidence).unwrap();
        assert!(json.get("id").is_none());
        assert!(json.get("capabilities").is_none());
        let observation = crate::coverage::observe(&report);
        assert_eq!(
            observation.stage,
            crate::coverage::CoverageStage::DriverEvidence
        );
        assert_eq!(observation.catalog_entries, 0);
        assert_eq!(observation.render_supported_entries, 0);
        let structural = observation.structural.as_ref().unwrap();
        assert_eq!(structural.inventories, 1);
        assert_eq!(structural.selectors, inventory.entries.len());
        assert_eq!(structural.held_selectors, inventory.held.len());
    }
}

#[test]
fn legacy_fingerprint_json_has_no_structural_inventory() {
    let candidate = super::gb_fingerprints::DriverCandidate {
        family: "test",
        variant: "v1",
        qualification: super::gb_fingerprints::FingerprintQualification::FingerprintOnly,
        fingerprint_source: "source",
        fingerprint_revision: "revision",
        evidence: Vec::new(),
        inventory: None,
        code: None,
    };
    assert_eq!(
        serde_json::to_value(candidate).unwrap(),
        serde_json::json!({
            "family": "test", "variant": "v1", "qualification": "fingerprint_only",
            "fingerprint_source": "source", "fingerprint_revision": "revision", "evidence": []
        })
    );
}

#[test]
fn structural_sidecar_reports_work_and_candidate_stops() {
    let bytes = super::nes_tose_structure::synthetic_rom();
    for (limits, expected) in [
        (
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            ScanStop::WorkLimit,
        ),
        (
            ScanLimits {
                max_candidates: 0,
                ..ScanLimits::default()
            },
            ScanStop::CandidateLimit,
        ),
    ] {
        let report = super::scan(System::Nes, &bytes, limits, &AtomicBool::new(false));
        assert_eq!(report.status, ScanStatus::Incomplete(expected));
        assert!(report.driver_candidates.is_empty());
        assert!(report.work_used <= limits.max_work);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn older_observations_load_without_structural_counts() {
    let report = crate::scan(
        System::Gb,
        &[],
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let observation = crate::coverage::observe(&report);
    let json = serde_json::to_value(&observation).unwrap();
    assert!(json.get("structural").is_none());
    assert!(json.get("code").is_none());
    let restored: crate::coverage::Observation = serde_json::from_value(json).unwrap();
    assert_eq!(restored, observation);
}

#[test]
fn decoded_sound_writes_are_evidence_without_an_inventory_or_selection() {
    let bytes = super::nes_sound_writes::synthetic_selector_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    let sidecar = super::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(report.driver_candidates, sidecar.driver_candidates);
    assert_eq!(report.driver_candidates.len(), 1);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::StaticCode
    );
    assert!(candidate.inventory.is_none());
    let calls = &candidate.code.as_ref().unwrap().calls;
    assert!(!calls.is_empty());
    let consumers = &candidate.code.as_ref().unwrap().selector_consumers;
    assert_eq!(consumers.len(), 1);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let finding = report.candidate_findings().next().unwrap();
        assert_eq!(
            finding.qualification,
            crate::findings::CandidateQualification::StaticCode
        );
        assert!(finding.inventory.is_none());
        assert_eq!(finding.code, candidate.code);
        assert_eq!(report.catalog().count(), 0);
        let observation = crate::coverage::observe(&report);
        assert!(observation.structural.is_none());
        let code = observation.code.as_ref().unwrap();
        assert_eq!(code.candidates, 1);
        assert!(code.sound_register_writes >= 2);
        assert_eq!(code.direct_calls, calls.len());
        assert_eq!(code.selector_consumers, 1);
        assert_eq!(code.unparsed_selector_pointers, 14);
        assert_eq!(code.held_selector_pointers, 3);
        let mut previous = serde_json::to_value(code).unwrap();
        previous.as_object_mut().unwrap().remove("direct_calls");
        previous
            .as_object_mut()
            .unwrap()
            .remove("selector_consumers");
        previous
            .as_object_mut()
            .unwrap()
            .remove("unparsed_selector_pointers");
        previous
            .as_object_mut()
            .unwrap()
            .remove("held_selector_pointers");
        for field in [
            "record_prefixes",
            "unparsed_stream_pointers",
            "held_stream_pointers",
            "command_dispatches",
            "guarded_command_fetches",
            "stream_fetch_bindings",
            "conditional_head_command_edges",
        ] {
            previous.as_object_mut().unwrap().remove(field);
        }
        let restored: crate::coverage::CodeCoverage = serde_json::from_value(previous).unwrap();
        assert_eq!(restored.direct_calls, 0);
        assert_eq!(restored.selector_consumers, 0);
        assert_eq!(restored.unparsed_selector_pointers, 0);
        assert_eq!(restored.held_selector_pointers, 0);
        assert_eq!(restored.record_prefixes, 0);
        assert_eq!(restored.unparsed_stream_pointers, 0);
        assert_eq!(restored.held_stream_pointers, 0);
        assert_eq!(restored.command_dispatches, 0);
        assert_eq!(restored.guarded_command_fetches, 0);
        assert_eq!(restored.stream_fetch_bindings, 0);
        assert_eq!(restored.conditional_head_command_edges, 0);
        assert_eq!(restored.sound_register_writes, code.sound_register_writes);
        assert_eq!(
            observation.stage,
            crate::coverage::CoverageStage::DriverEvidence
        );
    }
}

#[test]
fn command_dispatch_evidence_survives_projection_without_playback() {
    let bytes = super::nes_sound_writes::synthetic_dispatch_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    let sidecar = super::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(report.driver_candidates, sidecar.driver_candidates);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::StaticCode
    );
    assert!(candidate.inventory.is_none());
    let dispatches = &candidate.code.as_ref().unwrap().command_dispatches;
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].fetches.len(), 2);
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert_eq!(
            report.candidate_findings().next().unwrap().code,
            candidate.code
        );
        assert_eq!(report.catalog().count(), 0);
        let observation = crate::coverage::observe(&report);
        let code = observation.code.as_ref().unwrap();
        assert_eq!(code.command_dispatches, 1);
        assert_eq!(code.guarded_command_fetches, 2);
        assert_eq!(observation.render_supported_entries, 0);
        assert!(observation.structural.is_none());
        assert_eq!(
            serde_json::from_value::<crate::coverage::Observation>(
                serde_json::to_value(&observation).unwrap()
            )
            .unwrap(),
            observation
        );
    }
}

#[test]
fn record_pointer_fields_survive_sidecar_and_owned_projection_without_songs() {
    let bytes = super::nes_sound_writes::synthetic_record_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    let sidecar = super::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(report.driver_candidates, sidecar.driver_candidates);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert!(candidate.inventory.is_none());
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::StaticCode
    );
    let records = &candidate.code.as_ref().unwrap().selector_consumers[0].records;
    assert_eq!(records.len(), 16);
    assert_eq!(
        records
            .iter()
            .map(|record| record.streams.len())
            .sum::<usize>(),
        24
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert_eq!(
            report.candidate_findings().next().unwrap().code,
            candidate.code
        );
        assert_eq!(report.catalog().count(), 0);
        let observation = crate::coverage::observe(&report);
        let code = observation.code.as_ref().unwrap();
        assert_eq!(code.record_prefixes, 16);
        assert_eq!(code.unparsed_stream_pointers, 24);
        assert_eq!(code.held_stream_pointers, 0);
        assert_eq!(observation.render_supported_entries, 0);
        assert_eq!(
            observation.stage,
            crate::coverage::CoverageStage::DriverEvidence
        );
        assert_eq!(
            serde_json::from_value::<crate::coverage::Observation>(
                serde_json::to_value(&observation).unwrap()
            )
            .unwrap(),
            observation
        );
    }
}

#[test]
fn stream_fetch_bindings_survive_projection_without_song_admission() {
    let bytes = super::nes_sound_writes::synthetic_binding_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    let sidecar = super::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(sidecar.detector_version, 10);
    assert_eq!(report.driver_candidates, sidecar.driver_candidates);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::StaticCode
    );
    assert!(candidate.inventory.is_none());
    let streams = candidate
        .code
        .as_ref()
        .unwrap()
        .selector_consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
        .collect::<Vec<_>>();
    let bound = streams
        .iter()
        .filter(|stream| stream.fetch_binding.is_some())
        .count();
    assert_eq!(bound, 12);
    for stream in streams {
        if stream.fetch_binding.is_some() {
            assert_eq!(stream.disposition, super::CodePointerDisposition::Unparsed);
        } else {
            assert!(
                serde_json::to_value(stream)
                    .unwrap()
                    .get("fetch_binding")
                    .is_none()
            );
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert_eq!(
            report.candidate_findings().next().unwrap().code,
            candidate.code
        );
        assert_eq!(report.catalog().count(), 0);
        let observation = crate::coverage::observe(&report);
        assert_eq!(
            observation.code.as_ref().unwrap().stream_fetch_bindings,
            bound
        );
        assert_eq!(observation.render_supported_entries, 0);
        assert!(observation.structural.is_none());
        assert_eq!(
            serde_json::from_value::<crate::coverage::Observation>(
                serde_json::to_value(&observation).unwrap()
            )
            .unwrap(),
            observation
        );
    }
}

#[test]
fn conditional_head_edges_survive_projection_without_song_admission() {
    let bytes = super::nes_sound_writes::synthetic_head_edge_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    let sidecar = super::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(sidecar.detector_version, 10);
    assert_eq!(report.driver_candidates, sidecar.driver_candidates);
    assert_eq!(report.song_count(), 0);
    let candidate = &report.driver_candidates[0];
    assert_eq!(
        candidate.qualification,
        super::CandidateQualification::StaticCode
    );
    assert!(candidate.inventory.is_none());
    let mut edges = 0;
    for stream in candidate
        .code
        .as_ref()
        .unwrap()
        .selector_consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
    {
        let value = serde_json::to_value(stream).unwrap();
        if stream.conditional_head_command_edge.is_some() {
            edges += 1;
            assert!(stream.fetch_binding.is_some());
            assert_eq!(stream.disposition, super::CodePointerDisposition::Unparsed);
            assert!(value.get("conditional_head_command_edge").is_some());
        } else {
            assert!(value.get("conditional_head_command_edge").is_none());
        }
    }
    assert_eq!(edges, 12);
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert_eq!(
            report.candidate_findings().next().unwrap().code,
            candidate.code
        );
        assert_eq!(report.catalog().count(), 0);
        let observation = crate::coverage::observe(&report);
        assert_eq!(
            observation
                .code
                .as_ref()
                .unwrap()
                .conditional_head_command_edges,
            edges
        );
        assert_eq!(observation.render_supported_entries, 0);
        assert_eq!(
            observation.stage,
            crate::coverage::CoverageStage::DriverEvidence
        );
        assert_eq!(
            serde_json::from_value::<crate::coverage::Observation>(
                serde_json::to_value(&observation).unwrap()
            )
            .unwrap(),
            observation
        );
    }
}
