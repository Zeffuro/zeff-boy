use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

fn case(opcode: u16, cohort: &str) -> Value {
    let mut value = nop();
    value["cohort"] = json!(cohort);
    value["initial"]["prefetch"][0] = json!(opcode);
    for state in ["initial", "final"] {
        value[state]["ram"][0][1] = json!(opcode >> 8);
        value[state]["ram"][1][1] = json!(opcode & 255);
        value[state]["a0"] = json!(0x2000);
    }
    value
}

fn word(value: &mut Value, state: &str, address: u32, data: u16) {
    value[state]["ram"].as_array_mut().unwrap().extend([
        json!([address, data >> 8]),
        json!([address + 1, data & 255]),
    ]);
}

fn transfer(kind: &str, address: u32, data: u16, fc: u32) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":data,"uds":1,"lds":1})
}

#[test]
fn binary_rmw_reads_then_prefetches_then_writes_saved_ea() {
    let mut value = case(0xd158, "ADD.w");
    value["initial"]["d0"] = json!(2);
    value["final"]["d0"] = json!(2);
    value["final"]["a0"] = json!(0x2002);
    word(&mut value, "initial", 0x2000, 3);
    word(&mut value, "final", 0x2000, 5);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x2000, 3, 5),
        transfer("read", 0x1004, 0x5678, 6),
        transfer("write", 0x2000, 5, 5)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (a, b) in [(0, 1), (1, 2)] {
        let mut bad = value.clone();
        bad["transactions"].as_array_mut().unwrap().swap(a, b);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    let mut bad = value.clone();
    bad["transactions"][2]["address_bus"] = json!(0x2002);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x2000));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn quick_long_predecrement_uses_high_reads_and_low_writes() {
    let mut value = case(0x53a0, "SUB.l");
    value["initial"]["a0"] = json!(0x2004);
    for (state, low) in [("initial", 2), ("final", 1)] {
        word(&mut value, state, 0x2000, 0x1234);
        word(&mut value, state, 0x2002, low);
    }
    value["reference_total_cycles"] = json!(22);
    value["transactions"] = json!([
        {"kind":"idle","duration_cycles":2}, transfer("read",0x2000,0x1234,5),
        transfer("read",0x2002,2,5), transfer("read",0x1004,0x5678,6),
        transfer("write",0x2002,1,5), transfer("write",0x2000,0x1234,5)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"].as_array_mut().unwrap().swap(4, 5);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn compare_memory_keeps_operand_and_requires_internal_idle() {
    let mut value = case(0xb090, "CMP.l");
    value["initial"]["d0"] = json!(0x12340002);
    value["final"]["d0"] = json!(0x12340002);
    value["final"]["sr"] = json!(0x2004);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2000, 0x1234);
        word(&mut value, state, 0x2002, 2);
    }
    value["reference_total_cycles"] = json!(14);
    value["transactions"] = json!([transfer("read",0x2000,0x1234,5),
        transfer("read",0x2002,2,5), transfer("read",0x1004,0x5678,6),
        {"kind":"idle","duration_cycles":2}]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().swap(2, 3);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["final"]["ram"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()[1] = json!(3);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn word_address_arithmetic_accepts_split_tail_without_moving_fetch() {
    for (opcode, cohort, result) in [(0xd0d0, "ADDA.w", 0x2002), (0x90d0, "SUBA.w", 0x1ffe)] {
        let mut value = case(opcode, cohort);
        value["final"]["a0"] = json!(result);
        for state in ["initial", "final"] {
            word(&mut value, state, 0x2000, 2);
        }
        value["reference_total_cycles"] = json!(12);
        value["transactions"] = json!([
            transfer("read", 0x2000, 2, 5),
            transfer("read", 0x1004, 0x5678, 6),
            {"kind":"idle","duration_cycles":4}
        ]);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        value["transactions"][2]["duration_cycles"] = json!(2);
        value["transactions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"idle","duration_cycles":2}));
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        let mut bad = value.clone();
        bad["transactions"].as_array_mut().unwrap().swap(1, 2);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["reference_total_cycles"] = json!(14);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn pc_relative_binary_data_requires_program_function_code() {
    let mut value = case(0xd07a, "ADD.w");
    value["initial"]["prefetch"][1] = json!(0x0ffe);
    for state in ["initial", "final"] {
        value[state]["ram"][2][1] = json!(0x0f);
        value[state]["ram"][3][1] = json!(0xfe);
        word(&mut value, state, 0x2000, 3);
        word(&mut value, state, 0x1006, 0x9abc);
    }
    value["final"]["d0"] = json!(3);
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 6),
        transfer("read", 0x2000, 3, 6),
        transfer("read", 0x1006, 0x9abc, 6)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for field in ["function_code", "data_bus"] {
        let mut bad = value.clone();
        bad["transactions"][1][field] = json!(5);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
}

#[test]
fn binary_metadata_rejects_family_size_and_special_alias_contradictions() {
    for (opcode, cohort) in [
        (0x9050, "ADD.w"),
        (0xd090, "ADD.w"),
        (0xd1d0, "ADDA.w"),
        (0xc100, "AND.b"),
        (0x8100, "OR.b"),
        (0xb108, "EOR.b"),
        (0x5150, "ADD.w"),
    ] {
        assert!(
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, cohort)))).is_err(),
            "{opcode:04x} {cohort}"
        );
    }
}

#[test]
fn invalid_binary_operand_forms_are_named_before_runtime() {
    for (opcode, cohort, reason) in [
        (0xd07d, "ADD.w", "binary_invalid_source_form"),
        (0xd17a, "ADD.w", "binary_invalid_destination_form"),
    ] {
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, cohort)))).unwrap();
        assert_eq!(report.ledger[cohort].unsupported_routes[reason], 1);
        assert_eq!(report.ledger[cohort].passed, 0);
        assert_eq!(report.failures, 0);
    }
}
