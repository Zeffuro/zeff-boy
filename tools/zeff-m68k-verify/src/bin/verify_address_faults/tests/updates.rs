use super::*;
use crate::witness::WitnessResult;

pub(super) fn fixture(mode: u16, write: bool, queued: u16) -> Value {
    let mut v = case_value();
    let opcode = 0x3000 | if write { mode << 6 } else { mode << 3 };
    let operand = if mode == 4 {
        0x1234_5677_u32
    } else {
        0x1234_5679
    };
    let phase_an = if mode == 4 {
        operand
    } else if write {
        0x1234_5679
    } else {
        0x1234_567b
    };
    let ird = if mode == 4 && write { queued } else { opcode };
    let sr = if write { 0x10 } else { 0x1f };
    let pc = if mode == 4 || write { 0x1004 } else { 0x1002 };
    let words = [
        pc,
        sr,
        0,
        ird,
        operand as u16,
        (ird & !31) | if write { 1 } else { 17 },
        (operand >> 16) as u16,
    ];
    let locations = [0x1ffe, 0x1ffa, 0x1ffc, 0x1ff8, 0x1ff6, 0x1ff2, 0x1ff4];
    v["initial"]["prefetch"] = json!([opcode, queued]);
    v["final"]["sr"] = json!(sr | 0x2000);
    v["final"]["a0"] = json!(phase_an);
    v["transactions"][0]["kind"] = json!(if write {
        "write_address_error"
    } else {
        "read_address_error"
    });
    v["transactions"][0]["address_bus"] = json!(operand & 0xfffffe);
    v["transactions"][0]["data_bus"] = json!(if write { 0x1234 } else { 0xffff });
    for (index, word) in words.into_iter().enumerate() {
        v["transactions"][index + 2]["data_bus"] = json!(word);
    }
    for state in ["initial", "final"] {
        for row in v[state]["ram"].as_array_mut().unwrap() {
            let address = row[0].as_u64().unwrap();
            if address == 0x1001 {
                row[1] = json!(opcode as u8);
            }
            if address == 0x1002 {
                row[1] = json!((queued >> 8) as u8);
            }
            if address == 0x1003 {
                row[1] = json!(queued as u8);
            }
            if state == "final" {
                for (location, word) in locations.into_iter().zip(words) {
                    if address == location {
                        row[1] = json!((word >> 8) as u8);
                    }
                    if address == location + 1 {
                        row[1] = json!(word as u8);
                    }
                }
            }
        }
    }
    if mode == 4 {
        let prefix = if write {
            for state in ["initial", "final"] {
                v[state]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .extend([json!([0x1004, 0x74]), json!([0x1005, 0x80])]);
            }
            json!({"kind":"read","duration_cycles":4,"address_bus":0x1004,"function_code":2,"data_bus":0x7480,"uds":1,"lds":1})
        } else {
            json!({"kind":"idle","duration_cycles":2})
        };
        v["transactions"].as_array_mut().unwrap().insert(0, prefix);
    }
    v["reference_total_cycles"] = json!(
        58 + if mode != 4 {
            0
        } else if write {
            4
        } else {
            2
        }
    );
    v
}

#[test]
fn postincrement_and_predecrement_full_frames_use_the_right_prefix_and_latches() {
    for mode in [3, 4] {
        for write in [false, true] {
            let value = fixture(mode, write, 0x4e71);
            let result = witness::run(&serde_json::from_value(value).unwrap());
            assert_eq!(result, Ok(WitnessResult::Frame), "mode{mode}/write{write}");
        }
    }
    for queued in [0x2970, 0x6b5f] {
        let result = witness::run(&serde_json::from_value(fixture(4, true, queued)).unwrap());
        assert_eq!(result, Ok(WitnessResult::Frame));
    }
}

#[test]
fn unknown_last_write_class_is_a_verified_consumed_gap_without_entry() {
    let value = fixture(4, true, 0x4afc);
    let result = witness::run(&serde_json::from_value(value.clone()).unwrap());
    assert_eq!(result, Ok(WitnessResult::CaptureGap));
    let report = intake::verify_reader(std::io::Cursor::new(stream(value, 1))).unwrap();
    assert_eq!(
        (
            report.selected,
            report.passed,
            report.capture_gaps,
            report.failures
        ),
        (1, 0, 1, 0)
    );
    for field in ["data_bus", "function_code", "address_bus"] {
        let mut bad = fixture(4, true, 0x4afc);
        bad["transactions"][0][field] = json!(0);
        assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    let mut bad = fixture(4, true, 0x4afc);
    bad["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row[0] != 0x1004);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
}

#[test]
fn capture_family_masks_exclude_reserved_eas_and_privileged_unknowns() {
    for word in [
        0x2970, 0x6b5f, 0x1234, 0x2040, 0x307c, 0x4e71, 0x70ff, 0x6000,
    ] {
        assert!(crate::profile::known(word));
    }
    for word in [
        0x103d, 0x1040, 0x1008, 0x3ffc, 0x4e70, 0x4e72, 0x4afc, 0x7100, 0xa000, 0xf000, 0x4afc,
        0xc106, 0x5248, 0x5b1a, 0x4590, 0xcf0e,
    ] {
        assert!(!crate::profile::known(word), "{word:04x}");
    }
}
