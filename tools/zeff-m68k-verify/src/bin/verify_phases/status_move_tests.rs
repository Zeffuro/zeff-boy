use super::cohort_stream;
use super::stack_status_tests::{case, transfer, word};
use crate::*;
use serde_json::json;

#[test]
fn move_to_sr_latches_entry_trace_and_uses_new_program_bank() {
    for entry_t in [false, true] {
        for new_t in [false, true] {
            for new_s in [false, true] {
                let mut value = case("MOVEtoSR", 0x46c0);
                let source =
                    (if new_t { 0x8000 } else { 0 }) | (if new_s { 0x2000 } else { 0 }) | 4;
                let entry = if entry_t { 0xa010 } else { 0x2010 };
                for state in ["initial", "final"] {
                    value[state]["d0"] = json!(0xabcd0000_u32 | source);
                    value[state]["usp"] = json!(0x3000);
                    value[state]["ssp"] = json!(0x2000);
                }
                value["initial"]["sr"] = json!(entry);
                value["final"]["sr"] = json!(source);
                value["normalization_exclusion"] = if entry_t {
                    json!("trace")
                } else {
                    serde_json::Value::Null
                };
                value["reference_total_cycles"] = json!(12);
                value["transactions"] = json!([{"kind":"idle","duration_cycles":4},
                    transfer("read",0x1002,0x1234,if new_s {6} else {2}),transfer("read",0x1004,0x5678,if new_s {6} else {2})]);
                assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
                value["transactions"][1]["function_code"] = json!(if new_s { 2 } else { 6 });
                assert!(run(&serde_json::from_value(value).unwrap()).is_err());
            }
        }
    }
}

#[test]
fn move_to_sr_immediate_prefix_uses_entry_fc_and_tail_refetches_new_fc() {
    let mut value = case("MOVEtoSR", 0x46fc);
    value["initial"]["sr"] = json!(0xa010);
    value["final"]["sr"] = json!(4);
    value["initial"]["prefetch"][1] = json!(4);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 4);
    }
    value["normalization_exclusion"] = json!("trace");
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(16);
    value["transactions"] = json!([transfer("read",0x1004,0x5678,6),{"kind":"idle","duration_cycles":4},
        transfer("read",0x1004,0x5678,2),transfer("read",0x1006,0x9abc,2)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"][0]["function_code"] = json!(2);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn move_to_sr_a7_operand_side_effect_stays_in_entry_bank() {
    for (opcode, address, final_ssp) in [(0x46df, 0x2000, 0x2002), (0x46e7, 0x1ffe, 0x1ffe)] {
        let mut value = case("MOVEtoSR", opcode);
        for state in ["initial", "final"] {
            value[state]["usp"] = json!(0x3000);
            word(&mut value, state, address, 4);
        }
        value["initial"]["ssp"] = json!(0x2000);
        value["final"]["ssp"] = json!(final_ssp);
        value["final"]["sr"] = json!(4);
        let mut phases = vec![];
        if opcode == 0x46e7 {
            phases.push(json!({"kind":"idle","duration_cycles":2}));
        }
        phases.extend([
            transfer("read", address, 4, 5),
            json!({"kind":"idle","duration_cycles":4}),
            transfer("read", 0x1002, 0x1234, 2),
            transfer("read", 0x1004, 0x5678, 2),
        ]);
        value["reference_total_cycles"] = json!(if opcode == 0x46e7 { 18 } else { 16 });
        value["transactions"] = json!(phases);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        let mut bad = value.clone();
        bad["final"]["usp"] = json!(final_ssp);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["transactions"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["function_code"] = json!(6);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn move_to_ccr_pc_operand_has_finite_program_data_permission() {
    let mut value = case("MOVEtoCCR", 0x44fa);
    value["initial"]["sr"] = json!(0x2010);
    value["final"]["sr"] = json!(0x2004);
    value["initial"]["prefetch"][1] = json!(0x10);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0x10);
        word(&mut value, state, 0x1012, 0xff04);
    }
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(20);
    value["transactions"] = json!([transfer("read",0x1004,0x5678,6),transfer("read",0x1012,0xff04,6),
        {"kind":"idle","duration_cycles":4},transfer("read",0x1004,0x5678,6),transfer("read",0x1006,0x9abc,6)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let case: Case = serde_json::from_value(value.clone()).unwrap();
    let contract = stack_status::derive(&case).unwrap().unwrap();
    let access = BusAccess {
        logical_address: 0x1012,
        bus_address: 0x1012,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorProgram,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    };
    assert!(contract.permits(access, false));
    let mut false_prefetch = access;
    false_prefetch.kind = AccessKind::Prefetch;
    assert!(!contract.permits(false_prefetch, false));
    value["transactions"][1]["function_code"] = json!(5);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn move_from_sr_old_word_read_is_mandatory_and_a7_postincrement_is_preserved() {
    let mut value = case("MOVEfromSR", 0x40df);
    for state in ["initial", "final"] {
        value[state]["sr"] = json!(0xa010);
        word(&mut value, state, 0x2000, 0xface);
    }
    value["normalization_exclusion"] = json!("trace");
    value["initial"]["ssp"] = json!(0x2000);
    value["final"]["ssp"] = json!(0x2002);
    word(&mut value, "final", 0x2000, 0xa010);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x2000, 0xface, 5),
        transfer("read", 0x1004, 0x5678, 6),
        transfer("write", 0x2000, 0xa010, 5)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().remove(0);
    bad["reference_total_cycles"] = json!(8);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["transactions"][2]["data_bus"] = json!(0x2010);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn status_family_and_invalid_ea_metadata_are_named_before_execution() {
    for (cohort, opcode) in [
        ("MOVEfromSR", 0x44c0),
        ("MOVEtoSR", 0x44c0),
        ("MOVEtoCCR", 0x46c0),
    ] {
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case(cohort, opcode)))).is_err());
    }
    for (cohort, opcode, reason) in [
        ("MOVEfromSR", 0x40c8, "status_invalid_destination_form"),
        ("MOVEtoSR", 0x46c8, "status_invalid_source_form"),
        ("MOVEtoCCR", 0x44fd, "status_invalid_source_form"),
    ] {
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(cohort, opcode)))).unwrap();
        assert_eq!(report.ledger[cohort].unsupported_routes[reason], 1);
    }
}
