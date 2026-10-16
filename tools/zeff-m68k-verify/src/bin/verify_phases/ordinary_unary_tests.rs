use super::stack_status_tests::{case, transfer, word};
use crate::*;
use serde_json::{Value, json};

fn check(value: Value) -> Result<(), String> {
    let case = serde_json::from_value(value).unwrap();
    if !format::transfer_metadata(&case) {
        return Err("cohort metadata differs".into());
    }
    run(&case)
}

#[test]
fn unary_register_edges_independently_cover_sticky_z_x_and_minimum_signed() {
    for (family, base) in [("NEG", 0x4400), ("NEGX", 0x4000), ("NOT", 0x4600)] {
        for size in 0..3 {
            if family == "NEGX" && size == 2 {
                continue;
            }
            let mask = if size == 2 {
                u32::MAX
            } else {
                (1u32 << (8 << size)) - 1
            };
            let sign = 1u32 << ((8 << size) - 1);
            let suffix = ["b", "w", "l"][size];
            let vectors = match family {
                "NEG" => vec![
                    (0, 0, 0x2010, 0x2004),
                    (1, mask, 0x2000, 0x2019),
                    (sign, sign, 0x2000, 0x201b),
                ],
                "NEGX" => vec![
                    (0, 0, 0x2004, 0x2004),
                    (0, 0, 0x2000, 0x2000),
                    (0, mask, 0x2014, 0x2019),
                    (sign, sign, 0x2004, 0x201b),
                ],
                _ => vec![(0, mask, 0x201f, 0x2018), (mask, 0, 0x201f, 0x2014)],
            };
            for (input, output, sr, final_sr) in vectors {
                let mut value = case(&format!("{family}.{suffix}"), base | ((size as u16) << 6));
                let upper = 0x12345678 & !mask;
                value["initial"]["d0"] = json!(upper | input);
                value["final"]["d0"] = json!(upper | output);
                value["initial"]["sr"] = json!(sr);
                value["final"]["sr"] = json!(final_sr);
                if size == 2 {
                    value["reference_total_cycles"] = json!(6);
                    value["transactions"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"kind":"idle","duration_cycles":2}));
                }
                assert!(
                    check(value).is_ok(),
                    "{family}/{suffix}/{input:08x}/{sr:04x}"
                );
            }
        }
    }
}

fn memory(family: &str, size: u16, mode: u16, user: bool, odd: bool) -> Value {
    let base = match family {
        "NEG" => 0x4400,
        "NEGX" => 0x4000,
        _ => 0x4600,
    };
    let suffix = ["b", "w", "l"][usize::from(size)];
    let (encoded_mode, r) = if mode == 8 { (7, 1) } else { (mode, 0) };
    let mut value = case(
        &format!("{family}.{suffix}"),
        base | (size << 6) | (encoded_mode << 3) | r,
    );
    let width = 1u32 << size;
    let mask = if size == 2 {
        u32::MAX
    } else {
        (1u32 << (width * 8)) - 1
    };
    let address = 0x2000 + u32::from(odd);
    let input = if family == "NOT" { 0 } else { 1 };
    let output = if family == "NEGX" { mask - 1 } else { mask };
    let sr = if user { 0x14 } else { 0x2014 };
    let fc = if user { 1 } else { 5 };
    let pfc = fc + 1;
    let base_address = match mode {
        4 => address + width,
        5 | 6 => address - 16,
        _ => address,
    };
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(base_address);
        value[state]["d1"] = json!(16);
        value[state]["sr"] = json!(sr);
        word(&mut value, state, 0x1008, 0x7777);
    }
    let extension = match mode {
        5 => 16,
        6 => 0x1000,
        7 => address as u16,
        8 => 0,
        _ => 0x1234,
    };
    value["initial"]["prefetch"][1] = json!(extension);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, extension);
    }
    let mut phases = Vec::new();
    if mode == 4 || mode == 6 {
        phases.push(json!({"kind":"idle","duration_cycles":2}));
    }
    let extensions = if mode == 8 { 2 } else { u32::from(mode >= 5) };
    if mode == 8 {
        for state in ["initial", "final"] {
            word(&mut value, state, 0x1004, address as u16);
        }
    }
    for n in 0..extensions {
        phases.push(transfer(
            "read",
            0x1004 + n * 2,
            if mode == 8 && n == 0 {
                address as u16
            } else if n == 0 {
                0x5678
            } else {
                0x9abc
            },
            pfc,
        ));
    }
    let bus_address = address & !1;
    if size == 0 {
        for state in ["initial", "final"] {
            let byte = if state == "initial" {
                input as u16
            } else {
                output as u16
            };
            word(
                &mut value,
                state,
                bus_address,
                if odd {
                    0x5500 | byte
                } else {
                    (byte << 8) | 0xaa
                },
            );
        }
        let mut read = transfer(
            "read",
            bus_address,
            (input as u16) << if odd { 0 } else { 8 },
            fc,
        );
        read["uds"] = json!(u32::from(!odd));
        read["lds"] = json!(u32::from(odd));
        phases.push(read);
    } else {
        for state in ["initial", "final"] {
            let data = if state == "initial" { input } else { output };
            word(
                &mut value,
                state,
                address,
                if size == 2 {
                    (data >> 16) as u16
                } else {
                    data as u16
                },
            );
            if size == 2 {
                word(&mut value, state, address + 2, data as u16);
            }
        }
        phases.push(transfer(
            "read",
            address,
            if size == 2 {
                (input >> 16) as u16
            } else {
                input as u16
            },
            fc,
        ));
        if size == 2 {
            phases.push(transfer("read", address + 2, input as u16, fc));
        }
    }
    let tail_address = 0x1004 + extensions * 2;
    let tail = if extensions == 0 {
        0x5678
    } else if extensions == 1 {
        0x9abc
    } else {
        0x7777
    };
    phases.push(transfer("read", tail_address, tail, pfc));
    if size == 2 {
        phases.push(transfer("write", address + 2, output as u16, fc));
        phases.push(transfer("write", address, (output >> 16) as u16, fc));
    } else {
        let mut write = transfer(
            "write",
            bus_address,
            if size == 0 && !odd {
                (output as u16) << 8
            } else {
                output as u16
            },
            fc,
        );
        if size == 0 {
            write["uds"] = json!(u32::from(!odd));
            write["lds"] = json!(u32::from(odd));
        }
        phases.push(write);
    }
    if mode == 3 {
        value["final"]["a0"] = json!(address + width);
    }
    if mode == 4 {
        value["final"]["a0"] = json!(address);
    }
    value["final"]["sr"] = json!(if family == "NOT" {
        (sr & !31) | 24
    } else {
        (sr & !31) | 25
    });
    value["final"]["pc"] = json!(0x1006 + extensions * 2);
    value["final"]["prefetch"] = json!([
        if extensions == 0 {
            extension
        } else if extensions == 1 {
            0x5678
        } else {
            0x9abc
        },
        tail
    ]);
    value["reference_total_cycles"] = json!(
        phases
            .iter()
            .map(|x| x["duration_cycles"].as_u64().unwrap())
            .sum::<u64>()
    );
    value["transactions"] = json!(phases);
    value
}

#[test]
fn ordinary_unary_memory_all_ea_widths_banks_and_byte_lanes() {
    for family in ["NEG", "NEGX", "NOT"] {
        for size in 0..3 {
            if family == "NEGX" && size == 2 {
                continue;
            }
            for mode in 2..=8 {
                for user in [false, true] {
                    assert!(
                        check(memory(family, size, mode, user, false)).is_ok(),
                        "{family}/{size}/{mode}/{user}"
                    );
                }
            }
            if size == 0 {
                assert!(check(memory(family, size, 2, false, true)).is_ok());
            }
        }
    }
}

#[test]
fn byte_a7_updates_active_stack_by_two_in_both_banks_and_lanes() {
    for user in [false, true] {
        for odd in [false, true] {
            for mode in [3, 4] {
                let mut value = memory("NEG", 0, mode, user, odd);
                let address = 0x2000 + u32::from(odd);
                let sp = if user { "usp" } else { "ssp" };
                let opcode = 0x4400 | (mode << 3) | 7;
                value["initial"]["prefetch"][0] = json!(opcode);
                value["final"]["a0"] = value["initial"]["a0"].clone();
                value["initial"][sp] = json!(if mode == 4 { address + 2 } else { address });
                value["final"][sp] = json!(if mode == 3 { address + 2 } else { address });
                for state in ["initial", "final"] {
                    word(&mut value, state, 0x1000, opcode);
                }
                assert!(check(value.clone()).is_ok());
                value["final"][sp] = json!(address + 1);
                assert!(check(value).is_err());
            }
        }
    }
}

#[test]
fn long_unary_tail_is_sampled_before_overlapping_low_and_high_writes() {
    let mut value = memory("NOT", 2, 2, false, false);
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x1004);
        value[state]["ram"]
            .as_array_mut()
            .unwrap()
            .retain(|x| !(0x2000..0x2004).contains(&x[0].as_u64().unwrap()));
    }
    word(&mut value, "initial", 0x1004, 0x5678);
    word(&mut value, "initial", 0x1006, 0x1234);
    word(&mut value, "final", 0x1004, 0xa987);
    word(&mut value, "final", 0x1006, 0xedcb);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x5678, 5),
        transfer("read", 0x1006, 0x1234, 5),
        transfer("read", 0x1004, 0x5678, 6),
        transfer("write", 0x1006, 0xedcb, 5),
        transfer("write", 0x1004, 0xa987, 5)
    ]);
    assert!(check(value.clone()).is_ok());
    value["final"]["prefetch"][1] = json!(0xa987);
    assert!(check(value).is_err());
}

#[test]
fn unary_forged_fc_lane_purpose_order_state_and_missing_ram_fail() {
    let original = memory("NEG", 2, 3, false, false);
    for edit in 0..8 {
        let mut value = original.clone();
        match edit {
            0 => value["transactions"][0]["function_code"] = json!(6),
            1 => value["transactions"][0]["lds"] = json!(0),
            2 => {
                value["transactions"].as_array_mut().unwrap().swap(3, 4);
            }
            3 => {
                value["transactions"].as_array_mut().unwrap().remove(0);
            }
            4 => value["final"]["a0"] = json!(0x2000),
            5 => value["final"]["usp"] = json!(4),
            6 => {
                value["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(0x2000));
            }
            _ => {
                value["final"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(0x1000));
            }
        }
        assert!(check(value).is_err(), "edit{edit}");
    }
    let case: Case = serde_json::from_value(original).unwrap();
    let c = ordinary::derive(&case).unwrap().unwrap();
    let access = BusAccess {
        logical_address: 0x2000,
        bus_address: 0x2000,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    };
    assert!(c.permits(access, false, 0));
    assert!(!c.permits(
        BusAccess {
            kind: AccessKind::Prefetch,
            ..access
        },
        false,
        0
    ));
    assert!(!c.permits(access, true, 0));
    assert!(!c.permits(
        BusAccess {
            bus_address: 0x2004,
            ..access
        },
        false,
        0
    ));
    for op in [0x4448, 0x447a, 0x447c] {
        let mut v = memory("NEG", 1, 2, false, false);
        v["initial"]["prefetch"][0] = json!(op);
        assert!(check(v).is_err());
    }
}
