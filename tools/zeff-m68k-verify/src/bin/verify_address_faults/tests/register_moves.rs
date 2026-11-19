use super::*;
use crate::witness::WitnessResult;

fn fixture(mode: u16, source: u16, next: u16) -> Value {
    let mut v = super::updates::fixture(mode, true, next);
    let opcode = 0x3000 | mode << 6 | 8 | source;
    let value = if source == 0 { 0x1234_5679_u32 } else { 0x1234 };
    let marker = usize::from(mode == 4);
    let ird = if mode == 4 { next } else { opcode };
    v["initial"]["prefetch"] = json!([opcode, next]);
    for state in ["initial", "final"] {
        v[state]["d0"] = json!(0x8765_8000_u32);
        if source == 1 {
            v[state]["a1"] = json!(value);
        }
        for row in v[state]["ram"].as_array_mut().unwrap() {
            match row[0].as_u64().unwrap() {
                0x1001 => row[1] = json!(opcode as u8),
                0x1ff8 if state == "final" => row[1] = json!((ird >> 8) as u8),
                0x1ff9 if state == "final" => row[1] = json!(ird as u8),
                _ => {}
            }
        }
    }
    v["transactions"][marker]["data_bus"] = json!(value as u16);
    v["transactions"][marker + 5]["data_bus"] = json!(ird);
    v
}

#[test]
fn address_source_and_destination_alias_use_the_old_source_value() {
    for mode in [3, 4] {
        for source in [0, 1] {
            let v = fixture(mode, source, 0x4e71);
            assert_eq!(
                witness::run(&serde_json::from_value(v).unwrap()),
                Ok(WitnessResult::Frame)
            );
        }
    }
    let mut bad = fixture(4, 1, 0x4e71);
    bad["initial"]["a1"] = json!(0x8000);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
}

#[test]
fn unknown_promoted_word_retains_address_source_consumption_without_entry() {
    for source in [0, 1] {
        let v = fixture(4, source, 0x4afc);
        assert_eq!(
            witness::run(&serde_json::from_value(v.clone()).unwrap()),
            Ok(WitnessResult::CaptureGap)
        );
        let r = intake::verify_reader(std::io::Cursor::new(stream(v, 1))).unwrap();
        assert_eq!(
            (r.selected, r.passed, r.capture_gaps, r.failures),
            (1, 0, 1, 0)
        );
    }
}
