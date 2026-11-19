use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

fn case(opcode: u16, immediate: u16, cohort: &str) -> Value {
    let mut value = nop();
    value["cohort"] = json!(cohort);
    value["initial"]["prefetch"] = json!([opcode, immediate]);
    for state in ["initial", "final"] {
        value[state]["ram"][0][1] = json!(opcode >> 8);
        value[state]["ram"][1][1] = json!(opcode & 255);
        value[state]["ram"][2][1] = json!(immediate >> 8);
        value[state]["ram"][3][1] = json!(immediate & 255);
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
fn immediate_memory_uses_extension_then_read_prefetch_write() {
    let mut value = case(0x0650, 3, "ADD.w");
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x2000);
        word(&mut value, state, 0x1006, 0x9abc);
    }
    word(&mut value, "initial", 0x2000, 5);
    word(&mut value, "final", 0x2000, 8);
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(16);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 6),
        transfer("read", 0x2000, 5, 5),
        transfer("read", 0x1006, 0x9abc, 6),
        transfer("write", 0x2000, 8, 5)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (a, b) in [(0, 1), (2, 3)] {
        let mut bad = value.clone();
        bad["transactions"].as_array_mut().unwrap().swap(a, b);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
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
fn long_cmpi_uses_two_immediate_words_and_two_internal_clocks() {
    let mut value = case(0x0c80, 0, "CMP.l");
    for state in ["initial", "final"] {
        value[state]["ram"][4][1] = json!(0);
        value[state]["ram"][5][1] = json!(1);
        value[state]["d0"] = json!(1);
        word(&mut value, state, 0x1006, 0x5678);
        word(&mut value, state, 0x1008, 0x9abc);
    }
    value["final"]["pc"] = json!(0x100a);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["final"]["sr"] = json!(0x2004);
    value["reference_total_cycles"] = json!(14);
    value["transactions"] = json!([transfer("read",0x1004,1,6),transfer("read",0x1006,0x5678,6),
        transfer("read",0x1008,0x9abc,6),{"kind":"idle","duration_cycles":2}]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"][3]["duration_cycles"] = json!(4);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn ccr_masks_only_low_flags_and_repeats_instruction_refill() {
    let mut value = case(0x023c, 0x001e, "ANDItoCCR");
    value["initial"]["sr"] = json!(0xa01f);
    value["final"]["sr"] = json!(0xa01e);
    value["normalization_exclusion"] = json!("trace");
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1006, 0x9abc);
    }
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(20);
    value["transactions"] = json!([transfer("read",0x1004,0x5678,6),{"kind":"idle","duration_cycles":8},
        transfer("read",0x1004,0x5678,6),transfer("read",0x1006,0x9abc,6)]);
    let report = verify_reader(std::io::Cursor::new(cohort_stream(value.clone()))).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.ledger["ANDItoCCR"].trace_prefix_pass, 1);
    let mut bad = value.clone();
    bad["transactions"][2]["address_bus"] = json!(0x1006);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["final"]["sr"] = json!(0x001e);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn user_sr_privilege_has_known_vector_eight_and_no_pending_trace() {
    let mut value = case(0x007c, 0xffff, "ORItoSR");
    value["initial"]["sr"] = json!(0x8000);
    value["final"]["sr"] = json!(0x2000);
    value["initial"]["ssp"] = json!(0x2000);
    value["final"]["ssp"] = json!(0x1ffa);
    value["normalization_exclusion"] = json!("trace");
    for state in ["initial", "final"] {
        word(&mut value, state, 32, 0);
        word(&mut value, state, 34, 0x3000);
        word(&mut value, state, 0x3000, 0xabcd);
        word(&mut value, state, 0x3002, 0xef01);
    }
    word(&mut value, "final", 0x1ffe, 0x1000);
    word(&mut value, "final", 0x1ffa, 0x8000);
    word(&mut value, "final", 0x1ffc, 0);
    value["final"]["pc"] = json!(0x3004);
    value["final"]["prefetch"] = json!([0xabcd, 0xef01]);
    value["reference_total_cycles"] = json!(34);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":4},
        transfer("write",0x1ffe,0x1000,5),transfer("write",0x1ffa,0x8000,5),
        transfer("write",0x1ffc,0,5),transfer("read",32,0,5),transfer("read",34,0x3000,5),
        transfer("read",0x3000,0xabcd,6),{"kind":"idle","duration_cycles":2},
        transfer("read",0x3002,0xef01,6)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (index, field, data) in [
        (4, "address_bus", 36),
        (1, "function_code", 1),
        (2, "data_bus", 0),
    ] {
        let mut bad = value.clone();
        bad["transactions"][index][field] = json!(data);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(32));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn immediate_metadata_and_invalid_eas_fail_closed() {
    for (opcode, cohort) in [
        (0x003c, "OR.b"),
        (0x0a7c, "EOR.w"),
        (0x003c, "ORItoSR"),
        (0x007c, "ANDItoSR"),
        (0x0a7c, "EORItoCCR"),
        (0x0640, "SUB.w"),
        (0x0680, "ADD.w"),
    ] {
        assert!(
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, 0, cohort)))).is_err()
        );
    }
    for opcode in [0x0648, 0x067a, 0x067c] {
        let report = verify_reader(std::io::Cursor::new(cohort_stream(case(
            opcode, 0, "ADD.w",
        ))))
        .unwrap();
        assert_eq!(
            report.ledger["ADD.w"].unsupported_routes["immediate_invalid_operand_form"],
            1
        );
        assert_eq!(report.failures, 0);
    }
}
