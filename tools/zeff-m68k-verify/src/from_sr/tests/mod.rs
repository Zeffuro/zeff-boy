use super::{Selection, parse_jsonl, verify};
use crate::reference::Case;
use serde_json::{Value, json};

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
fn original_forms_preserve_full_status_and_actual_ram() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let rows = originals(selection);
        assert_eq!(
            rows.len(),
            if selection == Selection::FirstOdd {
                7
            } else {
                8
            }
        );
        for row in rows {
            verify(&case(&row), selection)
                .unwrap_or_else(|error| panic!("{}: {error}", row["name"]));
        }
    }
}

#[test]
fn trace_and_wrong_destination_are_rejected_before_execution() {
    let trace: Value = serde_json::from_str(include_str!("data/trace.json")).unwrap();
    assert!(
        verify(&case(&trace), Selection::FirstOdd)
            .unwrap_err()
            .contains("trace")
    );
    for opcode in [0x40c8, 0x40fa, 0x42c0, 0x44d0] {
        let mut row = originals(Selection::Completion).remove(0);
        row["initial"]["prefetch"][0] = json!(opcode);
        assert!(verify(&case(&row), Selection::Completion).is_err());
    }
}

#[test]
fn malformed_and_duplicate_identity_are_rejected() {
    let row = serde_json::to_string(&originals(Selection::Completion)[0]).unwrap();
    assert!(
        parse_jsonl(&format!("{row}\n{row}\n"))
            .err()
            .unwrap()
            .contains("duplicate")
    );
    assert!(parse_jsonl("").is_err());
    let mut row = originals(Selection::FirstOdd).remove(0);
    row["normalization_exclusion"] = json!(null);
    assert!(verify(&case(&row), Selection::FirstOdd).is_err());
}

#[test]
fn missing_actual_ram_cannot_be_seeded_from_transactions() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let mut row = originals(selection).remove(0);
        row["initial"]["ram"] = json!([]);
        assert!(
            verify(&case(&row), selection)
                .unwrap_err()
                .contains("actual RAM")
        );
    }
}

#[test]
fn transfer_order_data_lanes_and_duration_are_required() {
    let original = originals(Selection::Completion).remove(0);
    for field in [
        "data_bus",
        "function_code",
        "duration_cycles",
        "uds",
        "address_bus",
    ] {
        let mut row = original.clone();
        row["transactions"][0][field] = json!(row["transactions"][0][field].as_u64().unwrap() ^ 1);
        assert!(verify(&case(&row), Selection::Completion).is_err());
    }
    let mut row = original.clone();
    row["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(verify(&case(&row), Selection::Completion).is_err());
    let mut row = original;
    row["reference_total_cycles"] = json!(0);
    assert!(verify(&case(&row), Selection::Completion).is_err());
}

#[test]
fn complete_register_queue_and_ram_results_are_required() {
    for selection in [Selection::FirstOdd, Selection::Completion] {
        let original = originals(selection).remove(0);
        for field in ["d0", "d7", "usp", "ssp", "sr", "pc"] {
            let mut row = original.clone();
            row["final"][field] = json!(row["final"][field].as_u64().unwrap() ^ 2);
            assert!(verify(&case(&row), selection).is_err());
        }
        let mut row = original.clone();
        row["final"]["prefetch"][0] = json!(row["final"]["prefetch"][0].as_u64().unwrap() ^ 1);
        assert!(verify(&case(&row), selection).is_err());
        let mut row = original;
        row["final"]["ram"].as_array_mut().unwrap().pop();
        assert!(verify(&case(&row), selection).is_err());
    }
}

#[test]
fn duplicate_ram_and_wrong_selection_are_rejected() {
    let mut row = originals(Selection::Completion).remove(0);
    let byte = row["initial"]["ram"][0].clone();
    row["initial"]["ram"].as_array_mut().unwrap().push(byte);
    assert!(
        verify(&case(&row), Selection::Completion)
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(
        verify(
            &case(&originals(Selection::Completion)[0]),
            Selection::FirstOdd
        )
        .is_err()
    );
}

#[test]
fn encoded_destination_and_callback_geometry() {
    let mut words = 0;
    let mut callbacks = 0;
    for ea in 0u16..64 {
        let mode = ea >> 3;
        let register = ea & 7;
        if mode == 0 || (2..=6).contains(&mode) || mode == 7 && register <= 1 {
            words += 1;
            callbacks += if mode == 0 { 1 } else { 3 }
                + if mode == 7 && register == 1 {
                    2
                } else {
                    u32::from(mode >= 5)
                };
        }
    }
    assert_eq!((words, callbacks), (50, 153));
}
