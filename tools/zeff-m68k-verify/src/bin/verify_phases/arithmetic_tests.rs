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
    }
    value
}

fn word(value: &mut Value, state: &str, address: u32, data: u16) {
    let ram = value[state]["ram"].as_array_mut().unwrap();
    for (address, data) in [(address, data >> 8), (address + 1, data & 255)] {
        if let Some(byte) = ram.iter_mut().find(|x| x[0] == json!(address)) {
            byte[1] = json!(data);
        } else {
            ram.push(json!([address, data]));
        }
    }
}

fn transfer(kind: &str, address: u32, data: u16, fc: u32) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":data,"uds":1,"lds":1})
}

fn exception(value: &mut Value, vector: u8, stack: u32, saved_pc: u32, saved_sr: u16, idle: u32) {
    let handler = 0x4000;
    for state in ["initial", "final"] {
        word(value, state, u32::from(vector) * 4, 0);
        word(value, state, u32::from(vector) * 4 + 2, handler);
        word(value, state, u32::from(handler), 0x1357);
        word(value, state, u32::from(handler) + 2, 0x2468);
    }
    word(value, "final", stack - 2, saved_pc as u16);
    word(value, "final", stack - 6, saved_sr);
    word(value, "final", stack - 4, (saved_pc >> 16) as u16);
    value["final"]["ssp"] = json!(stack - 6);
    value["final"]["sr"] = json!((saved_sr | 0x2000) & !0x8000);
    value["final"]["pc"] = json!(u32::from(handler) + 4);
    value["final"]["prefetch"] = json!([0x1357, 0x2468]);
    value["reference_total_cycles"] = json!(idle + 30);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":idle},
        transfer("write",stack-2,saved_pc as u16,5),transfer("write",stack-6,saved_sr,5),
        transfer("write",stack-4,(saved_pc>>16) as u16,5),
        transfer("read",u32::from(vector)*4,0,5),transfer("read",u32::from(vector)*4+2,handler,5),
        transfer("read",u32::from(handler),0x1357,6),{"kind":"idle","duration_cycles":2},
        transfer("read",u32::from(handler)+2,0x2468,6)]);
}

#[test]
fn chk_fast_slow_and_normal_retain_entry_trace_request() {
    for traced in [false, true] {
        for (destination, bound, idle, flags) in [
            (1_i16, 0_i16, 8, 0x10),
            (-1, 0, 10, 0x18),
            (-1, -2, 8, 0x18),
            (-32768, -1, 10, 0x18),
            (-32768, 0, 8, 0x18),
        ] {
            let mut value = case(0x4181, "CHK");
            let sr = if traced { 0xa014 } else { 0x2014 };
            value["initial"]["sr"] = json!(sr);
            value["initial"]["ssp"] = json!(0x3000);
            for state in ["initial", "final"] {
                value[state]["d0"] = json!(0xabcd0000_u32 | u32::from(destination as u16));
                value[state]["d1"] = json!(u32::from(bound as u16));
            }
            value["normalization_exclusion"] = if traced { json!("trace") } else { Value::Null };
            exception(&mut value, 6, 0x3000, 0x1002, (sr & !31) | flags, idle);
            assert!(
                run(&serde_json::from_value(value.clone()).unwrap()).is_ok(),
                "{destination}/{bound}/{traced}"
            );
            let mut bad = value.clone();
            bad["transactions"][0]["duration_cycles"] = json!(4);
            assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
            value["transactions"][2]["data_bus"] = json!(sr);
            assert!(run(&serde_json::from_value(value).unwrap()).is_err());
        }
        let mut normal = case(0x4181, "CHK");
        let sr = if traced { 0xa014 } else { 0x2014 };
        normal["initial"]["sr"] = json!(sr);
        normal["final"]["sr"] = json!(sr);
        normal["normalization_exclusion"] = if traced { json!("trace") } else { Value::Null };
        normal["reference_total_cycles"] = json!(10);
        normal["transactions"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"kind":"idle","duration_cycles":6}));
        assert!(run(&serde_json::from_value(normal).unwrap()).is_ok());
    }
}

#[test]
fn divzero_dn_alias_uses_high_word_flags_and_keeps_pending_trace() {
    for (opcode, cohort, saved_sr) in [(0x82c1, "DIVU", 0x8018), (0x83c1, "DIVS", 0x8014)] {
        let mut value = case(opcode, cohort);
        value["initial"]["sr"] = json!(0x8014);
        value["initial"]["ssp"] = json!(0x3000);
        for state in ["initial", "final"] {
            value[state]["d1"] = json!(0x80000000_u32);
        }
        value["normalization_exclusion"] = json!("trace");
        exception(&mut value, 5, 0x3000, 0x1002, saved_sr, 8);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        let mut bad = value.clone();
        bad["transactions"][4]["address_bus"] = json!(24);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["transactions"].as_array_mut().unwrap().swap(1, 2);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn divzero_immediate_consumes_extension_and_stacks_following_cursor() {
    let mut value = case(0x81fc, "DIVS");
    value["initial"]["prefetch"][1] = json!(0);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0);
    }
    value["initial"]["sr"] = json!(0xa010);
    value["initial"]["ssp"] = json!(0x3000);
    value["normalization_exclusion"] = json!("trace");
    exception(&mut value, 5, 0x3000, 0x1004, 0xa014, 8);
    value["transactions"]
        .as_array_mut()
        .unwrap()
        .insert(0, transfer("read", 0x1004, 0x5678, 6));
    value["reference_total_cycles"] = json!(42);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn pc_relative_mul_uses_program_data_fc_and_initial_operand_evidence() {
    let mut value = case(0xc0fa, "MULU");
    value["initial"]["prefetch"][1] = json!(0x10);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0x10);
        word(&mut value, state, 0x1012, 1);
        word(&mut value, state, 0x1006, 0x9abc);
    }
    value["initial"]["d0"] = json!(1);
    value["final"]["d0"] = json!(1);
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(48);
    value["transactions"] = json!([
        transfer("read",0x1004,0x5678,6),transfer("read",0x1012,1,6),
        transfer("read",0x1006,0x9abc,6),{"kind":"idle","duration_cycles":36}]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (field, data) in [
        ("function_code", 5),
        ("data_bus", 2),
        ("address_bus", 0x1010),
    ] {
        let mut bad = value.clone();
        bad["transactions"][1][field] = json!(data);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x1012));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved")
    );
}

#[test]
fn a7_zero_operand_updates_stack_before_exception_writes() {
    let mut value = case(0x80df, "DIVU");
    value["initial"]["ssp"] = json!(0x2000);
    value["initial"]["sr"] = json!(0xa010);
    value["normalization_exclusion"] = json!("trace");
    word(&mut value, "initial", 0x2000, 0);
    exception(&mut value, 5, 0x2002, 0x1002, 0xa014, 8);
    value["transactions"]
        .as_array_mut()
        .unwrap()
        .insert(0, transfer("read", 0x2000, 0, 5));
    value["reference_total_cycles"] = json!(42);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["final"]["ssp"] = json!(0x1ffa);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x2000));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved")
    );
}

#[test]
fn variable_mul_div_are_write_free_with_opposite_fetch_placement() {
    for (opcode, cohort, input, output, sr, idle, late) in [
        (0xc0c1, "MULU", 1_u32, 1_u32, 0x2000, 36, false),
        (0x80c1, "DIVU", 0x10000, 0x10000, 0x200a, 6, true),
        (0x81c1, "DIVS", 0x10000, 0x10000, 0x200a, 12, true),
    ] {
        let mut value = case(opcode, cohort);
        for state in ["initial", "final"] {
            value[state]["d1"] = json!(1);
        }
        value["initial"]["d0"] = json!(input);
        value["final"]["d0"] = json!(output);
        value["final"]["sr"] = json!(sr);
        value["reference_total_cycles"] = json!(idle + 4);
        if late {
            value["transactions"]
                .as_array_mut()
                .unwrap()
                .insert(0, json!({"kind":"idle","duration_cycles":idle}));
        } else {
            value["transactions"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"idle","duration_cycles":idle}));
        }
        assert!(
            run(&serde_json::from_value(value.clone()).unwrap()).is_ok(),
            "{cohort}"
        );
        value["transactions"].as_array_mut().unwrap().swap(0, 1);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn arithmetic_family_invalid_ea_and_fabricated_frames_fail_closed() {
    for (opcode, cohort) in [(0xc0c1, "MULS"), (0x81c1, "DIVU"), (0x41c1, "CHK")] {
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, cohort)))).is_err());
    }
    for opcode in [0x80c8, 0x80fd] {
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, "DIVU")))).unwrap();
        assert_eq!(
            report.ledger["DIVU"].unsupported_routes["arithmetic_invalid_source_form"],
            1
        );
    }
    let mut normal = case(0xc0c1, "MULU");
    normal["transactions"] = json!([transfer("write", 0x2ffe, 0x1002, 5)]);
    assert!(run(&serde_json::from_value(normal).unwrap()).is_err());
    let mut trapped = case(0x4181, "CHK");
    trapped["initial"]["ssp"] = json!(0x3000);
    trapped["initial"]["d0"] = json!(1);
    trapped["final"]["d0"] = json!(1);
    exception(&mut trapped, 6, 0x3000, 0x1002, 0x2000, 8);
    trapped["final"]["prefetch"][1] = json!(0);
    assert!(run(&serde_json::from_value(trapped).unwrap()).is_err());
}
