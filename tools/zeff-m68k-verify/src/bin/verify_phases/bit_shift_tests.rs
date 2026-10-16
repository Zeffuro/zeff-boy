use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

fn case(opcode: u16, next: u16, cohort: &str) -> Value {
    let mut value = nop();
    value["cohort"] = json!(cohort);
    value["initial"]["prefetch"] = json!([opcode, next]);
    for state in ["initial", "final"] {
        value[state]["ram"][0][1] = json!(opcode >> 8);
        value[state]["ram"][1][1] = json!(opcode & 255);
        value[state]["ram"][2][1] = json!(next >> 8);
        value[state]["ram"][3][1] = json!(next & 255);
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

fn transfer(kind: &str, address: u32, data: u16, fc: u32, lower: bool) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,"address_bus":address,
        "data_bus":data,"uds":1,"lds":if lower {1} else {0}})
}

fn extended(value: &mut Value) {
    for state in ["initial", "final"] {
        word(value, state, 0x1006, 0x9abc);
    }
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
}

#[test]
fn static_bit_rmw_consumes_bit_word_and_preserves_inactive_lane() {
    let mut value = case(0x0890, 0, "BCLR");
    extended(&mut value);
    word(&mut value, "initial", 0x2000, 0x01aa);
    word(&mut value, "final", 0x2000, 0x00aa);
    value["reference_total_cycles"] = json!(16);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 6, true),
        transfer("read", 0x2000, 0x0100, 5, false),
        transfer("read", 0x1006, 0x9abc, 6, true),
        transfer("write", 0x2000, 0, 5, false)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (a, b) in [(0, 1), (2, 3)] {
        let mut bad = value.clone();
        bad["transactions"].as_array_mut().unwrap().swap(a, b);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    value["final"]["ram"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()[1] = json!(0);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn dynamic_bit_register_alias_and_high_bit_idle_are_exact() {
    let mut value = case(0x0140, 0x1234, "BCHG");
    value["initial"]["d0"] = json!(16);
    value["final"]["d0"] = json!(0x10010);
    value["initial"]["sr"] = json!(0x2010);
    value["final"]["sr"] = json!(0x2014);
    value["reference_total_cycles"] = json!(8);
    value["transactions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"idle","duration_cycles":4}));
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"][1]["duration_cycles"] = json!(2);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn dynamic_btst_immediate_and_pc_data_have_distinct_bus_routes() {
    let mut immediate = case(0x013c, 1, "BTST");
    extended(&mut immediate);
    immediate["reference_total_cycles"] = json!(10);
    immediate["transactions"] = json!([transfer("read",0x1004,0x5678,6,true),
        transfer("read",0x1006,0x9abc,6,true),{"kind":"idle","duration_cycles":2}]);
    assert!(run(&serde_json::from_value(immediate).unwrap()).is_ok());
    let mut pc = case(0x013a, 0x0ffe, "BTST");
    extended(&mut pc);
    for state in ["initial", "final"] {
        word(&mut pc, state, 0x2000, 0x0100);
    }
    pc["reference_total_cycles"] = json!(12);
    pc["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 6, true),
        transfer("read", 0x2000, 0x0100, 6, false),
        transfer("read", 0x1006, 0x9abc, 6, true)
    ]);
    assert!(run(&serde_json::from_value(pc.clone()).unwrap()).is_ok());
    pc["transactions"][1]["function_code"] = json!(5);
    assert!(run(&serde_json::from_value(pc).unwrap()).is_err());
}

#[test]
fn memory_shift_is_word_rmw_and_preserves_extend_semantics() {
    let mut value = case(0xe7d0, 0x1234, "ROL.w");
    word(&mut value, "initial", 0x2000, 0x8001);
    word(&mut value, "final", 0x2000, 3);
    value["initial"]["sr"] = json!(0x2010);
    value["final"]["sr"] = json!(0x2011);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x2000, 0x8001, 5, true),
        transfer("read", 0x1004, 0x5678, 6, true),
        transfer("write", 0x2000, 3, 5, true)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"][0]["lds"] = json!(0);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().swap(1, 2);
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
fn bit_shift_metadata_and_nonalterable_forms_are_explicit() {
    for (opcode, cohort) in [
        (0x0890, "BSET"),
        (0x0140, "BTST"),
        (0xe0d0, "ASL.w"),
        (0xe7d0, "ROR.w"),
        (0xe180, "ASL.w"),
    ] {
        assert!(
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, 0, cohort)))).is_err()
        );
    }
    for (opcode, cohort, reason) in [
        (0x083c, "BTST", "bit_invalid_operand_form"),
        (0x0848, "BCHG", "bit_invalid_operand_form"),
        (0x01fa, "BSET", "bit_invalid_operand_form"),
        (0xe1fa, "ASL.w", "shift_invalid_operand_form"),
    ] {
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, 0, cohort)))).unwrap();
        assert_eq!(report.ledger[cohort].unsupported_routes[reason], 1);
        assert_eq!(report.failures, 0);
    }
}
