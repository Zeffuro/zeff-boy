use super::*;
use std::collections::BTreeMap;

fn put(memory: &mut BTreeMap<u32, u8>, address: u32, word: u16) {
    memory.insert(address, (word >> 8) as u8);
    memory.insert(address + 1, word as u8);
}

fn fixture(long: bool, mode: u16, register: u16, destination: u16) -> Value {
    let opcode = (if long { 2 } else { 3 }) << 12 | destination << 9 | 0x40 | mode << 3 | register;
    let stride = if long { 4 } else { 2 };
    let (extension, base, operand) = match (mode, register) {
        (2 | 3, _) => (0x4afc, 0x1234_6001_u32, 0x1234_6001_u32),
        (4, _) => (0x4afc, 0x1234_6001 + stride, 0x1234_6001),
        (5, _) => (1, 0x1234_6000, 0x1234_6001),
        (6, _) => (0x58ff, 0x1234_6000, 0x1234_601f),
        (7, 0) => (0x6001, 0, 0x6001),
        (7, 1) => (0x1234, 0, 0x1234_6001),
        (7, 2) => (0x4fff, 0, 0x6001),
        (7, 3) => (0x58ff, 0, 0x1021),
        _ => unreachable!(),
    };
    let phase_base = match mode {
        3 if !long => base + stride,
        4 => operand,
        _ => base,
    };
    let saved_pc = match (mode, register) {
        (4, _) if !long => 0x1004_u32,
        (7, 0) => 0x1004,
        (7, 1) => 0x1006,
        _ => 0x1002,
    };
    let mut case = case_value();
    case["cohort"] = json!(if long { "MOVEA.l" } else { "MOVEA.w" });
    case["initial"]["a0"] = json!(base);
    case["initial"]["d5"] = json!(0x20);
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
        if mode == 7 && register == 1 {
            0x6001
        } else {
            0x4afc
        },
    );
    put(&mut memory, 0x1006, 0x4afc);
    case["initial"]["ram"] = json!(memory.iter().map(|(&a, &b)| (a, b)).collect::<Vec<_>>());
    let mut phases = Vec::new();
    if matches!(mode, 4 | 6) || mode == 7 && register == 3 {
        phases.push(json!({"kind":"idle", "duration_cycles":2}));
    }
    if mode >= 5 {
        for address in [0x1004, 0x1006]
            .into_iter()
            .take(if mode == 7 && register == 1 { 2 } else { 1 })
        {
            let word = if address == 0x1004 && mode == 7 && register == 1 {
                0x6001
            } else {
                0x4afc
            };
            phases.push(json!({"kind":"read", "duration_cycles":4,
                "function_code":2, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
        }
    }
    let prefix = phases
        .iter()
        .map(|p| p["duration_cycles"].as_u64().unwrap())
        .sum::<u64>();
    let fc = if mode == 7 && matches!(register, 2 | 3) {
        2
    } else {
        1
    };
    phases.push(json!({"kind":"read_address_error", "duration_cycles":4,
        "function_code":fc, "address_bus":operand & 0x00ff_fffe,
        "data_bus":0xffff, "uds":1, "lds":1}));
    phases.push(json!({"kind":"idle", "duration_cycles":8}));
    for (address, word) in [
        (0x1ffe, saved_pc as u16),
        (0x1ffa, 0x001f),
        (0x1ffc, 0),
        (0x1ff8, opcode),
        (0x1ff6, operand as u16),
        (0x1ff2, (opcode & !31) | fc | 16),
        (0x1ff4, (operand >> 16) as u16),
    ] {
        phases.push(json!({"kind":"write", "duration_cycles":4,
            "function_code":5, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
        put(&mut memory, address, word);
    }
    for (address, word, fc) in [(12, 0, 5), (14, 0x3000, 5), (0x3000, 0x4e71, 6)] {
        phases.push(json!({"kind":"read", "duration_cycles":4,
            "function_code":fc, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
    }
    phases.push(json!({"kind":"idle", "duration_cycles":2}));
    phases.push(json!({"kind":"read", "duration_cycles":4,
        "function_code":6, "address_bus":0x3002, "data_bus":0x7001, "uds":1, "lds":1}));
    let mut expected = case["initial"].clone();
    expected["a0"] = json!(phase_base);
    expected["ssp"] = json!(0x1ff2);
    expected["sr"] = json!(0x201f);
    expected["pc"] = json!(0x3004);
    expected["prefetch"] = json!([0x4e71, 0x7001]);
    expected["ram"] = json!(memory.into_iter().collect::<Vec<_>>());
    case["final"] = expected;
    case["transactions"] = json!(phases);
    case["reference_total_cycles"] = json!(prefix + 58);
    case
}

#[test]
fn all_movea_odd_routes_keep_old_destination_and_ccr_after_source_updates() {
    let mut checked = 0;
    for long in [false, true] {
        for (mode, register) in [
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (7, 1),
            (7, 2),
            (7, 3),
        ] {
            for destination in [0, 7] {
                let case: Case =
                    serde_json::from_value(fixture(long, mode, register, destination)).unwrap();
                assert_eq!(witness::run(&case), Ok(witness::WitnessResult::Frame));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 36);
}

#[test]
fn movea_intake_retains_trace_exclusion_and_rejects_changed_phase_state() {
    let value = fixture(true, 3, 0, 0);
    let report = intake::verify_reader(std::io::Cursor::new(stream(value.clone(), 1))).unwrap();
    assert_eq!(
        (report.selected, report.passed, report.capture_gaps),
        (1, 1, 0)
    );
    let mut traced = value.clone();
    traced["initial"]["sr"] = json!(0x801f);
    traced["normalization_exclusion"] = json!("trace");
    let report = intake::verify_reader(std::io::Cursor::new(stream(traced, 1))).unwrap();
    assert_eq!(
        (report.selected, report.selected_trace, report.passed),
        (0, 1, 0)
    );
    for field in ["a0", "sr"] {
        let mut bad = value.clone();
        bad["final"][field] = json!(0);
        let case: Case = serde_json::from_value(bad).unwrap();
        assert!(witness::run(&case).is_err());
    }
}

#[test]
fn movea_frame_validator_rejects_a_fabricated_destination_usp() {
    let mut bad = fixture(false, 5, 0, 7);
    bad["final"]["usp"] = json!(0xffff_8001_u32);
    let case: Case = serde_json::from_value(bad).unwrap();
    assert!(witness::run(&case).is_err());
}
