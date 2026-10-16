use super::{parse_jsonl, validate_original_payload, verify};
use crate::reference::Case;
use serde_json::{Value, json};

fn case(word: u16, sr: u16, source: u16) -> Value {
    let immediate = word & 63 == 60;
    let mut initial = json!({
        "d0": 0x12345678_u32, "d1": 1, "d2": 2, "d3": 3,
        "d4": 4, "d5": 5, "d6": 6, "d7": 0xabcd0000_u32,
        "a0": 0x2000, "a1": 0x2002, "a2": 0x2004, "a3": 0x2006,
        "a4": 0x2008, "a5": 0x200a, "a6": 0x200c,
        "usp": 0x6000, "ssp": 0x5000, "sr": sr, "pc": 0x1004,
        "prefetch": [word, if immediate {source} else {0x7001}],
        "ram": [[0x1000,0],[0x1001,0],[0x1002,0xde],[0x1003,0xad],
            [0x1004,0x4e],[0x1005,0x71],[0x1006,0x72],[0x1007,1]]
    });
    if !immediate {
        initial[format!("d{}", word & 7)] = json!(0xabcd0000_u32 | u32::from(source));
    }
    let to_sr = word & 0xffc0 == 0x46c0;
    let next_sr = if to_sr {
        source & 0xa71f
    } else {
        sr & 0xffe0 | source & 31
    };
    let old_fc = if sr & 0x2000 == 0 { 2 } else { 6 };
    let new_fc = if next_sr & 0x2000 == 0 { 2 } else { 6 };
    let mut expected = initial.clone();
    expected["sr"] = json!(next_sr);
    expected["pc"] = json!(if immediate { 0x1008 } else { 0x1006 });
    expected["prefetch"] = if immediate {
        json!([0x4e71, 0x7201])
    } else {
        json!([0xdead, 0x4e71])
    };
    let read = |address, value, fc| {
        json!({"kind":"read", "duration_cycles":4,
        "function_code":fc, "address_bus":address, "data_bus":value, "uds":1, "lds":1})
    };
    let mut phases = Vec::new();
    if immediate {
        phases.push(read(0x1004, 0x4e71, old_fc));
    }
    phases.push(json!({"kind":"idle","duration_cycles":4}));
    phases.extend(if immediate {
        [read(0x1004, 0x4e71, new_fc), read(0x1006, 0x7201, new_fc)]
    } else {
        [read(0x1002, 0xdead, new_fc), read(0x1004, 0x4e71, new_fc)]
    });
    json!({"cohort":if to_sr {"MOVEtoSR"} else {"MOVEtoCCR"}, "name":"synthetic-status",
        "initial":initial,"final":expected,"reference_total_cycles":if immediate {16} else {12},
        "transactions":phases,"normalization_exclusion":null})
}

fn typed(value: Value) -> Case {
    serde_json::from_value(value).unwrap()
}

#[test]
fn register_and_immediate_full_state_use_original_source_and_actual_ram() {
    for word in [0x44c0, 0x44c7, 0x44fc, 0x46c0, 0x46c7, 0x46fc] {
        for source in [0, 31, 0xffff, 0x805f] {
            verify(&typed(case(word, 0x2510, source))).unwrap();
        }
    }
    for word in [0x44c0, 0x44c7, 0x44fc] {
        verify(&typed(case(word, 0x0510, 0xffff))).unwrap();
    }
}

#[test]
fn immediate_prefix_uses_old_fc_and_refetch_uses_new_fc() {
    let seed = case(0x46fc, 0x2010, 0x805f);
    for index in [0, 2, 3] {
        let mut row = seed.clone();
        row["transactions"][index]["function_code"] = json!(if index == 0 { 2 } else { 6 });
        assert!(verify(&typed(row)).is_err());
    }
}

#[test]
fn every_raw_phase_field_and_order_is_checked() {
    let seed = case(0x46fc, 0x2010, 0x001f);
    for field in ["duration_cycles", "address_bus", "data_bus", "uds", "lds"] {
        let mut row = seed.clone();
        let old = row["transactions"][0][field].as_u64().unwrap();
        row["transactions"][0][field] = json!(old + 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    let mut row = seed.clone();
    row["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(verify(&typed(row)).is_err());
    let mut row = seed;
    row["reference_total_cycles"] = json!(17);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn original_final_registers_queue_and_ram_are_not_trusted_as_oracle() {
    for field in ["d0", "a6", "usp", "ssp", "pc", "sr"] {
        let mut row = case(0x46fc, 0x2010, 0x001f);
        let old = row["final"][field].as_u64().unwrap();
        row["final"][field] = json!(old ^ 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    let mut row = case(0x46fc, 0x2010, 0x001f);
    row["final"]["prefetch"][0] = json!(0x7000);
    assert!(verify(&typed(row)).is_err());
    let mut row = case(0x46fc, 0x2010, 0x001f);
    row["final"]["ram"][0][1] = json!(1);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn unselected_opcode_privilege_trace_and_boundary_are_rejected() {
    for word in [0x44c8, 0x44d0, 0x46fa, 0x40c0] {
        assert!(verify(&typed(case(word, 0x2010, 0x001f))).is_err());
    }
    assert!(verify(&typed(case(0x46fc, 0x0010, 0x001f))).is_err());
    assert!(verify(&typed(case(0x44fc, 0xa010, 0x001f))).is_err());
    let mut row = case(0x44fc, 0x2010, 0x001f);
    row["normalization_exclusion"] = json!("trace");
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn duplicate_identity_ram_and_unknown_json_fields_are_rejected() {
    let seed = case(0x44fc, 0x2010, 0x001f);
    let line = serde_json::to_string(&seed).unwrap();
    assert!(parse_jsonl(&format!("{line}\n{line}\n")).is_err());
    assert!(parse_jsonl("").is_err());
    let mut row = seed.clone();
    let byte = row["initial"]["ram"][0].clone();
    row["initial"]["ram"].as_array_mut().unwrap().push(byte);
    assert!(verify(&typed(row)).is_err());
    let mut row = seed;
    row["invented"] = json!(true);
    assert!(parse_jsonl(&serde_json::to_string(&row).unwrap()).is_err());
    assert!(validate_original_payload(&line).is_err());
}
