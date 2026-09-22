use super::*;

#[test]
fn grouping_keeps_variants_and_qualification_separate_without_counting_duplicate_inputs() {
    let candidate = json!({"family": "family", "variant": "a", "qualification": "structural", "inventory": {"entries": [{}, {}], "held": [{}]}});
    let mut other = candidate.clone();
    other["variant"] = json!("b");
    let report = json!({
        "findings": [{"family": "family", "variant": "a", "qualification": {"kind": "known_rom"}}],
        "driver_candidates": [candidate.clone(), candidate, other],
    });
    let groups = inventory_groups(&report).unwrap();
    assert_eq!(groups.len(), 3);
    let repeated = &groups[&("family".into(), "a".into(), "structural".into())];
    assert_eq!(
        (
            repeated.candidates,
            repeated.structural,
            repeated.selectors,
            repeated.held_selectors
        ),
        (2, 2, 4, 2)
    );
    assert_eq!(
        groups[&("family".into(), "a".into(), "known_rom".into())].findings,
        1
    );
}

#[test]
fn activity_does_not_create_catalog_or_render_coverage() {
    let counts = input_counts(
        &json!({"stage": "driver_evidence", "catalog_entries": 0, "render_supported_entries": 0, "pending_runtime_entries": 0, "active": true}),
    );
    assert_eq!(counts["evidence_only_inputs"], 1);
    assert_eq!(counts["catalogued_inputs"], 0);
    assert_eq!(counts["render_supported_inputs"], 0);
    let pending = input_counts(
        &json!({"stage": "catalogued", "catalog_entries": 3, "pending_runtime_entries": 3, "render_supported_entries": 0}),
    );
    assert_eq!(pending["catalogued_inputs"], 1);
    assert_eq!(pending["pending_runtime_inputs"], 1);
    assert_eq!(pending["render_supported_inputs"], 0);
}

#[test]
fn missing_or_mutated_evidence_cannot_produce_a_census() {
    for evidence in [
        json!(null),
        json!({"report": {"driver_candidates": []}, "sha256": "forged"}),
    ] {
        let result = summarize(&evidence, &json!({}), &[]);
        assert_eq!(result["status"], "unavailable");
        assert_eq!(result["reason"], "invalid_static_identity");
        assert_eq!(result["groups"], json!([]));
    }
}

#[test]
fn malformed_inventory_is_not_silently_counted_as_empty() {
    assert!(inventory_groups(&json!({"driver_candidates": {}})).is_none());
    assert!(inventory_groups(&json!({"driver_candidates": [{"family": "x", "variant": "y", "qualification": "playable"}]})).is_none());
    assert!(inventory_groups(&json!({"driver_candidates": [{"family": "x", "variant": "y", "qualification": "structural", "inventory": {"entries": 12, "held": []}}]})).is_none());
    assert!(
        inventory_groups(&json!({"findings": [], "driver_candidates": []}))
            .unwrap()
            .is_empty()
    );
}
