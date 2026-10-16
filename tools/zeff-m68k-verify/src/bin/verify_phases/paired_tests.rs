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
    value[state]["ram"].as_array_mut().unwrap().extend([
        json!([address, data >> 8]),
        json!([address + 1, data & 255]),
    ]);
}

fn transfer(kind: &str, address: u32, data: u16, byte: bool) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":if address==0x1004 {6}else{5},
        "address_bus":address,"data_bus":data,"uds":1,"lds":if byte {0}else{1}})
}

#[test]
fn abcd_same_a7_decrements_twice_and_preserves_byte_neighbors() {
    let mut value = case(0xcf0f, "ABCD");
    value["initial"]["ssp"] = json!(0x2004);
    value["final"]["ssp"] = json!(0x2000);
    value["initial"]["sr"] = json!(0x2004);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2002, 0x09aa);
    }
    word(&mut value, "initial", 0x2000, 0x01bb);
    word(&mut value, "final", 0x2000, 0x10bb);
    value["reference_total_cycles"] = json!(18);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
        transfer("read",0x2002,0x0900,true),transfer("read",0x2000,0x0100,true),
        transfer("read",0x1004,0x5678,false),transfer("write",0x2000,0x1000,true)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["final"]["ssp"] = json!(0x2002);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn addx_long_alias_reads_low_first_and_writes_low_before_prefetch() {
    let mut value = case(0xd188, "ADDX.l");
    value["initial"]["a0"] = json!(0x2008);
    value["final"]["a0"] = json!(0x2000);
    value["initial"]["sr"] = json!(0x2014);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2004, 0);
        word(&mut value, state, 0x2006, 1);
        word(&mut value, state, 0x2000, 0);
    }
    word(&mut value, "initial", 0x2002, 2);
    word(&mut value, "final", 0x2002, 4);
    value["reference_total_cycles"] = json!(30);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
        transfer("read",0x2006,1,false),transfer("read",0x2004,0,false),
        transfer("read",0x2002,2,false),transfer("read",0x2000,0,false),
        transfer("write",0x2002,4,false),transfer("read",0x1004,0x5678,false),
        transfer("write",0x2000,0,false)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    for (a, b) in [(1, 2), (3, 4), (5, 6), (5, 7)] {
        let mut bad = value.clone();
        bad["transactions"].as_array_mut().unwrap().swap(a, b);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x2006));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn cmpm_long_alias_increments_twice_without_memory_writes() {
    let mut value = case(0xb188, "CMP.l");
    value["initial"]["a0"] = json!(0x2000);
    value["final"]["a0"] = json!(0x2008);
    value["initial"]["sr"] = json!(0x2010);
    value["final"]["sr"] = json!(0x2014);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2000, 0x1234);
        word(&mut value, state, 0x2002, 2);
        word(&mut value, state, 0x2004, 0x1234);
        word(&mut value, state, 0x2006, 2);
    }
    value["reference_total_cycles"] = json!(20);
    value["transactions"] = json!([
        transfer("read", 0x2000, 0x1234, false),
        transfer("read", 0x2002, 2, false),
        transfer("read", 0x2004, 0x1234, false),
        transfer("read", 0x2006, 2, false),
        transfer("read", 0x1004, 0x5678, false)
    ]);
    let report = verify_reader(std::io::Cursor::new(cohort_stream(value.clone()))).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.ledger["CMP.l"].passed, 1);
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["final"]["a0"] = json!(0x2004);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn nbcd_postincrement_writes_saved_byte_address_and_keeps_sticky_zero() {
    let mut value = case(0x4818, "NBCD");
    value["initial"]["a0"] = json!(0x2000);
    value["final"]["a0"] = json!(0x2001);
    value["initial"]["sr"] = json!(0x2004);
    value["final"]["sr"] = json!(0x2004);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2000, 0x00aa);
    }
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x2000, 0, true),
        transfer("read", 0x1004, 0x5678, false),
        transfer("write", 0x2000, 0, true)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"][2]["address_bus"] = json!(0x2002);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["initial"]["sr"] = json!(0x2000);
    value["final"]["sr"] = json!(0x2000);
    assert!(run(&serde_json::from_value(value).unwrap()).is_ok());
}

#[test]
fn paired_family_size_metadata_and_nbcd_eas_are_strict() {
    for (opcode, cohort) in [
        (0x8108, "ABCD"),
        (0xd148, "SUBX.w"),
        (0xd188, "ADDX.w"),
        (0xd050, "ADDX.w"),
        (0x4840, "NBCD"),
    ] {
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, cohort)))).is_err());
    }
    for opcode in [0x4808, 0x483a, 0x483c] {
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(opcode, "NBCD")))).unwrap();
        assert_eq!(
            report.ledger["NBCD"].unsupported_routes["bcd_invalid_operand_form"],
            1
        );
        assert_eq!(report.failures, 0);
    }
}
#[test]
fn subtract_pairs_include_extend_and_clear_sticky_zero_on_nonzero_result() {
    for (opcode, cohort, source, destination, output, final_sr, byte) in [
        (0x8109, "SBCD", 1, 9, 7, 0x2000, true),
        (0x9149, "SUBX.w", 1, 0, 0xfffe, 0x2019, false),
    ] {
        let mut value = case(opcode, cohort);
        value["initial"]["a1"] = json!(if byte { 0x2001 } else { 0x2002 });
        value["final"]["a1"] = json!(0x2000);
        value["initial"]["a0"] = json!(if byte { 0x3001 } else { 0x3002 });
        value["final"]["a0"] = json!(0x3000);
        value["initial"]["sr"] = json!(0x2014);
        value["final"]["sr"] = json!(final_sr);
        let source_bus = if byte { source << 8 } else { source };
        let destination_bus = if byte { destination << 8 } else { destination };
        let output_bus = if byte { output << 8 } else { output };
        for state in ["initial", "final"] {
            word(&mut value, state, 0x2000, source_bus);
        }
        word(&mut value, "initial", 0x3000, destination_bus);
        word(&mut value, "final", 0x3000, output_bus);
        value["reference_total_cycles"] = json!(18);
        value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
            transfer("read",0x2000,source_bus,byte),transfer("read",0x3000,destination_bus,byte),
            transfer("read",0x1004,0x5678,false),transfer("write",0x3000,output_bus,byte)]);
        assert!(
            run(&serde_json::from_value(value.clone()).unwrap()).is_ok(),
            "{cohort}"
        );
        value["final"]["sr"] = json!(0x2014);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}
