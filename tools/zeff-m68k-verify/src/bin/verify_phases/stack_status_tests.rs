use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

pub(super) fn word(value: &mut Value, state: &str, address: u32, data: u16) {
    let ram = value[state]["ram"].as_array_mut().unwrap();
    for (address, data) in [(address, data >> 8), (address + 1, data & 255)] {
        if let Some(byte) = ram.iter_mut().find(|x| x[0] == json!(address)) {
            byte[1] = json!(data);
        } else {
            ram.push(json!([address, data]));
        }
    }
}

pub(super) fn case(cohort: &str, opcode: u16) -> Value {
    let mut value = nop();
    value["cohort"] = json!(cohort);
    value["initial"]["prefetch"][0] = json!(opcode);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1000, opcode);
        word(&mut value, state, 0x1006, 0x9abc);
    }
    value
}

pub(super) fn transfer(kind: &str, address: u32, data: u16, fc: u32) -> Value {
    json!({"kind":kind,"duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":data,"uds":1,"lds":1})
}

pub(super) fn exception(value: &mut Value, vector: u8, pc: u32, sr: u16) {
    value["initial"]["ssp"] = json!(0x3000);
    for state in ["initial", "final"] {
        word(value, state, u32::from(vector) * 4, 0);
        word(value, state, u32::from(vector) * 4 + 2, 0x4000);
        word(value, state, 0x4000, 0x1357);
        word(value, state, 0x4002, 0x2468);
    }
    word(value, "final", 0x2ffe, pc as u16);
    word(value, "final", 0x2ffa, sr);
    word(value, "final", 0x2ffc, (pc >> 16) as u16);
    value["final"]["ssp"] = json!(0x2ffa);
    value["final"]["pc"] = json!(0x4004);
    value["final"]["sr"] = json!((sr | 0x2000) & !0x8000);
    value["final"]["prefetch"] = json!([0x1357, 0x2468]);
    value["reference_total_cycles"] = json!(34);
    value["transactions"] = json!([{"kind":"idle","duration_cycles":4},
        transfer("write",0x2ffe,pc as u16,5),transfer("write",0x2ffa,sr,5),
        transfer("write",0x2ffc,(pc>>16) as u16,5),
        transfer("read",u32::from(vector)*4,0,5),transfer("read",u32::from(vector)*4+2,0x4000,5),
        transfer("read",0x4000,0x1357,6),{"kind":"idle","duration_cycles":2},transfer("read",0x4002,0x2468,6)]);
}

#[test]
fn link_all_address_registers_capture_entry_value_in_both_banks() {
    for supervisor in [false, true] {
        for index in 0..8 {
            let mut value = case("LINK", 0x4e50 | index);
            let sp = if supervisor { "ssp" } else { "usp" };
            let source = if index == 7 { 0x2000 } else { 0x12345678 };
            let fc = if supervisor { 5 } else { 1 };
            let pfc = fc + 1;
            for state in ["initial", "final"] {
                value[state]["sr"] = json!(if supervisor { 0x2000 } else { 0 });
                word(&mut value, state, 0x1002, 0xfff8);
                if index < 7 {
                    value[state][format!("a{index}")] = json!(source);
                }
            }
            value["initial"]["prefetch"][1] = json!(0xfff8);
            value["initial"][sp] = json!(0x2000);
            value["final"][sp] = json!(0x1ff4);
            if index < 7 {
                value["final"][format!("a{index}")] = json!(0x1ffc);
            }
            word(&mut value, "final", 0x1ffc, (source >> 16) as u16);
            word(&mut value, "final", 0x1ffe, source as u16);
            value["final"]["pc"] = json!(0x1008);
            value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
            value["reference_total_cycles"] = json!(16);
            value["transactions"] = json!([
                transfer("read", 0x1004, 0x5678, pfc),
                transfer("write", 0x1ffc, (source >> 16) as u16, fc),
                transfer("write", 0x1ffe, source as u16, fc),
                transfer("read", 0x1006, 0x9abc, pfc)
            ]);
            assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
            value["transactions"].as_array_mut().unwrap().swap(1, 2);
            assert!(run(&serde_json::from_value(value).unwrap()).is_err());
        }
    }
}

#[test]
fn unlinked_a7_is_loaded_pointer_and_frame_reads_are_finite() {
    for index in 0..8 {
        let mut value = case("UNLINK", 0x4e58 | index);
        value["initial"]["ssp"] = json!(0x2000);
        for state in ["initial", "final"] {
            if index < 7 {
                value[state][format!("a{index}")] = json!(0x2000);
            }
            word(&mut value, state, 0x2000, 0x1234);
            word(&mut value, state, 0x2002, 0x5678);
        }
        if index < 7 {
            value["final"][format!("a{index}")] = json!(0x12345678);
        }
        value["final"]["ssp"] = json!(if index == 7 { 0x12345678 } else { 0x2004 });
        value["reference_total_cycles"] = json!(12);
        value["transactions"] = json!([
            transfer("read", 0x2000, 0x1234, 5),
            transfer("read", 0x2002, 0x5678, 5),
            transfer("read", 0x1004, 0x5678, 6)
        ]);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        if index == 7 {
            let mut bad = value.clone();
            bad["final"]["ssp"] = json!(0x2004);
            assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        }
        let mut bad = value.clone();
        bad["transactions"][1]["address_bus"] = json!(0x2004);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["initial"]["ram"]
            .as_array_mut()
            .unwrap()
            .retain(|x| x[0] != json!(0x2002));
        assert!(
            run(&serde_json::from_value(value).unwrap())
                .unwrap_err()
                .contains("unobserved")
        );
    }
}

#[test]
fn rtr_uses_only_ccr_and_preserves_entry_trace_and_bank() {
    for supervisor in [false, true] {
        let mut value = case("RTR", 0x4e77);
        let sp = if supervisor { "ssp" } else { "usp" };
        let fc = if supervisor { 5 } else { 1 };
        for state in ["initial", "final"] {
            value[state]["sr"] = json!(if supervisor { 0xa000 } else { 0x8000 });
            word(&mut value, state, 0x2000, 0x071f);
            word(&mut value, state, 0x2002, 0);
            word(&mut value, state, 0x2004, 0x1100);
            word(&mut value, state, 0x1100, 0xaaaa);
            word(&mut value, state, 0x1102, 0xbbbb);
        }
        value["initial"][sp] = json!(0x2000);
        value["final"][sp] = json!(0x2006);
        value["final"]["sr"] = json!(if supervisor { 0xa01f } else { 0x801f });
        value["normalization_exclusion"] = json!("trace");
        value["final"]["pc"] = json!(0x1104);
        value["final"]["prefetch"] = json!([0xaaaa, 0xbbbb]);
        value["reference_total_cycles"] = json!(20);
        value["transactions"] = json!([
            transfer("read", 0x2000, 0x071f, fc),
            transfer("read", 0x2002, 0, fc),
            transfer("read", 0x2004, 0x1100, fc),
            transfer("read", 0x1100, 0xaaaa, fc + 1),
            transfer("read", 0x1102, 0xbbbb, fc + 1)
        ]);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        let mut bad = value.clone();
        bad["final"]["sr"] = json!(0x871f);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["transactions"].as_array_mut().unwrap().swap(0, 1);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn trap_and_privilege_frames_use_different_saved_pc_and_trace_policy() {
    for (cohort, opcode, vector, pc) in [
        ("TRAP", 0x4e4f, 47, 0x1002),
        ("MOVEfromUSP", 0x4e6f, 8, 0x1000),
        ("MOVEtoUSP", 0x4e67, 8, 0x1000),
        ("MOVEtoSR", 0x46d0, 8, 0x1000),
    ] {
        let mut value = case(cohort, opcode);
        value["initial"]["sr"] = json!(0x8015);
        value["normalization_exclusion"] = json!("trace");
        exception(&mut value, vector, pc, 0x8015);
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        for (field, data) in [
            ("data_bus", 0x1004),
            ("address_bus", 0x2ff8),
            ("function_code", 1),
            ("lds", 0),
        ] {
            let mut bad = value.clone();
            bad["transactions"][1][field] = json!(data);
            assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        }
        let mut bad = value.clone();
        bad["transactions"][4]["address_bus"] = json!(36);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        value["transactions"].as_array_mut().unwrap().swap(1, 2);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn usp_a7_aliases_and_wrong_metadata_remain_strict() {
    for (cohort, opcode) in [("MOVEfromUSP", 0x4e6f), ("MOVEtoUSP", 0x4e67)] {
        let mut value = case(cohort, opcode);
        value["initial"]["ssp"] = json!(0x2000);
        value["initial"]["usp"] = json!(0x3000);
        value["final"]["ssp"] = json!(if cohort == "MOVEfromUSP" {
            0x3000
        } else {
            0x2000
        });
        value["final"]["usp"] = json!(if cohort == "MOVEfromUSP" {
            0x3000
        } else {
            0x2000
        });
        assert!(run(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        value["transactions"] = json!([transfer("write", 0x1ffe, 0, 5)]);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
    for (cohort, opcode) in [
        ("LINK", 0x4e58),
        ("UNLINK", 0x4e50),
        ("RTR", 0x4e73),
        ("TRAP", 0x4e76),
        ("MOVEfromUSP", 0x4e60),
        ("MOVEtoUSP", 0x4e68),
    ] {
        assert!(verify_reader(std::io::Cursor::new(cohort_stream(case(cohort, opcode)))).is_err());
    }
    assert!(verify_reader(std::io::Cursor::new(cohort_stream(case("TRAPV", 0x4e76)))).is_err());
}
