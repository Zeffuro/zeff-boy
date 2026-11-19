use super::{verify, verify_export};
use crate::reference::Case;
use serde_json::{Value, json};

fn case(word: u16) -> Value {
    let (base, extension, next_pc, queue, reads, idle) = match word {
        0x4a00 | 0x4a40 | 0x4a80 => (0x2000, 0x7001, 0x1006, [0x7001, 0x4e71], vec![], false),
        0x4a10 | 0x4a18 => (
            0x2000,
            0x7001,
            0x1006,
            [0x7001, 0x4e71],
            vec![(0x2000, 0x8000, 5, 1, 0)],
            false,
        ),
        0x4a20 => (
            0x2001,
            0x7001,
            0x1006,
            [0x7001, 0x4e71],
            vec![(0x2000, 0x8000, 5, 1, 0)],
            true,
        ),
        0x4a28 => (
            0x1ff0,
            0x0010,
            0x1008,
            [0x4e71, 0x7201],
            vec![(0x1004, 0x4e71, 6, 1, 1), (0x2000, 0x8000, 5, 1, 0)],
            false,
        ),
        0x4a30 => (
            0x1fee,
            0x3010,
            0x1008,
            [0x4e71, 0x7201],
            vec![(0x1004, 0x4e71, 6, 1, 1), (0x2000, 0x8000, 5, 1, 0)],
            true,
        ),
        0x4a38 => (
            0x2000,
            0x2000,
            0x1008,
            [0x4e71, 0x7201],
            vec![(0x1004, 0x4e71, 6, 1, 1), (0x2000, 0x8000, 5, 1, 0)],
            false,
        ),
        0x4a39 => (
            0x2000,
            0,
            0x100a,
            [0x7201, 0x7301],
            vec![
                (0x1004, 0x2000, 6, 1, 1),
                (0x1006, 0x7201, 6, 1, 1),
                (0x2000, 0x8000, 5, 1, 0),
            ],
            false,
        ),
        _ => panic!("test encoding"),
    };
    let source = if word == 0x4a80 {
        0x80000000_u32
    } else {
        0xdead0080
    };
    let mut initial = json!({
        "d0":source,"d1":1,"d2":2,"d3":2,"d4":4,"d5":5,"d6":6,"d7":7,
        "a0":base,"a1":0x3000,"a2":0x4000,"a3":0x5000,"a4":0x6000,"a5":0x7000,"a6":0x8000,
        "usp":0xb000,"ssp":0xa000,"sr":0x251f,"pc":0x1004,"prefetch":[word,extension],
        "ram":[[0x1004,0x4e],[0x1005,0x71],[0x1006,0x72],[0x1007,1],
            [0x1008,0x73],[0x1009,1],[0x2000,0x80],[0x2001,0x5a]]
    });
    if word == 0x4a39 {
        initial["ram"][0][1] = json!(0x20);
        initial["ram"][1][1] = json!(0);
    }
    let mut expected = initial.clone();
    expected["pc"] = json!(next_pc);
    expected["prefetch"] = json!(queue);
    expected["sr"] = json!(if word == 0x4a40 { 0x2510 } else { 0x2518 });
    if word == 0x4a18 {
        expected["a0"] = json!(0x2001);
    }
    if word == 0x4a20 {
        expected["a0"] = json!(0x2000);
    }
    let mut phases = Vec::new();
    if idle {
        phases.push(json!({"kind":"idle","duration_cycles":2}));
    }
    for (address, data, fc, uds, lds) in reads {
        phases.push(json!({"kind":"read","duration_cycles":4,"function_code":fc,
            "address_bus":address,"data_bus":data,"uds":uds,"lds":lds}));
    }
    phases.push(json!({"kind":"read","duration_cycles":4,"function_code":6,
        "address_bus":next_pc - 2,"data_bus":queue[1],"uds":1,"lds":1}));
    let clocks = phases.len() as u32 * 4 - if idle { 2 } else { 0 };
    json!({"cohort":match word {0x4a40=>"TST.w",0x4a80=>"TST.l",_=>"TST.b"},
        "name":"synthetic-tst","initial":initial,"final":expected,
        "reference_total_cycles":clocks,"transactions":phases,"normalization_exclusion":null})
}

fn typed(value: Value) -> Case {
    serde_json::from_value(value).unwrap()
}

#[test]
fn original_n_and_actual_ram_drive_every_selected_geometry() {
    for word in [
        0x4a00, 0x4a40, 0x4a80, 0x4a10, 0x4a18, 0x4a20, 0x4a28, 0x4a30, 0x4a38, 0x4a39,
    ] {
        verify(&typed(case(word))).unwrap();
    }
}

#[test]
fn every_raw_phase_field_kind_order_and_clock_is_checked() {
    let seed = case(0x4a30);
    for field in [
        "duration_cycles",
        "function_code",
        "address_bus",
        "data_bus",
        "uds",
        "lds",
    ] {
        let mut row = seed.clone();
        let old = row["transactions"][1][field].as_u64().unwrap();
        row["transactions"][1][field] = json!(old ^ 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    for mutation in 0..4 {
        let mut row = seed.clone();
        match mutation {
            0 => row["transactions"].as_array_mut().unwrap().swap(1, 2),
            1 => row["transactions"][1]["kind"] = json!("write"),
            2 => row["transactions"][0]["duration_cycles"] = json!(3),
            _ => row["reference_total_cycles"] = json!(15),
        }
        assert!(verify(&typed(row)).is_err());
    }
}

#[test]
fn final_registers_queue_and_ram_are_independently_derived() {
    let seed = case(0x4a18);
    for field in ["d0", "d7", "a0", "a6", "usp", "ssp", "pc", "sr"] {
        let mut row = seed.clone();
        row["final"][field] = json!(row["final"][field].as_u64().unwrap() ^ 1);
        assert!(verify(&typed(row)).is_err(), "{field}");
    }
    for slot in 0..2 {
        let mut row = seed.clone();
        row["final"]["prefetch"][slot] = json!(0);
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = seed;
    row["final"]["ram"][0][1] = json!(0);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn lane_selection_uses_the_active_byte_and_a7_stride() {
    let mut row = case(0x4a18);
    row["initial"]["prefetch"][0] = json!(0x4a1f);
    row["final"]["prefetch"][0] = json!(0x7001);
    row["initial"]["ssp"] = json!(0x2001);
    row["final"]["ssp"] = json!(0x2003);
    row["final"]["a0"] = row["initial"]["a0"].clone();
    row["final"]["sr"] = json!(0x2510);
    row["transactions"][0]["data_bus"] = json!(0x5a);
    row["transactions"][0]["uds"] = json!(0);
    row["transactions"][0]["lds"] = json!(1);
    verify(&typed(row.clone())).unwrap();
    row["transactions"][0]["data_bus"] = json!(0x805a);
    assert!(verify(&typed(row)).is_err());
}

#[test]
fn tags_and_selection_preserve_only_the_supported_source_boundary() {
    for mutation in 0..5 {
        let mut row = case(0x4a10);
        match mutation {
            0 => row["initial"]["prefetch"][0] = json!(0x4a50),
            1 => row["initial"]["sr"] = json!(0xa51f),
            2 => row["cohort"] = json!("TST.w"),
            3 => row["normalization_exclusion"] = json!("trace"),
            _ => row["normalization_exclusion"] = json!("exception_or_low_supervisor_data"),
        }
        assert!(verify(&typed(row)).is_err());
    }
    let mut row = case(0x4a10);
    for state in ["initial", "final"] {
        row[state]["a0"] = json!(648);
        row[state]["ram"][6][0] = json!(648);
        row[state]["ram"][7][0] = json!(649);
    }
    row["transactions"][0]["address_bus"] = json!(648);
    row["normalization_exclusion"] = json!("exception_or_low_supervisor_data");
    verify(&typed(row)).unwrap();
}

#[test]
fn operand_and_program_alias_read_the_same_ram() {
    let mut row = case(0x4a10);
    row["initial"]["a0"] = json!(0x1004);
    row["final"]["a0"] = json!(0x1004);
    row["final"]["sr"] = json!(0x2510);
    row["transactions"][0]["address_bus"] = json!(0x1004);
    row["transactions"][0]["data_bus"] = json!(0x4e00);
    verify(&typed(row)).unwrap();
}

#[test]
fn retained_queue_and_extensions_precede_changed_program_ram() {
    for word in [0x4a00, 0x4a28, 0x4a30, 0x4a38, 0x4a39] {
        let mut row = case(word);
        for state in ["initial", "final"] {
            row[state]["ram"]
                .as_array_mut()
                .unwrap()
                .extend([json!([0x1002, 0xbe]), json!([0x1003, 0xef])]);
        }
        verify(&typed(row)).unwrap();
    }
}

#[test]
fn malformed_ram_and_changed_original_export_are_rejected() {
    let seed = case(0x4a10);
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
