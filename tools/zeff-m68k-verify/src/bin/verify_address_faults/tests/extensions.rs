use super::*;
use std::collections::BTreeMap;

fn put(memory: &mut BTreeMap<u32, u8>, address: u32, word: u16) {
    memory.insert(address, (word >> 8) as u8);
    memory.insert(address + 1, word as u8);
}

fn fixture(route: (bool, u16, u16), state: (bool, u32, u16, u16)) -> Value {
    let (write, mode, register) = route;
    let (long, value, old_sr, phase_sr) = state;
    let opcode = (if long { 2 } else { 3 }) << 12
        | if write {
            register << 9 | mode << 6
        } else {
            mode << 3 | register
        };
    let indexed = mode == 6 || mode == 7 && register == 3;
    let absolute_long = mode == 7 && register == 1;
    let program_operand = !write && mode == 7 && matches!(register, 2 | 3);
    let (extension, index, operand) = match (mode, register) {
        (5, _) => (1, 0x10, 0x1234_6001_u32),
        (6, _) => (0x58ff, 0x20, 0x1234_601f),
        (7, 0) => (0x6001, 0x10, 0x6001),
        (7, 1) => (0x1234, 0x10, 0x1234_6001),
        (7, 2) => (0x4fff, 0x10, 0x6001),
        (7, 3) => (0x58ff, 0x5000, 0x6001),
        _ => unreachable!(),
    };
    let saved_pc = if absolute_long {
        0x1006_u32
    } else if write || mode == 7 && register == 0 {
        0x1004
    } else {
        0x1002
    };
    let mut case = case_value();
    case["cohort"] = json!(if long { "MOVE.l" } else { "MOVE.w" });
    case["initial"]["d0"] = json!(value);
    case["initial"]["d5"] = json!(index);
    case["initial"]["a0"] = json!(0x1234_6000_u32);
    case["initial"]["sr"] = json!(old_sr);
    case["initial"]["prefetch"] = json!([opcode, extension]);
    let mut memory: BTreeMap<u32, u8> =
        serde_json::from_value::<Vec<(u32, u8)>>(case["initial"]["ram"].clone())
            .unwrap()
            .into_iter()
            .collect();
    put(&mut memory, 0x1000, opcode);
    put(&mut memory, 0x1002, extension);
    put(
        &mut memory,
        0x1004,
        if absolute_long { 0x6001 } else { 0x4e71 },
    );
    if absolute_long {
        put(&mut memory, 0x1006, 0x4e71);
    }
    case["initial"]["ram"] = json!(memory.iter().map(|(&a, &b)| (a, b)).collect::<Vec<_>>());
    let mut phases = Vec::new();
    if indexed {
        phases.push(json!({"kind":"idle", "duration_cycles":2}));
    }
    for (address, word) in [
        (0x1004, if absolute_long { 0x6001 } else { 0x4e71 }),
        (0x1006, 0x4e71),
    ]
    .into_iter()
    .take(if absolute_long { 2 } else { 1 })
    {
        phases.push(
            json!({"kind":"read", "duration_cycles":4, "function_code":2,
            "address_bus":address, "data_bus":word, "uds":1, "lds":1}),
        );
    }
    let fc = if program_operand { 2 } else { 1 };
    phases.push(
        json!({"kind":if write {"write_address_error"} else {"read_address_error"},
        "duration_cycles":4, "function_code":fc, "address_bus":operand & 0x00ff_fffe,
        "data_bus":if write {if long {value >> 16} else {value & 65535}} else {0},
        "uds":1, "lds":1}),
    );
    phases.push(json!({"kind":"idle", "duration_cycles":8}));
    let ssw = (opcode & 0xffe0) | fc | if write { 0 } else { 16 };
    for (address, word) in [
        (0x1ffe, saved_pc as u16),
        (0x1ffa, phase_sr),
        (0x1ffc, 0),
        (0x1ff8, opcode),
        (0x1ff6, operand as u16),
        (0x1ff2, ssw),
        (0x1ff4, (operand >> 16) as u16),
    ] {
        phases.push(
            json!({"kind":"write", "duration_cycles":4, "function_code":5,
            "address_bus":address, "data_bus":word, "uds":1, "lds":1}),
        );
        put(&mut memory, address, word);
    }
    for (address, word, fc) in [(12, 0, 5), (14, 0x3000, 5), (0x3000, 0x4e71, 6)] {
        phases.push(
            json!({"kind":"read", "duration_cycles":4, "function_code":fc,
            "address_bus":address, "data_bus":word, "uds":1, "lds":1}),
        );
    }
    phases.push(json!({"kind":"idle", "duration_cycles":2}));
    phases.push(
        json!({"kind":"read", "duration_cycles":4, "function_code":6,
        "address_bus":0x3002, "data_bus":0x7001, "uds":1, "lds":1}),
    );
    let mut final_state = case["initial"].clone();
    final_state["ssp"] = json!(0x1ff2);
    final_state["sr"] = json!(phase_sr | 0x2000);
    final_state["pc"] = json!(0x3004);
    final_state["prefetch"] = json!([0x4e71, 0x7001]);
    final_state["ram"] = json!(memory.into_iter().collect::<Vec<_>>());
    case["final"] = final_state;
    case["transactions"] = json!(phases);
    case["reference_total_cycles"] =
        json!(62 + u32::from(indexed) * 2 + u32::from(absolute_long) * 4);
    case
}

#[test]
fn extension_routes_check_complete_frames_with_program_relative_operands() {
    let mut checked = 0;
    for long in [false, true] {
        for write in [false, true] {
            for (mode, register) in [(5, 0), (6, 0), (7, 0), (7, 1), (7, 2), (7, 3)] {
                if write && register >= 2 && mode == 7 {
                    continue;
                }
                let sr = if !write {
                    0x1f
                } else if !long {
                    0x14
                } else if mode == 7 {
                    0x10
                } else {
                    0x13
                };
                let case: Case = serde_json::from_value(fixture(
                    (write, mode, register),
                    (long, 0x10000, 0x1f, sr),
                ))
                .unwrap();
                assert_eq!(witness::run(&case), Ok(witness::WitnessResult::Frame));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 20);
}

#[test]
fn displacement_long_high_fold_retains_v_c_and_sticky_old_z() {
    for (value, old, captured) in [
        (0, 0x1f, 0x17),
        (0, 0x1b, 0x13),
        (0, 0x10, 0x10),
        (0x10000, 0x1f, 0x13),
        (0x80000001, 0x1f, 0x1b),
    ] {
        let case: Case =
            serde_json::from_value(fixture((true, 5, 0), (true, value, old, captured))).unwrap();
        assert_eq!(witness::run(&case), Ok(witness::WitnessResult::Frame));
    }
}

#[test]
fn absolute_long_prefix_requires_actual_initial_ram_and_exact_read_metadata() {
    let case = fixture((false, 7, 1), (true, 0x10000, 0x1f, 0x1f));
    for field in [
        "address_bus",
        "function_code",
        "data_bus",
        "duration_cycles",
        "uds",
    ] {
        let mut bad = case.clone();
        bad["transactions"][1][field] = json!(0);
        let case: Case = serde_json::from_value(bad).unwrap();
        assert!(witness::run(&case).is_err());
    }
    let mut bad = case;
    bad["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v[0] != json!(0x1004));
    let case: Case = serde_json::from_value(bad).unwrap();
    assert!(witness::run(&case).is_err());
}

#[test]
fn pc_relative_odd_operands_reject_data_function_codes() {
    for register in [2, 3] {
        let mut bad = fixture((false, 7, register), (true, 0x10000, 0x1f, 0x1f));
        let marker = if register == 3 { 2 } else { 1 };
        bad["transactions"][marker]["function_code"] = json!(1);
        let case: Case = serde_json::from_value(bad).unwrap();
        assert!(witness::run(&case).is_err());
    }
}
