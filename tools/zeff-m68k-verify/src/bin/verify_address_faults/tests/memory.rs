use super::*;
use crate::witness::WitnessResult;

fn fixture(pc_relative: bool) -> Value {
    let mut v = super::updates::fixture(3, false, 0x4e71);
    let opcode = if pc_relative { 0x32ba_u16 } else { 0x3298 };
    let operand = if pc_relative { 0x1007_u32 } else { 0x1234_5679 };
    let ssw = (opcode & !31) | if pc_relative { 18 } else { 17 };
    v["initial"]["prefetch"] = json!([opcode, if pc_relative { 5 } else { 0x4e71 }]);
    if pc_relative {
        v["final"]["a0"] = v["initial"]["a0"].clone();
        v["transactions"][0]["address_bus"] = json!(operand & !1);
        v["transactions"][0]["function_code"] = json!(2);
    }
    for (at, word) in [
        (5, opcode),
        (6, operand as u16),
        (7, ssw),
        (8, (operand >> 16) as u16),
    ] {
        v["transactions"][at]["data_bus"] = json!(word);
    }
    for state in ["initial", "final"] {
        for row in v[state]["ram"].as_array_mut().unwrap() {
            let address = row[0].as_u64().unwrap();
            match address {
                0x1000 => row[1] = json!((opcode >> 8) as u8),
                0x1001 => row[1] = json!(opcode as u8),
                0x1002 if pc_relative => row[1] = json!(0),
                0x1003 if pc_relative => row[1] = json!(5),
                _ => {}
            }
            if state == "final" {
                for (location, word) in [
                    (0x1ff8, opcode),
                    (0x1ff6, operand as u16),
                    (0x1ff2, ssw),
                    (0x1ff4, (operand >> 16) as u16),
                ] {
                    if address == location {
                        row[1] = json!((word >> 8) as u8);
                    }
                    if address == location + 1 {
                        row[1] = json!(word as u8);
                    }
                }
            }
        }
        if pc_relative {
            v[state]["ram"]
                .as_array_mut()
                .unwrap()
                .extend([json!([0x1004, 0x4e]), json!([0x1005, 0x71])]);
        }
    }
    if pc_relative {
        v["transactions"].as_array_mut().unwrap().insert(0,
            json!({"kind":"read","duration_cycles":4,"function_code":2,"address_bus":0x1004,"data_bus":0x4e71,"uds":1,"lds":1}));
        v["reference_total_cycles"] = json!(62);
    }
    v
}

#[test]
fn memory_source_update_fault_enters_before_destination_preparation() {
    let v = fixture(false);
    assert_eq!(
        witness::run(&serde_json::from_value(v.clone()).unwrap()),
        Ok(WitnessResult::Frame)
    );
    let mut wrong = v;
    wrong["final"]["a0"] = wrong["initial"]["a0"].clone();
    assert!(witness::run(&serde_json::from_value(wrong).unwrap()).is_err());
}

#[test]
fn pc_relative_memory_source_requires_program_function_code() {
    let v = fixture(true);
    assert_eq!(
        witness::run(&serde_json::from_value(v.clone()).unwrap()),
        Ok(WitnessResult::Frame)
    );
    let mut wrong = v;
    wrong["transactions"][1]["function_code"] = json!(1);
    assert!(witness::run(&serde_json::from_value(wrong).unwrap()).is_err());
}

fn extended_fixture(opcode: u16, extension: u16, source: &[u16], next: u16) -> Case {
    let mut value = case_value();
    value["initial"]["prefetch"] = json!([opcode, extension]);
    value["initial"]["a0"] = json!(0x2000);
    value["initial"]["a1"] = json!(0x1001);
    let ram = value["initial"]["ram"].as_array_mut().unwrap();
    for (address, word) in source
        .iter()
        .enumerate()
        .map(|(i, word)| (0x2000 + i * 2, *word))
        .chain([(0x1004, next)])
    {
        ram.extend([
            json!([address, word >> 8]),
            json!([address + 1, word & 255]),
        ]);
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn extended_destination_index_uses_published_source_address_register() {
    let case = extended_fixture(0x3398, 0x8800, &[0x8001], 0x7201);
    let profile = profile::Profile::new(&case).unwrap();
    assert_eq!(profile.operand, 0x3003);
    assert_eq!(profile.phase.a[0], 0x2002);
    assert_eq!(profile.phase.status_register(), 0x18);
    assert_eq!(profile.marker_word, Some(0x8001));
    assert_eq!((profile.marker, profile.prefix_clocks), (3, 10));
    assert!(matches!(
        profile.prefix.as_slice(),
        [
            profile::Prefix::DataRead(0x2000, false),
            profile::Prefix::Idle(2),
            profile::Prefix::Read(0x1004)
        ]
    ));
    let mut changed = case;
    changed.initial.a0 += 2;
    changed.initial.ram.extend([(0x2002, 0x80), (0x2003, 1)]);
    assert_eq!(profile::Profile::new(&changed).unwrap().operand, 0x3005);
}

#[test]
fn absolute_long_destination_reads_actual_low_before_low_half_flagged_write() {
    let mut case = extended_fixture(0x23d8, 0x1234, &[0x8000, 1], 0x5679);
    let profile = profile::Profile::new(&case).unwrap();
    assert_eq!(profile.operand, 0x1234_5679);
    assert_eq!(profile.phase.a[0], 0x2004);
    assert_eq!(profile.phase.status_register(), 0x10);
    assert_eq!(profile.marker_word, Some(0x8000));
    assert_eq!((profile.marker, profile.prefix_clocks), (3, 12));
    assert!(matches!(
        profile.prefix.as_slice(),
        [
            profile::Prefix::DataRead(0x2000, false),
            profile::Prefix::DataRead(0x2002, false),
            profile::Prefix::Read(0x1004)
        ]
    ));
    for (address, byte) in &mut case.initial.ram {
        if *address == 0x1005 {
            *byte = 0x7b;
        }
    }
    assert_eq!(profile::Profile::new(&case).unwrap().operand, 0x1234_567b);
}

#[test]
fn extended_long_write_flags_distinguish_full_value_from_low_half() {
    for (high, low, full_flags, low_flags) in [(0x8000, 1, 0x18, 0x10), (1, 0x8000, 0x10, 0x18)] {
        for (opcode, extension) in [(0x2358, 0), (0x2398, 0x4800), (0x21d8, 0x1001), (0x23d8, 0)] {
            let case = extended_fixture(opcode, extension, &[high, low], 0x1001);
            let profile = profile::Profile::new(&case).unwrap();
            assert_eq!(
                profile.phase.status_register(),
                if opcode == 0x23d8 {
                    low_flags
                } else {
                    full_flags
                }
            );
            assert_eq!(profile.marker_word, Some(high));
            assert_eq!(profile.operand, 0x1001);
        }
    }
}
