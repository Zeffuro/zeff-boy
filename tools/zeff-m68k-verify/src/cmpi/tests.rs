use super::{verify, verify_export};
use crate::reference::Case;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn typed(value: Value) -> Case {
    serde_json::from_value(value).unwrap()
}

fn phase(address: u32, value: u16, bank: bool, program: bool, lane: u32) -> Value {
    json!({"kind":"read","duration_cycles":4,"address_bus":address & 0xfffffe,
        "function_code":if program {if bank {6} else {2}} else if bank {5} else {1},
        "data_bus":if program {u32::from(value)} else if lane == 0 {u32::from(value) & 0xff00} else {u32::from(value) & 255},
        "uds":u32::from(program || lane == 0),"lds":u32::from(program || lane != 0)})
}

fn case(word: u16, bank: bool, lane: u32) -> Value {
    let size = word >> 6 & 3;
    let mode = word >> 3 & 7;
    let register = word & 7;
    let ea = 0x3000 | lane;
    let (extension, base) = match mode {
        4 => (0, ea + if register == 7 { 2 } else { 1 }),
        5 => (0xfff0, ea + 16),
        6 => (0x30f0, ea + 32),
        7 if register == 0 => (ea as u16, ea),
        7 => (0, ea),
        _ => (0, ea),
    };
    let immediate = match size {
        0 => 0xff80,
        1 => 0x8000,
        _ => 0,
    };
    let destination = match size {
        0 => 0xdead_be7f_u32,
        1 => 0xdead_7fff,
        _ => 0x8000_0000,
    };
    let extensions = if mode < 5 {
        0
    } else if mode == 7 && register == 1 {
        2
    } else {
        1
    };
    let mut memory = BTreeMap::new();
    let mut put = |address: u32, value: u16| {
        memory.insert(address, value >> 8);
        memory.insert(address + 1, value & 255);
    };
    put(0x1000, word);
    put(0x1002, 0xbeef);
    let mut phases = Vec::new();
    let canonical = 0x1004 + u32::from(size == 2) * 2 + extensions * 2;
    if size == 2 {
        put(0x1004, 1);
        phases.push(phase(0x1004, 1, bank, true, 0));
        put(0x1006, 0x4e71);
        phases.push(phase(0x1006, 0x4e71, bank, true, 0));
    } else {
        let fetched = if extensions == 0 { 0x4e71 } else { extension };
        put(0x1004, fetched);
        phases.push(phase(0x1004, fetched, bank, true, 0));
    }
    if matches!(mode, 4 | 6) {
        phases.push(json!({"kind":"idle","duration_cycles":2}));
    }
    if extensions == 1 {
        put(0x1006, 0x4e71);
        phases.push(phase(0x1006, 0x4e71, bank, true, 0));
    } else if extensions == 2 {
        put(0x1006, ea as u16);
        phases.push(phase(0x1006, ea as u16, bank, true, 0));
        put(0x1008, 0x4e71);
        phases.push(phase(0x1008, 0x4e71, bank, true, 0));
    }
    if mode != 0 {
        let operand = if lane == 0 { 0x7f55 } else { 0xaa7f };
        put(0x3000, operand);
        phases.push(phase(ea, operand, bank, false, lane));
    }
    put(canonical + 2, 0x7201);
    phases.push(phase(canonical + 2, 0x7201, bank, true, 0));
    if size == 2 {
        phases.push(json!({"kind":"idle","duration_cycles":2}));
    }
    let ram: Vec<Value> = memory.into_iter().map(|(a, v)| json!([a, v])).collect();
    let sr = if bank { 0x2514 } else { 0x514 };
    let mut initial = json!({"d0":destination,"d1":1,"d2":2,"d3":0xfffffff0_u32,"d4":4,"d5":5,"d6":6,"d7":7,
        "a0":base,"a1":0x4000,"a2":0x5000,"a3":0x6000,"a4":0x7000,"a5":0x8000,"a6":0x9000,
        "usp":0xb000,"ssp":0xa000,"sr":sr,"pc":0x1004,"prefetch":[word,immediate],"ram":ram});
    if register == 7 {
        initial[if bank { "ssp" } else { "usp" }] = json!(base);
    }
    let mut expected = initial.clone();
    expected["sr"] = json!((sr & !15) | if size == 2 { 2 } else { 11 });
    expected["pc"] = json!(canonical + 4);
    expected["prefetch"] = json!([0x4e71, 0x7201]);
    if mode == 3 || mode == 4 {
        expected[if register == 7 {
            if bank { "ssp" } else { "usp" }
        } else {
            "a0"
        }] = json!(if mode == 3 {
            ea + if register == 7 { 2 } else { 1 }
        } else {
            ea
        });
    }
    let cycles: u64 = phases
        .iter()
        .map(|p| p["duration_cycles"].as_u64().unwrap())
        .sum();
    json!({"cohort":(["CMP.b","CMP.w","CMP.l"][usize::from(size)]),"name":"synthetic-cmpi",
        "initial":initial,"final":expected,"transactions":phases,"reference_total_cycles":cycles,
        "normalization_exclusion":null})
}

fn change_word(row: &mut Value, address: u32, value: u16) {
    for state in ["initial", "final"] {
        for byte in row[state]["ram"].as_array_mut().unwrap() {
            if byte[0].as_u64() == Some(u64::from(address)) {
                byte[1] = json!(value >> 8);
            } else if byte[0].as_u64() == Some(u64::from(address + 1)) {
                byte[1] = json!(value & 255);
            }
        }
    }
}

#[test]
fn sized_immediate_actual_ram_and_tail_match_every_new_form_and_bank() {
    for word in [
        0x0c00, 0x0c40, 0x0c80, 0x0c10, 0x0c1f, 0x0c27, 0x0c28, 0x0c30, 0x0c38, 0x0c39,
    ] {
        for bank in [false, true] {
            for lane in [0, 1] {
                verify(&typed(case(word, bank, lane))).unwrap();
            }
        }
    }
}

#[test]
fn byte_immediate_high_and_dn_upper_bits_do_not_enter_subtraction() {
    for high in [0, 0x7f00, 0xff00] {
        let mut row = case(0x0c00, true, 0);
        row["initial"]["prefetch"][1] = json!(high | 0x80);
        verify(&typed(row)).unwrap();
    }
    for (word, value) in [(0x0c00, 0x1100007f_u32), (0x0c40, 0x11007fff)] {
        let mut row = case(word, true, 0);
        for state in ["initial", "final"] {
            row[state]["d0"] = json!(value);
        }
        verify(&typed(row)).unwrap();
    }
    for (word, value) in [(0x0c00, 0xff7f), (0x0c40, 0x7fff), (0x0c80, 0x8000)] {
        let mut row = case(word, true, 0);
        row["initial"]["prefetch"][1] = json!(value);
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = case(0x0c80, true, 0);
    change_word(&mut row, 0x1004, 0);
    row["transactions"][0]["data_bus"] = json!(0);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn original_phase_fields_order_and_long_tail_are_required() {
    let seed = case(0x0c39, true, 1);
    for field in [
        "duration_cycles",
        "function_code",
        "address_bus",
        "data_bus",
        "uds",
        "lds",
    ] {
        let mut row = seed.clone();
        row["transactions"][3][field] = json!(row["transactions"][3][field].as_u64().unwrap() ^ 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    let mut row = seed.clone();
    row["transactions"][3]["data_bus"] = json!(0xaa7f);
    assert!(verify(&typed(row)).is_err());
    for mutation in 0..4 {
        let mut row = seed.clone();
        match mutation {
            0 => row["transactions"].as_array_mut().unwrap().swap(0, 1),
            1 => row["transactions"][3]["kind"] = json!("write"),
            2 => row["reference_total_cycles"] = json!(21),
            _ => row["transactions"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"idle","duration_cycles":2})),
        }
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = case(0x0c80, true, 0);
    row["transactions"].as_array_mut().unwrap().pop();
    row["reference_total_cycles"] = json!(12);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn full_final_state_ram_and_pipeline_are_derived_from_initial_source() {
    let seed = case(0x0c1f, true, 1);
    for field in ["d0", "d7", "a0", "a6", "usp", "ssp", "pc", "sr"] {
        let mut row = seed.clone();
        row["final"][field] = json!(row["final"][field].as_u64().unwrap() ^ 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    for slot in [0, 1] {
        let mut row = seed.clone();
        row["final"]["prefetch"][slot] = json!(0);
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = seed;
    row["final"]["ram"][0][1] = json!(0);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn index_original_alias_width_and_ignored_bits_use_actual_extensions() {
    for extension in [0x30f0, 0x38f0, 0x36f0, 0x3ef0] {
        let mut row = case(0x0c30, true, 1);
        change_word(&mut row, 0x1004, extension);
        row["transactions"][0]["data_bus"] = json!(extension);
        verify(&typed(row)).unwrap();
    }
    let mut row = case(0x0c30, true, 1);
    change_word(&mut row, 0x1004, 0x80f1);
    row["transactions"][0]["data_bus"] = json!(0x80f1);
    for state in ["initial", "final"] {
        row[state]["a0"] = json!(0x1808);
    }
    verify(&typed(row)).unwrap();
}

#[test]
fn signed_absolute_and_program_operand_alias_keep_full_logical_geometry() {
    let mut row = case(0x0c38, true, 1);
    change_word(&mut row, 0x1004, 0xf001);
    row["transactions"][0]["data_bus"] = json!(0xf001);
    for state in ["initial", "final"] {
        for byte in row[state]["ram"].as_array_mut().unwrap() {
            if let Some(address) = byte[0].as_u64().filter(|a| *a >= 0x3000) {
                byte[0] = json!(address - 0x3000 + 0xfff000);
            }
        }
    }
    row["transactions"][2]["address_bus"] = json!(0xfff000);
    verify(&typed(row)).unwrap();
    for lane in [0, 1] {
        let mut row = case(0x0c10, true, lane);
        for state in ["initial", "final"] {
            row[state]["a0"] = json!(0x1004 | lane);
        }
        row["transactions"][1]["address_bus"] = json!(0x1004);
        row["transactions"][1]["data_bus"] = json!(if lane == 0 { 0x4e00 } else { 0x71 });
        verify(&typed(row)).unwrap();
    }
}

#[test]
fn normalization_tags_and_invalid_original_boundaries_are_rejected() {
    for mutation in 0..6 {
        let mut row = case(0x0c10, true, 0);
        match mutation {
            0 => row["initial"]["prefetch"][0] = json!(0x0c50),
            1 => row["initial"]["sr"] = json!(0xa514),
            2 => row["cohort"] = json!("CMP.w"),
            3 => row["normalization_exclusion"] = json!("trace"),
            4 => row["normalization_exclusion"] = json!("exception_or_low_supervisor_data"),
            _ => row["initial"]["pc"] = json!(0x1005),
        }
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = case(0x0c10, true, 0);
    for state in ["initial", "final"] {
        row[state]["a0"] = json!(648);
        for byte in row[state]["ram"].as_array_mut().unwrap() {
            if let Some(address) = byte[0].as_u64().filter(|a| *a >= 0x3000) {
                byte[0] = json!(address - 0x3000 + 648);
            }
        }
    }
    row["transactions"][1]["address_bus"] = json!(648);
    row["normalization_exclusion"] = json!("exception_or_low_supervisor_data");
    verify(&typed(row)).unwrap();
}

#[test]
fn malformed_memory_and_export_identity_fail_before_cpu_execution() {
    let seed = case(0x0c10, true, 0);
    for mutation in 0..3 {
        let mut row = seed.clone();
        match mutation {
            0 => {
                let byte = row["initial"]["ram"][0].clone();
                row["initial"]["ram"].as_array_mut().unwrap().push(byte);
            }
            1 => row["initial"]["ram"][0][0] = json!(0x1000000),
            _ => {
                row["initial"]["ram"].as_array_mut().unwrap().pop();
            }
        }
        assert!(verify(&typed(row)).is_err());
    }
    assert!(verify_export("").is_err());
    assert!(verify_export(&serde_json::to_string(&seed).unwrap()).is_err());
}
