use super::stack_status_tests::case;
use crate::*;
use serde_json::{Value, json};

fn fixture(
    cohort: &str,
    op: u16,
    operands: (u32, u32, u32),
    status: (u16, u16),
    clocks: u32,
) -> Value {
    let (input, count, output) = operands;
    let (sr, final_sr) = status;
    let mut value = case(cohort, op);
    value["initial"]["d0"] = json!(input);
    value["final"]["d0"] = json!(output);
    value["initial"]["d1"] = json!(count);
    value["final"]["d1"] = json!(count);
    value["initial"]["sr"] = json!(sr);
    value["final"]["sr"] = json!(final_sr);
    value["reference_total_cycles"] = json!(clocks);
    if sr & 0x8000 != 0 {
        value["normalization_exclusion"] = json!("trace");
    }
    if clocks > 4 {
        value["transactions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"idle","duration_cycles":clocks-4}));
    }
    value
}

fn check(value: Value) -> Result<(), String> {
    let case = serde_json::from_value(value).unwrap();
    if !format::transfer_metadata(&case) {
        return Err("cohort metadata differs".into());
    }
    run(&case)
}

#[test]
fn remaining_shifts_independent_count_alias_rotate_and_flag_vectors() {
    for (cohort, op, input, count, output, sr, final_sr, clocks) in [
        ("ASL.l", 0xe180, 1, 0, 256, 0x2000, 0x2000, 24),
        ("ASL.l", 0xe1a0, 0x80000001, 0, 2, 0x2000, 0x2013, 10),
        (
            "ASR.b", 0xe220, 0x12345680, 127, 0x123456ff, 0x2000, 0x2019, 132,
        ),
        (
            "ASR.l", 0xe2a0, 0x80000000, 64, 0x80000000, 0x2011, 0x2018, 8,
        ),
        (
            "LSL.b", 0xe108, 0x12345681, 0, 0x12345600, 0x2000, 0x2015, 22,
        ),
        ("LSL.l", 0xe3a8, 1, 33, 0, 0x2010, 0x2004, 74),
        (
            "LSR.b", 0xe208, 0x12345681, 0, 0x12345640, 0x2000, 0x2011, 8,
        ),
        ("LSR.l", 0xe2a8, 0x80000000, 32, 0, 0x2000, 0x2015, 72),
        (
            "ROL.b", 0xe118, 0x12345681, 0, 0x12345681, 0x2010, 0x2019, 22,
        ),
        (
            "ROL.l", 0xe3b8, 0x80000001, 32, 0x80000001, 0x2000, 0x2009, 72,
        ),
        (
            "ROR.b", 0xe218, 0x12345601, 0, 0x12345680, 0x2010, 0x2019, 8,
        ),
        (
            "ROXL.b", 0xe330, 0x12345680, 0, 0x12345680, 0x2010, 0x2019, 6,
        ),
        (
            "ROXL.l", 0xe3b0, 0x80000001, 33, 0x80000001, 0x2000, 0x2008, 74,
        ),
        (
            "ROXR.b", 0xe230, 0x12345602, 1, 0x12345681, 0x2010, 0x2008, 8,
        ),
        ("ROXR.l", 0xe2b0, 2, 1, 1, 0x2000, 0x2000, 10),
    ] {
        assert!(
            check(fixture(
                cohort,
                op,
                (input, count, output),
                (sr, final_sr),
                clocks
            ))
            .is_ok(),
            "{cohort}/{op:04x}"
        );
    }
}

#[test]
fn ext_and_swap_preserve_upper_register_bits_x_and_unused_state() {
    for (cohort, op, input, output, sr) in [
        ("EXT.w", 0x4880, 0x12345680, 0x1234ff80, 0x2018),
        ("EXT.w", 0x4880, 0x12010000, 0x12010000, 0x2014),
        ("EXT.l", 0x48c0, 0x12348000, 0xffff8000, 0x2018),
        ("SWAP", 0x4840, 0x12348000, 0x80001234, 0x2018),
        ("SWAP", 0x4840, 0, 0, 0x2014),
    ] {
        assert!(check(fixture(cohort, op, (input, 0, output), (0x201f, sr), 4)).is_ok());
    }
}

#[test]
fn ordinary_register_permissions_and_forged_metadata_are_fail_closed() {
    let original = fixture("ASL.l", 0xe180, (1, 0, 256), (0x2000, 0x2000), 24);
    let case: Case = serde_json::from_value(original.clone()).unwrap();
    let contract = ordinary::derive(&case).unwrap().unwrap();
    let access = BusAccess {
        logical_address: 0x1004,
        bus_address: 0x1004,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    };
    assert!(contract.permits(access, false, 0));
    assert!(!contract.permits(
        BusAccess {
            kind: AccessKind::Data,
            ..access
        },
        false,
        0
    ));
    assert!(!contract.permits(access, true, 0));
    for edit in 0..7 {
        let mut value = original.clone();
        match edit {
            0 => value["cohort"] = json!("ASR.l"),
            1 => value["cohort"] = json!("ASL.b"),
            2 => value["initial"]["prefetch"][0] = json!(0xe1d0),
            3 => value["final"]["d1"] = json!(256),
            4 => value["transactions"][1]["duration_cycles"] = json!(18),
            5 => {
                value["transactions"].as_array_mut().unwrap().swap(0, 1);
            }
            _ => {
                value["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(0x1004));
            }
        }
        assert!(check(value).is_err(), "edit{edit}");
    }
    for (cohort, op) in [("EXT.w", 0x4890), ("EXT.l", 0x4880), ("SWAP", 0x4850)] {
        assert!(check(fixture(cohort, op, (0, 0, 0), (0x2000, 0x2004), 4)).is_err());
    }
}

#[test]
fn register_idle_partition_changes_preserve_one_transfer_boundary() {
    let mut value = fixture("ASL.l", 0xe180, (1, 0, 256), (0xa000, 0xa000), 24);
    let phases = value["transactions"].as_array_mut().unwrap();
    phases.pop();
    for _ in 0..10 {
        phases.push(json!({"kind":"idle","duration_cycles":2}));
    }
    assert!(check(value.clone()).is_ok());
    value["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(check(value).is_err());
}

#[test]
fn ordinary_tail_wraps_external_bus_without_truncating_logical_pc() {
    let mut value = fixture("ASL.l", 0xe180, (1, 0, 256), (0x2000, 0x2000), 24);
    value["initial"]["pc"] = json!(0x02000000);
    value["final"]["pc"] = json!(0x02000002);
    for state in ["initial", "final"] {
        for byte in value[state]["ram"].as_array_mut().unwrap() {
            byte[0] = json!((byte[0].as_u64().unwrap() - 0x1000 + 0x01fffffc) & 0xffffff);
        }
    }
    value["transactions"][0]["address_bus"] = json!(0);
    assert!(check(value.clone()).is_ok());
    value["final"]["pc"] = json!(2);
    assert!(check(value).is_err());
}
