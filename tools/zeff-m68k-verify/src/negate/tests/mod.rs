use super::{Selection, parse_jsonl, verify};
use crate::reference::Case;
use serde_json::Value;

fn originals(selection: Selection) -> Vec<Value> {
    serde_json::from_str(match selection {
        Selection::FirstOdd => include_str!("data/firstodd.json"),
        Selection::Completion => include_str!("data/completion.json"),
    })
    .unwrap()
}

fn case(value: &Value) -> Case {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn all_four_original_cohorts_and_seven_routes_match_full_physical_results() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let values = originals(selection);
        assert_eq!(values.len(), 28);
        for value in values {
            verify(&case(&value), selection).unwrap();
        }
    }
}

#[test]
fn original_trace_record_is_excluded_before_execution() {
    let value: Value = serde_json::from_str(include_str!("data/trace.json")).unwrap();
    assert!(
        verify(&case(&value), Selection::FirstOdd)
            .unwrap_err()
            .contains("trace")
    );
}

#[test]
fn duplicate_identity_is_rejected_before_execution() {
    let value = originals(Selection::Completion).remove(0);
    let row = serde_json::to_string(&value).unwrap();
    assert!(
        parse_jsonl(&format!("{row}\n{row}\n"))
            .err()
            .unwrap()
            .contains("duplicate")
    );
}

#[test]
fn missing_actual_operand_ram_cannot_be_seeded_from_raw_reads() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let mut value = originals(selection).remove(0);
        value["initial"]["ram"] = serde_json::json!([]);
        assert!(
            verify(&case(&value), selection)
                .unwrap_err()
                .contains("actual RAM")
        );
    }
}

#[test]
fn malformed_exclusion_cohort_and_transfer_fields_are_rejected() {
    let original = originals(Selection::FirstOdd).remove(0);
    for (field, changed) in [
        ("normalization_exclusion", serde_json::json!(null)),
        ("cohort", serde_json::json!("CLR.w")),
    ] {
        let mut value = original.clone();
        value[field] = changed;
        assert!(verify(&case(&value), Selection::FirstOdd).is_err());
    }
    for (field, changed) in [
        ("function_code", 7),
        ("uds", 0),
        ("address_bus", 1),
        ("duration_cycles", 3),
    ] {
        let mut value = original.clone();
        let index = value["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .position(|p| p["kind"] == "read_address_error")
            .unwrap();
        value["transactions"][index][field] = serde_json::json!(changed);
        assert!(verify(&case(&value), Selection::FirstOdd).is_err());
    }
}

#[test]
fn raw_data_direction_order_and_total_are_qualified_independently() {
    let original = originals(Selection::Completion).remove(0);
    for field in ["data_bus", "function_code"] {
        let mut value = original.clone();
        let index = value["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .position(|p| p["kind"] == "write")
            .unwrap();
        value["transactions"][index][field] =
            serde_json::json!(value["transactions"][index][field].as_u64().unwrap() ^ 1);
        assert!(verify(&case(&value), Selection::Completion).is_err());
    }
    let mut value = original.clone();
    value["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(verify(&case(&value), Selection::Completion).is_err());
    let mut value = original;
    value["reference_total_cycles"] = serde_json::json!(0);
    assert!(verify(&case(&value), Selection::Completion).is_err());
}

#[test]
fn full_final_registers_prefetch_and_ram_are_required() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let original = originals(selection).remove(0);
        for field in ["d7", "usp", "ssp", "sr", "pc"] {
            let mut value = original.clone();
            value["final"][field] = serde_json::json!(value["final"][field].as_u64().unwrap() ^ 2);
            assert!(verify(&case(&value), selection).is_err());
        }
        let mut value = original.clone();
        value["final"]["prefetch"][1] =
            serde_json::json!(value["final"]["prefetch"][1].as_u64().unwrap() ^ 1);
        assert!(verify(&case(&value), selection).is_err());
        let mut value = original;
        value["final"]["ram"].as_array_mut().unwrap().pop();
        assert!(verify(&case(&value), selection).is_err());
    }
}

#[test]
fn duplicate_ram_and_wrong_selection_are_rejected() {
    let mut value = originals(Selection::Completion).remove(0);
    let byte = value["initial"]["ram"][0].clone();
    value["initial"]["ram"].as_array_mut().unwrap().push(byte);
    assert!(
        verify(&case(&value), Selection::Completion)
            .unwrap_err()
            .contains("duplicate")
    );
    let value = originals(Selection::Completion).remove(0);
    assert!(verify(&case(&value), Selection::FirstOdd).is_err());
}

#[test]
fn encoded_geometry_is_168_execution_and_748_callback_positions() {
    let mut encodings = 0;
    let mut callbacks = 0;
    let mut legal = 0;
    for family in [0x4400u16, 0x4000] {
        for size in 0..3 {
            for ea in 0..64 {
                let mode = ea >> 3;
                let reg = ea & 7;
                if mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 1 {
                    legal += 1;
                }
                if size != 0 && ((2..=6).contains(&mode) || mode == 7 && reg <= 1) {
                    let opcode = family | size << 6 | ea;
                    assert!(matches!(opcode >> 6 & 3, 1 | 2));
                    encodings += 1;
                    callbacks += if size == 1 { 3 } else { 5 }
                        + if mode == 7 && reg == 1 {
                            2
                        } else {
                            u32::from(mode >= 5)
                        };
                }
            }
        }
    }
    assert_eq!((legal, encodings, callbacks), (300, 168, 748));
}

#[test]
fn payload_digest_rejects_non_original_bytes() {
    assert!(super::validate_original_payload("{}\n", Selection::FirstOdd).is_err());
    assert!(super::validate_original_payload("{}\n", Selection::Completion).is_err());
}
