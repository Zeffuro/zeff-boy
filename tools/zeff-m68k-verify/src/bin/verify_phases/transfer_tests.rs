use super::stack_status_tests::{case, transfer, word};
use crate::*;
use serde_json::{Value, json};

fn multiple(load: bool, long: bool, mask: u16, predecrement: bool) -> Value {
    let mode = if predecrement {
        4
    } else if load {
        3
    } else {
        2
    };
    let op = 0x4880 | if load { 1024 } else { 0 } | if long { 64 } else { 0 } | (mode << 3);
    let mut value = case(if long { "MOVEM.l" } else { "MOVEM.w" }, op);
    value["initial"]["prefetch"][1] = json!(mask);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, mask);
        value[state]["a0"] = json!(0x3000);
        value[state]["ssp"] = json!(0x9000);
        for index in 0..8 {
            value[state][format!("d{index}")] = json!(0x80000010u32 + index);
        }
        for index in 1..7 {
            value[state][format!("a{index}")] = json!(0x80000020u32 + index);
        }
    }
    let mut phases = vec![transfer("read", 0x1004, 0x5678, 6)];
    let width = if long { 4 } else { 2 };
    let mut address = 0x3000;
    for bit in 0..16 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let index = if predecrement { 15 - bit } else { bit };
        let key = if index < 8 {
            format!("d{index}")
        } else if index == 15 {
            "ssp".into()
        } else {
            format!("a{}", index - 8)
        };
        let data = value["initial"][&key].as_u64().unwrap() as u32;
        if load {
            let high = 0x8000 | index as u16;
            let low = 0x6000 | index as u16;
            for state in ["initial", "final"] {
                word(&mut value, state, address, high);
                if long {
                    word(&mut value, state, address + 2, low);
                }
            }
            phases.push(transfer("read", address, high, 5));
            if long {
                phases.push(transfer("read", address + 2, low, 5));
            }
            value["final"][key] = json!(if long {
                (u32::from(high) << 16) | u32::from(low)
            } else {
                i32::from(high as i16) as u32
            });
            address += width;
        } else {
            if predecrement {
                address -= width;
            }
            let high = if long {
                (data >> 16) as u16
            } else {
                data as u16
            };
            if predecrement && long {
                phases.push(transfer("write", address + 2, data as u16, 5));
                phases.push(transfer("write", address, high, 5));
            } else {
                phases.push(transfer("write", address, high, 5));
                if long {
                    phases.push(transfer("write", address + 2, data as u16, 5));
                }
            }
            word(&mut value, "final", address, high);
            if long {
                word(&mut value, "final", address + 2, data as u16);
            }
            if !predecrement {
                address += width;
            }
        }
    }
    if load {
        for state in ["initial", "final"] {
            word(&mut value, state, address, 0xabcd);
        }
        phases.push(transfer("read", address, 0xabcd, 5));
    }
    if load || predecrement {
        value["final"]["a0"] = json!(address);
    }
    phases.push(transfer("read", 0x1006, 0x9abc, 6));
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(phases.len() * 4);
    value["transactions"] = json!(phases);
    value
}

fn check(value: Value) -> Result<(), String> {
    run(&serde_json::from_value(value).unwrap())
}

#[test]
fn movem_empty_full_masks_sign_extension_and_base_aliases() {
    for long in [false, true] {
        for mask in [0, 0xffff, 1, 0x100, 0x8000, 0x5555] {
            for (load, predec) in [(true, false), (false, false), (false, true)] {
                assert!(
                    check(multiple(load, long, mask, predec)).is_ok(),
                    "{load}/{long}/{mask:04x}/{predec}"
                );
            }
        }
    }
}

#[test]
fn movem_forged_masks_aliases_discard_and_order_fail() {
    let original = multiple(true, true, 0x101, false);
    assert!(check(original.clone()).is_ok());
    for edit in 0..6 {
        let mut value = original.clone();
        match edit {
            0 => value["initial"]["prefetch"][1] = json!(1),
            1 => value["final"]["a0"] = json!(0x80086008u32),
            2 => {
                value["transactions"].as_array_mut().unwrap().remove(5);
            }
            3 => value["transactions"][5]["address_bus"] = json!(0x80086008u32 & 0xffffff),
            4 => value["cohort"] = json!("MOVEM.w"),
            _ => {
                value["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|x| x[0] != json!(0x3000));
            }
        }
        assert!(check(value).is_err(), "edit {edit}");
    }
    let mut value = multiple(false, true, 1, true);
    value["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(check(value).is_err());
}

#[test]
fn movem_discard_permission_is_finite_and_purpose_specific() {
    let value = multiple(true, false, 0, false);
    let case: Case = serde_json::from_value(value).unwrap();
    let contract = transfers::derive(&case).unwrap().unwrap();
    let access = BusAccess {
        logical_address: 0x3000,
        bus_address: 0x3000,
        function_code: zeff_m68k::bus::FunctionCode::SupervisorData,
        lanes: ByteLanes::Both,
        kind: AccessKind::Discard,
    };
    assert!(contract.permits(access, false, 1));
    assert!(!contract.permits(
        BusAccess {
            kind: AccessKind::Data,
            ..access
        },
        false,
        1
    ));
    assert!(!contract.permits(access, true, 1));
    assert!(!contract.permits(
        BusAccess {
            bus_address: 0x3002,
            ..access
        },
        false,
        1
    ));
    assert!(!contract.permits(access, false, 0));
}

#[test]
fn movem_pc_base_program_fc_and_write_after_sample_are_exact() {
    let mut value = case("MOVEM.w", 0x4cba);
    value["initial"]["prefetch"][1] = json!(1);
    for state in ["initial", "final"] {
        word(&mut value, state, 0x1002, 1);
        word(&mut value, state, 0x1004, 0x20);
        word(&mut value, state, 0x1008, 0x7777);
        word(&mut value, state, 0x1024, 0xfffe);
        word(&mut value, state, 0x1026, 0xdead);
    }
    value["final"]["d0"] = json!(0xfffffffeu32);
    value["final"]["pc"] = json!(0x100a);
    value["final"]["prefetch"] = json!([0x9abc, 0x7777]);
    value["reference_total_cycles"] = json!(20);
    value["transactions"] = json!([
        transfer("read", 0x1004, 0x20, 6),
        transfer("read", 0x1006, 0x9abc, 6),
        transfer("read", 0x1024, 0xfffe, 6),
        transfer("read", 0x1026, 0xdead, 6),
        transfer("read", 0x1008, 0x7777, 6)
    ]);
    assert!(check(value.clone()).is_ok());
    value["transactions"][2]["function_code"] = json!(5);
    assert!(check(value).is_err());
    let mut value = multiple(false, false, 1, false);
    value["initial"]["a0"] = json!(0x1004);
    value["final"]["a0"] = json!(0x1004);
    value["transactions"][1]["address_bus"] = json!(0x1004);
    // Remove the old final-only store witness when relocating this write.
    value["final"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|x| !matches!(x[0].as_u64(), Some(0x3000 | 0x3001)));
    word(&mut value, "final", 0x1004, 0x10);
    assert!(check(value.clone()).is_ok());
    value["final"]["prefetch"][0] = json!(0x10);
    assert!(check(value).is_err());
}

fn peripheral(long: bool, store: bool, odd: bool) -> Value {
    let opcode = 0x0108 | if long { 64 } else { 0 } | if store { 128 } else { 0 };
    let mut value = case(if long { "MOVEP.l" } else { "MOVEP.w" }, opcode);
    value["initial"]["prefetch"][1] = json!(0xfffe);
    for state in ["initial", "final"] {
        value[state]["a0"] = json!(0x3002 + u32::from(odd));
        value[state]["d0"] = json!(0x89abcdefu32);
        word(&mut value, state, 0x1002, 0xfffe);
        for n in 0..4 {
            word(&mut value, state, 0x3000 + n * 2, 0x55aa);
        }
    }
    let mut phases = vec![transfer("read", 0x1004, 0x5678, 6)];
    let count = if long { 4 } else { 2 };
    let mut result = 0;
    for n in 0..count {
        let byte = if store {
            ((0x89abcdefu32 >> (8 * (count - 1 - n))) & 255) as u16
        } else if odd {
            0xaa
        } else {
            0x55
        };
        let mut t = transfer(
            if store { "write" } else { "read" },
            0x3000 + n * 2,
            byte << if odd { 0 } else { 8 },
            5,
        );
        t["uds"] = json!(u32::from(!odd));
        t["lds"] = json!(u32::from(odd));
        phases.push(t);
        if store {
            word(
                &mut value,
                "final",
                0x3000 + n * 2,
                if odd {
                    0x5500 | byte
                } else {
                    (byte << 8) | 0xaa
                },
            );
        }
        result = (result << 8) | u32::from(byte);
    }
    if !store {
        value["final"]["d0"] = json!(if long { result } else { 0x89ab0000 | result });
    }
    phases.push(transfer("read", 0x1006, 0x9abc, 6));
    value["final"]["pc"] = json!(0x1008);
    value["final"]["prefetch"] = json!([0x5678, 0x9abc]);
    value["reference_total_cycles"] = json!(phases.len() * 4);
    value["transactions"] = json!(phases);
    value
}

#[test]
fn movep_sparse_both_lanes_word_upper_and_neighbors() {
    for long in [false, true] {
        for store in [false, true] {
            for odd in [false, true] {
                assert!(check(peripheral(long, store, odd)).is_ok());
            }
        }
    }
    for field in ["uds", "lds", "address_bus", "function_code", "data_bus"] {
        let mut value = peripheral(true, true, false);
        value["transactions"][1][field] = json!(u32::from(field == "lds"));
        assert!(check(value).is_err(), "{field}");
    }
    let mut value = peripheral(false, false, true);
    value["final"]["d0"] = json!(0xaaaa);
    assert!(check(value).is_err());
    let mut value = peripheral(true, true, false);
    word(&mut value, "final", 0x3000, 0x8900);
    assert!(check(value).is_err());
}
