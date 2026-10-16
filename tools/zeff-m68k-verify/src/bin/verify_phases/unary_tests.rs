use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

fn unary_case(opcode: u16, cohort: &str) -> Value {
    let mut case = nop();
    case["cohort"] = json!(cohort);
    case["initial"]["prefetch"][0] = json!(opcode);
    for state in ["initial", "final"] {
        case[state]["ram"][0][1] = json!(opcode >> 8);
        case[state]["ram"][1][1] = json!(opcode & 255);
        case[state]["a0"] = json!(0x2000);
    }
    case
}

fn word(case: &mut Value, state: &str, address: u32, value: u16) {
    case[state]["ram"].as_array_mut().unwrap().extend([
        json!([address, value >> 8]),
        json!([address + 1, value & 255]),
    ]);
}

fn transfer(kind: &str, address: u32, value: u16, fc: u32) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":value,"uds":1,"lds":1})
}

#[test]
fn clr_requires_operand_read_then_prefetch_then_write() {
    let mut case = unary_case(0x4250, "CLR.w");
    word(&mut case, "initial", 0x2000, 0x1234);
    word(&mut case, "final", 0x2000, 0);
    case["final"]["sr"] = json!(0x2004);
    case["reference_total_cycles"] = json!(12);
    case["transactions"] = json!([
        transfer("read", 0x2000, 0x1234, 5),
        transfer("read", 0x1004, 0x5678, 6),
        transfer("write", 0x2000, 0, 5)
    ]);
    assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
    let mut bad = case.clone();
    bad["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = case.clone();
    bad["transactions"].as_array_mut().unwrap().remove(0);
    bad["reference_total_cycles"] = json!(8);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    case["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|byte| byte[0] != json!(0x2000));
    assert!(
        run(&serde_json::from_value(case).unwrap())
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn long_predecrement_reads_high_first_and_writes_low_first() {
    for (opcode, cohort, input, output, initial_sr, final_sr) in [
        (0x42a0, "CLR.l", 0x11223344_u32, 0_u32, 0x2010, 0x2014),
        (0x40a0, "NEGX.l", 1_u32, 0xfffffffe_u32, 0x2014, 0x2019),
    ] {
        let mut case = unary_case(opcode, cohort);
        case["initial"]["a0"] = json!(0x2004);
        case["initial"]["sr"] = json!(initial_sr);
        case["final"]["sr"] = json!(final_sr);
        word(&mut case, "initial", 0x2000, (input >> 16) as u16);
        word(&mut case, "initial", 0x2002, input as u16);
        word(&mut case, "final", 0x2000, (output >> 16) as u16);
        word(&mut case, "final", 0x2002, output as u16);
        case["reference_total_cycles"] = json!(22);
        case["transactions"] = json!([{"kind":"idle","duration_cycles":2},
            transfer("read",0x2000,(input>>16) as u16,5),transfer("read",0x2002,input as u16,5),
            transfer("read",0x1004,0x5678,6),transfer("write",0x2002,output as u16,5),
            transfer("write",0x2000,(output>>16) as u16,5)]);
        assert!(
            run(&serde_json::from_value(case.clone()).unwrap()).is_ok(),
            "{cohort}"
        );
        case["transactions"].as_array_mut().unwrap().swap(4, 5);
        assert!(run(&serde_json::from_value(case).unwrap()).is_err());
    }
}

#[test]
fn ordinary_low_supervisor_tst_is_executed_despite_normalizer_tag() {
    let mut case = unary_case(0x4a10, "TST.b");
    for state in ["initial", "final"] {
        case[state]["a0"] = json!(0x200);
        word(&mut case, state, 0x200, 0x7f55);
    }
    case["reference_total_cycles"] = json!(8);
    case["transactions"] = json!([{"kind":"read","duration_cycles":4,"function_code":5,
        "address_bus":0x200,"data_bus":0x7f00,"uds":1,"lds":0},transfer("read",0x1004,0x5678,6)]);
    case["normalization_exclusion"] = json!("exception_or_low_supervisor_data");
    let report = verify_reader(std::io::Cursor::new(cohort_stream(case))).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.ledger["TST.b"].passed, 1);
    assert_eq!(
        report.ledger["TST.b"].excluded["exception_or_low_supervisor_data"],
        1
    );
}

#[test]
fn unary_metadata_and_nonalterable_routes_are_explicit() {
    for (opcode, cohort) in [(0x4a50, "CLR.w"), (0x4250, "CLR.b")] {
        let case = unary_case(opcode, cohort);
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case))).is_err());
    }
    for opcode in [0x4a48, 0x4a7a, 0x4a7c] {
        let case = unary_case(opcode, "TST.w");
        let report = verify_reader(std::io::Cursor::new(cohort_stream(case))).unwrap();
        assert_eq!(
            report.ledger["TST.w"].unsupported_routes["unary_nonalterable_operand_form"],
            1
        );
        assert_eq!(report.ledger["TST.w"].passed, 0);
    }
}
