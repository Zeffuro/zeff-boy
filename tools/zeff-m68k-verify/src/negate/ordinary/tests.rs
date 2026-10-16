use super::verify;
use crate::reference::Case;
use serde_json::Value;

fn originals() -> Vec<Value> {
    serde_json::from_str(include_str!("../tests/data/completion.json")).unwrap()
}

fn case(value: Value) -> Case {
    serde_json::from_value(value).unwrap()
}

#[test]
fn all_original_negate_operations_and_address_routes_execute_ordinary_path() {
    let values = originals();
    assert_eq!(values.len(), 28);
    for value in values {
        verify(&case(value)).unwrap();
    }
}

#[test]
fn ordinary_completion_rejects_missing_ram_and_inconsistent_results() {
    let original = originals().remove(0);
    let mut missing = original.clone();
    missing["initial"]["ram"] = serde_json::json!([]);
    assert!(verify(&case(missing)).is_err());
    for field in ["d7", "usp", "ssp", "sr", "pc"] {
        let mut value = original.clone();
        value["final"][field] = serde_json::json!(value["final"][field].as_u64().unwrap() ^ 2);
        assert!(verify(&case(value)).is_err());
    }
    for field in ["prefetch", "ram"] {
        let mut value = original.clone();
        value["final"][field].as_array_mut().unwrap().pop();
        if field == "ram" {
            assert!(verify(&case(value)).is_err());
        } else {
            assert!(serde_json::from_value::<Case>(value).is_err());
        }
    }
}

#[test]
fn ordinary_completion_rejects_raw_order_data_clocks_trace_and_odd_source() {
    let original = originals().remove(0);
    for field in [
        "data_bus",
        "function_code",
        "address_bus",
        "duration_cycles",
    ] {
        let mut value = original.clone();
        let index = value["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .position(|phase| phase["kind"] == "write")
            .unwrap();
        value["transactions"][index][field] =
            serde_json::json!(value["transactions"][index][field].as_u64().unwrap() ^ 1);
        assert!(verify(&case(value)).is_err());
    }
    let mut reordered = original.clone();
    reordered["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(verify(&case(reordered)).is_err());
    let mut value = original;
    value["reference_total_cycles"] = serde_json::json!(0);
    assert!(verify(&case(value)).is_err());
    let trace: Value = serde_json::from_str(include_str!("../tests/data/trace.json")).unwrap();
    assert!(verify(&case(trace)).unwrap_err().contains("trace"));
    let odd: Vec<Value> =
        serde_json::from_str(include_str!("../tests/data/firstodd.json")).unwrap();
    assert!(
        verify(&case(odd[0].clone()))
            .unwrap_err()
            .contains("aligned")
    );
}
