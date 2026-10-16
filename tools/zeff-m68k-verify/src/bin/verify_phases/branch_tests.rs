use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

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

fn case(cohort: &str, opcode: u16) -> Value {
    let mut value = nop();
    value["cohort"] = json!(cohort);
    value["initial"]["prefetch"][0] = json!(opcode);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1000, opcode);
        word(&mut value, state, 0x1006, 0x9abc);
        word(&mut value, state, 0x1100, 0xaaaa);
        word(&mut value, state, 0x1102, 0xbbbb);
    }
    value
}

fn transfer(kind: &str, address: u32, data: u16, fc: u32) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":data,"uds":1,"lds":1})
}

fn final_queue(value: &mut Value, pc: u32, words: [u16; 2]) {
    value["final"]["pc"] = json!(pc + 4);
    value["final"]["prefetch"] = json!(words);
}

fn push(value: &mut Value, data: u32, supervisor: bool) {
    let field = if supervisor { "ssp" } else { "usp" };
    value["initial"][field] = json!(0x2000);
    value["final"][field] = json!(0x1ffc);
    for state in ["initial", "final"] {
        value[state]["sr"] = json!(if supervisor { 0x2000 } else { 0 });
    }
    word(value, "final", 0x1ffc, (data >> 16) as u16);
    word(value, "final", 0x1ffe, data as u16);
}

#[test]
fn bsr_byte_word_bank_and_stack_order_are_exact() {
    for supervisor in [false, true] {
        for long in [false, true] {
            let mut value = case("BSR", if long { 0x6100 } else { 0x6102 });
            let pc = if long { 0x1100 } else { 0x1004 };
            let words = if long {
                [0xaaaa, 0xbbbb]
            } else {
                [0x5678, 0x9abc]
            };
            let following = if long { 0x1004 } else { 0x1002 };
            if long {
                value["initial"]["prefetch"][1] = json!(0xfe);
                for state in ["initial", "final"] {
                    word(&mut value, state, 0x1002, 0xfe);
                }
            }
            push(&mut value, following, supervisor);
            final_queue(&mut value, pc, words);
            let dfc = if supervisor { 5 } else { 1 };
            let pfc = if supervisor { 6 } else { 2 };
            value["reference_total_cycles"] = json!(18);
            value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
                transfer("write",0x1ffc,0,dfc),transfer("write",0x1ffe,following as u16,dfc),
                transfer("read",pc,words[0],pfc),transfer("read",pc+2,words[1],pfc)]);
            assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
            for (field, data) in [
                ("address_bus", 0x1ffa),
                ("data_bus", 1),
                ("function_code", 3),
                ("lds", 0),
            ] {
                let mut bad = value.clone();
                bad["transactions"][1][field] = json!(data);
                assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
            }
            value["transactions"].as_array_mut().unwrap().swap(1, 2);
            assert!(run(&serde_json::from_value(value).unwrap()).is_err());
        }
    }
}

#[test]
fn jsr_fetches_target_before_stack_and_rejects_fabricated_target() {
    let mut value = case("JSR", 0x4e90);
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x1100);
    }
    push(&mut value, 0x1002, true);
    final_queue(&mut value, 0x1100, [0xaaaa, 0xbbbb]);
    value["reference_total_cycles"] = json!(16);
    value["transactions"] = json!([
        transfer("read", 0x1100, 0xaaaa, 6),
        transfer("write", 0x1ffc, 0, 5),
        transfer("write", 0x1ffe, 0x1002, 5),
        transfer("read", 0x1102, 0xbbbb, 6)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["final"]["pc"] = json!(0x1108);
    assert!(
        run(&serde_json::from_value(bad).unwrap())
            .unwrap_err()
            .contains("target")
    );
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x1100));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved")
    );
}

#[test]
fn pea_pushes_address_without_operand_read() {
    let mut value = case("PEA", 0x4850);
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x1100);
    }
    push(&mut value, 0x1100, true);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 6),
        transfer("write", 0x1ffc, 0, 5),
        transfer("write", 0x1ffe, 0x1100, 5)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["transactions"][2]["data_bus"] = json!(0xaaaa);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn jmp_absolute_long_extension_is_initial_evidence() {
    let mut value = case("JMP", 0x4ef9);
    value["initial"]["prefetch"][1] = json!(0);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0);
        word(&mut value, state, 0x1004, 0x1100);
    }
    final_queue(&mut value, 0x1100, [0xaaaa, 0xbbbb]);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x1100, 6),
        transfer("read", 0x1100, 0xaaaa, 6),
        transfer("read", 0x1102, 0xbbbb, 6)
    ]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x[0] != json!(0x1004));
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("unobserved")
    );
}

#[test]
fn lea_index_keeps_idle_on_both_sides_of_extension_fetch() {
    let mut value = case("LEA", 0x45f0);
    value["initial"]["prefetch"][1] = json!(0x10fe);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0x10fe);
        value[state]["a0"] = json!(0x1100);
        value[state]["d1"] = json!(4);
    }
    value["final"]["a2"] = json!(0x1102);
    final_queue(&mut value, 0x1004, [0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":2},transfer("read",0x1004,0x5678,6),
        {"kind":"idle","duration_cycles":2},transfer("read",0x1006,0x9abc,6)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["final"]["a2"] = json!(0x1100);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn dbcc_count_metadata_and_control_aliases_fail_closed() {
    let mut value = case("DBcc", 0x51c8);
    value["initial"]["prefetch"][1] = json!(0xfe);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 0xfe);
    }
    value["initial"]["d0"] = json!(0xabcd0001_u32);
    value["final"]["d0"] = json!(0xabcd0000_u32);
    final_queue(&mut value, 0x1100, [0xaaaa, 0xbbbb]);
    value["reference_total_cycles"] = json!(10);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
        transfer("read",0x1100,0xaaaa,6),transfer("read",0x1102,0xbbbb,6)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["final"]["d0"] = json!(0xabcd0001_u32);
    assert!(
        run(&serde_json::from_value(value).unwrap())
            .unwrap_err()
            .contains("count")
    );
    for (cohort, opcode) in [
        ("Bcc", 0x6102),
        ("BSR", 0x6002),
        ("JSR", 0x4ed0),
        ("LEA", 0x4850),
        ("Scc", 0x51c8),
    ] {
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case(cohort, opcode)))).is_err());
    }
    for cohort in ["JMP", "JSR", "LEA", "PEA"] {
        let opcode = match cohort {
            "JMP" => 0x4ec0,
            "JSR" => 0x4e80,
            "LEA" => 0x41c0,
            _ => 0x4840,
        };
        let report =
            verify_reader(std::io::Cursor::new(cohort_stream(case(cohort, opcode)))).unwrap();
        assert_eq!(
            report.ledger[cohort].unsupported_routes["control_invalid_address_form"],
            1
        );
    }
}

#[test]
fn dbcc_expiry_reads_tentative_target_then_sequential_queue() {
    let mut value = case("DBcc", 0x51c8);
    value["initial"]["prefetch"][1] = json!(0xfe);
    for state in ["initial", "final"] {
        value[state]["sr"] = json!(0xa000);
        word(&mut value, state, 0x1002, 0xfe);
    }
    value["normalization_exclusion"] = json!("trace");
    value["initial"]["d0"] = json!(0xabcd0000_u32);
    value["final"]["d0"] = json!(0xabcdffff_u32);
    final_queue(&mut value, 0x1004, [0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(14);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":2},
        transfer("read",0x1100,0xaaaa,6),transfer("read",0x1004,0x5678,6),
        transfer("read",0x1006,0x9abc,6)]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().remove(1);
    bad["reference_total_cycles"] = json!(10);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

#[test]
fn scc_memory_reads_old_byte_and_preserves_inactive_lane() {
    let mut value = case("Scc", 0x56d0);
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x1100);
        word(&mut value, state, 0x1100, 0x1234);
    }
    word(&mut value, "final", 0x1100, 0xff34);
    let mut read = transfer("read", 0x1100, 0x1200, 5);
    read["lds"] = json!(0);
    let mut write = transfer("write", 0x1100, 0xff00, 5);
    write["lds"] = json!(0);
    value["reference_total_cycles"] = json!(12);
    value["transactions"] = json!([read, transfer("read", 0x1004, 0x5678, 6), write]);
    assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    let mut bad = value.clone();
    bad["transactions"].as_array_mut().unwrap().remove(0);
    bad["reference_total_cycles"] = json!(8);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = value.clone();
    word(&mut bad, "final", 0x1100, 0xff00);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    value["transactions"][2]["function_code"] = json!(6);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}
