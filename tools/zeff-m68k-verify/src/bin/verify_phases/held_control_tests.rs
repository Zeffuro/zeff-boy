use super::stack_status_tests::{case, exception, transfer, word};
use crate::*;
use serde_json::{Value, json};

fn check(value: Value) -> Result<(), String> {
    run(&serde_json::from_value(value).unwrap())
}

fn reset(supervisor: bool, traced: bool) -> Value {
    let mut value = case("RESET", 0x4e70);
    let sr = if supervisor { 0x2000 } else { 0 } | if traced { 0x8000 } else { 0 };
    for state in ["initial", "final"] {
        value[state]["sr"] = json!(sr);
    }
    if traced {
        value["normalization_exclusion"] = json!("trace");
    }
    if supervisor {
        value["reference_total_cycles"] = json!(132);
        value["transactions"] =
            json!([{"kind":"idle","duration_cycles":128},transfer("read",0x1004,0x5678,6)]);
    } else {
        exception(&mut value, 8, 0x1000, sr);
    }
    value
}

fn returning(rte: bool, supervisor: bool, traced: bool, restored: u16) -> Value {
    let mut value = case(
        if rte { "RTE" } else { "RTS" },
        if rte { 0x4e73 } else { 0x4e75 },
    );
    let sr = if supervisor { 0x2000 } else { 0 } | if traced { 0x8000 } else { 0 };
    for state in ["initial", "final"] {
        value[state]["sr"] = json!(sr);
        value[state]["ssp"] = json!(0x3000);
        value[state]["usp"] = json!(0x5000);
        word(&mut value, state, 0x4000, 0x1357);
        word(&mut value, state, 0x4002, 0x2468);
    }
    if traced {
        value["normalization_exclusion"] = json!("trace");
    }
    if rte && !supervisor {
        exception(&mut value, 8, 0x1000, sr);
        return value;
    }
    let sp = if supervisor { 0x3000 } else { 0x5000 };
    let offset = if rte { 2 } else { 0 };
    let fc = if supervisor { 5 } else { 1 };
    let mut phases = Vec::new();
    for state in ["initial", "final"] {
        if rte {
            word(&mut value, state, sp, restored);
        }
        word(&mut value, state, sp + offset, 0);
        word(&mut value, state, sp + offset + 2, 0x4000);
    }
    if rte {
        phases.push(transfer("read", sp, restored, 5));
    }
    phases.push(transfer("read", sp + offset, 0, fc));
    phases.push(transfer("read", sp + offset + 2, 0x4000, fc));
    let final_sr = if rte { restored & 0xa71f } else { sr };
    let pfc = if final_sr & 0x2000 != 0 { 6 } else { 2 };
    phases.push(transfer("read", 0x4000, 0x1357, pfc));
    phases.push(transfer("read", 0x4002, 0x2468, pfc));
    value["final"][if supervisor { "ssp" } else { "usp" }] = json!(sp + if rte { 6 } else { 4 });
    value["final"]["sr"] = json!(final_sr);
    value["final"]["pc"] = json!(0x4004);
    value["final"]["prefetch"] = json!([0x1357, 0x2468]);
    value["reference_total_cycles"] = json!(phases.len() * 4);
    value["transactions"] = json!(phases);
    value
}

#[test]
fn reset_supervisor_prefix_and_user_privilege_both_trace_states() {
    for supervisor in [false, true] {
        for trace in [false, true] {
            assert!(check(reset(supervisor, trace)).is_ok());
        }
    }
    for edit in 0..4 {
        let mut value = reset(true, false);
        match edit {
            0 => value["transactions"][0]["duration_cycles"] = json!(124),
            1 => value["final"]["sr"] = json!(0),
            2 => value["final"]["ssp"] = json!(6),
            _ => value["initial"]["prefetch"][0] = json!(0x4e71),
        }
        assert!(check(value).is_err());
    }
}

#[test]
fn returns_use_entry_stack_restored_fc_and_entry_trace() {
    for traced in [false, true] {
        for sr in [0, 0x2000, 0x8000, 0xa71f, 0xffff] {
            assert!(
                check(returning(true, true, traced, sr)).is_ok(),
                "{traced}/{sr:04x}"
            );
        }
        for supervisor in [false, true] {
            assert!(check(returning(false, supervisor, traced, 0)).is_ok());
            assert!(check(returning(true, false, traced, 0)).is_ok());
        }
    }
}

#[test]
fn return_forged_bank_frame_fc_queue_and_missing_witness_fail() {
    let original = returning(true, true, true, 0);
    for edit in 0..7 {
        let mut value = original.clone();
        match edit {
            0 => value["final"]["usp"] = json!(0x3006),
            1 => value["final"]["ssp"] = json!(0x3004),
            2 => value["transactions"][3]["function_code"] = json!(6),
            3 => value["transactions"].as_array_mut().unwrap().swap(1, 2),
            4 => value["final"]["prefetch"][0] = json!(0x2468),
            5 => {
                value["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(0x3000));
            }
            _ => value["final"]["sr"] = json!(0x2000),
        }
        assert!(check(value).is_err(), "edit{edit}");
    }
    let mut value = returning(false, false, false, 0);
    value["transactions"]
        .as_array_mut()
        .unwrap()
        .insert(0, transfer("read", 0x5000, 0, 1));
    assert!(check(value).is_err());
}

fn line(family: &str, opcode: u16, sr: u16) -> Value {
    let mut value = case(family, opcode);
    value["initial"]["sr"] = json!(sr);
    if sr & 0x8000 != 0 {
        value["normalization_exclusion"] = json!("trace");
    }
    exception(
        &mut value,
        if opcode >> 12 == 10 { 10 } else { 11 },
        0x1000,
        sr,
    );
    value
}

#[test]
fn lines_and_privilege_derive_finite_vector_saved_pc_and_suppressed_trace() {
    for (family, op) in [("ILLEGAL_LINEA", 0xa000), ("ILLEGAL_LINEF", 0xffff)] {
        for sr in [0, 0x2000, 0x8000, 0xa000] {
            assert!(check(line(family, op, sr)).is_ok());
        }
    }
    let original = line("ILLEGAL_LINEA", 0xa123, 0x8000);
    for edit in 0..7 {
        let mut value = original.clone();
        match edit {
            0 => value["transactions"][1]["data_bus"] = json!(0x1002),
            1 => value["transactions"][4]["address_bus"] = json!(44),
            2 => value["transactions"][1]["function_code"] = json!(1),
            3 => value["transactions"].as_array_mut().unwrap().swap(1, 3),
            4 => value["cohort"] = json!("ILLEGAL_LINEF"),
            5 => {
                value["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(40));
            }
            _ => {
                value["final"]["ram"].as_array_mut().unwrap().pop();
            }
        }
        assert!(check(value).is_err(), "edit{edit}");
    }
    let case: Case = serde_json::from_value(original).unwrap();
    let contract = held_control::derive(&case).unwrap().unwrap();
    let access = BusAccess {
        logical_address: 40,
        bus_address: 40,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorData,
        kind: AccessKind::Vector,
        lanes: ByteLanes::Both,
    };
    assert!(contract.permits(access, false, 4));
    assert!(!contract.permits(
        BusAccess {
            kind: AccessKind::Stack,
            ..access
        },
        false,
        4
    ));
    assert!(!contract.permits(access, true, 4));
    assert!(!contract.permits(access, false, 5));
}

#[test]
fn reset_adapter_rejects_unowned_mistimed_repeated_and_fetch_while_owned() {
    let case: Case = serde_json::from_value(reset(true, false)).unwrap();
    let mut bus = WitnessBus {
        memory: memory_map(&case.initial.ram).unwrap(),
        phases: &case.transactions,
        cursor: 0,
        elapsed: 0,
        error: None,
        trace_stack: None,
        control_stack: None,
        status_contract: None,
        transfer_contract: None,
        held_contract: held_control::derive(&case).unwrap(),
        ordinary_contract: None,
        short_vector: None,
        reset_owned: false,
        reset_pairs: 0,
        idle_remaining: 0,
        operand_data: false,
        pc_source: false,
    };
    assert!(bus.begin_reset_output(CpuClocks(0)).is_err());
    bus.idle(CpuClocks(0), 4);
    assert!(bus.begin_reset_output(CpuClocks(4)).is_ok());
    assert!(bus.begin_reset_output(CpuClocks(4)).is_err());
    let access = BusAccess {
        logical_address: 0x1004,
        bus_address: 0x1004,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    };
    assert!(bus.read(access, CpuClocks(4)).is_err());
    bus.idle(CpuClocks(4), 124);
    bus.end_reset_output(CpuClocks(128));
    assert!(bus.error.is_none());
    assert!(!bus.reset_owned);
    assert_eq!(bus.reset_pairs, 1);
    assert!(bus.begin_reset_output(CpuClocks(128)).is_err());
    assert!(bus.read(access, CpuClocks(128)).is_ok());
    bus.end_reset_output(CpuClocks(132));
    assert!(bus.error.is_some());
    let case: Case = serde_json::from_value(reset(false, false)).unwrap();
    bus.held_contract = held_control::derive(&case).unwrap();
    bus.elapsed = 4;
    assert!(bus.begin_reset_output(CpuClocks(4)).is_err());
}

#[test]
fn line_vector_reads_observe_prior_frame_alias_writes() {
    let mut value = line("ILLEGAL_LINEA", 0xa123, 0x2000);
    value["initial"]["ssp"] = json!(48);
    value["final"]["ssp"] = json!(42);
    value["final"]["pc"] = json!(0x2004);
    value["final"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| !(0x2ffa..=0x2fff).contains(&x[0].as_u64().unwrap()));
    for state in ["initial", "final"] {
        word(&mut value, state, 0x2000, 0x1357);
        word(&mut value, state, 0x2002, 0x2468);
    }
    word(&mut value, "final", 46, 0x1000);
    word(&mut value, "final", 42, 0x2000);
    word(&mut value, "final", 44, 0);
    value["transactions"][1]["address_bus"] = json!(46);
    value["transactions"][2]["address_bus"] = json!(42);
    value["transactions"][3]["address_bus"] = json!(44);
    value["transactions"][5]["data_bus"] = json!(0x2000);
    value["transactions"][6]["address_bus"] = json!(0x2000);
    value["transactions"][8]["address_bus"] = json!(0x2002);
    assert!(check(value.clone()).is_ok());
    value["transactions"][6]["address_bus"] = json!(0x4000);
    assert!(check(value).is_err());
}
