use super::*;
use crate::witness::WitnessResult;

fn fixture(mode: u16, next: u16) -> Value {
    let mut v = super::updates::fixture(mode, true, next);
    let opcode = 0x303c | mode << 6;
    let old_marker = usize::from(mode == 4);
    let ird = if mode == 4 { next } else { opcode };
    v["initial"]["prefetch"] = json!([opcode, 0x1234]);
    v["transactions"][old_marker + 2]["data_bus"] = json!(0x1006);
    v["transactions"][old_marker + 5]["data_bus"] = json!(ird);
    v["transactions"][old_marker + 7]["data_bus"] = json!((ird & !31) | 1);
    if mode == 4 {
        v["transactions"][0]["address_bus"] = json!(0x1006);
    }
    for state in ["initial", "final"] {
        v[state]["d0"] = json!(0xaabb_789a_u32);
        for row in v[state]["ram"].as_array_mut().unwrap() {
            let address = row[0].as_u64().unwrap();
            match address {
                0x1001 => row[1] = json!(opcode as u8),
                0x1002 => row[1] = json!(0x12),
                0x1003 => row[1] = json!(0x34),
                0x1004 | 0x1005 if mode == 4 => row[0] = json!(address + 2),
                0x1fff if state == "final" => row[1] = json!(6),
                0x1ff8 if state == "final" => row[1] = json!((ird >> 8) as u8),
                0x1ff9 if state == "final" => row[1] = json!(ird as u8),
                0x1ff2 if state == "final" => row[1] = json!(((ird & !31) >> 8) as u8),
                0x1ff3 if state == "final" => row[1] = json!(((ird & !31) | 1) as u8),
                _ => {}
            }
        }
        v[state]["ram"].as_array_mut().unwrap().extend([
            json!([0x1004, (next >> 8) as u8]),
            json!([0x1005, next as u8]),
        ]);
    }
    v["transactions"].as_array_mut().unwrap().insert(
        0,
        json!({
            "kind":"read", "duration_cycles":4, "address_bus":0x1004,
            "function_code":2, "data_bus":next, "uds":1, "lds":1
        }),
    );
    v["reference_total_cycles"] = json!(62 + if mode == 4 { 4 } else { 0 });
    v
}

#[test]
fn immediate_prefix_and_prewrite_retention_match_full_frames() {
    for mode in [3, 4] {
        let v = fixture(mode, 0x4e71);
        assert_eq!(
            witness::run(&serde_json::from_value(v.clone()).unwrap()),
            Ok(WitnessResult::Frame)
        );
        let mut wrong = v;
        wrong["initial"]["prefetch"][1] = json!(0x8000);
        assert!(witness::run(&serde_json::from_value(wrong).unwrap()).is_err());
    }
}

#[test]
fn actual_unknown_next_is_a_consumed_immediate_prewrite_gap() {
    let v = fixture(4, 0x4afc);
    assert_eq!(
        witness::run(&serde_json::from_value(v.clone()).unwrap()),
        Ok(WitnessResult::CaptureGap)
    );
    let report = intake::verify_reader(std::io::Cursor::new(stream(v, 1))).unwrap();
    assert_eq!(
        (
            report.selected,
            report.passed,
            report.capture_gaps,
            report.failures
        ),
        (1, 0, 1, 0)
    );
}
