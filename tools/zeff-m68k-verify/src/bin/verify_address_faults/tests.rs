use super::*;
use serde_json::{Value, json};
use zeff_m68k_verify::reference::Case;

#[path = "tests/cmp.rs"]
mod cmp;
#[path = "tests/cmpa.rs"]
mod cmpa;
#[path = "tests/cmpi.rs"]
mod cmpi;
#[path = "tests/status.rs"]
mod status;

#[path = "tests/address_arithmetic.rs"]
mod address_arithmetic;

#[path = "tests/data_arithmetic.rs"]
mod data_arithmetic;

fn case_value() -> Value {
    let mut initial = serde_json::Map::new();
    for name in [
        "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "a0", "a1", "a2", "a3", "a4", "a5", "a6",
        "usp", "ssp",
    ] {
        initial.insert(name.into(), json!(0));
    }
    initial.insert("d0".into(), json!(0x1234));
    initial.insert("a0".into(), json!(0x12345679_u32));
    initial.insert("ssp".into(), json!(0x2000));
    initial.insert("sr".into(), json!(0x001f));
    initial.insert("pc".into(), json!(0x1004));
    initial.insert("prefetch".into(), json!([0x3080, 0x4e71]));
    let ram = vec![
        (12, 0),
        (13, 0),
        (14, 0x30),
        (15, 0),
        (0x1000, 0x30),
        (0x1001, 0x80),
        (0x1002, 0x4e),
        (0x1003, 0x71),
        (0x3000, 0x4e),
        (0x3001, 0x71),
        (0x3002, 0x70),
        (0x3003, 1),
    ];
    initial.insert("ram".into(), json!(ram));
    let mut expected = initial.clone();
    expected.insert("ssp".into(), json!(0x1ff2));
    expected.insert("sr".into(), json!(0x2010));
    expected.insert("pc".into(), json!(0x3004));
    expected.insert("prefetch".into(), json!([0x4e71, 0x7001]));
    let writes = [
        (0x1ffe, 0x1004_u16),
        (0x1ffa, 0x0010),
        (0x1ffc, 0),
        (0x1ff8, 0x3080),
        (0x1ff6, 0x5679),
        (0x1ff2, 0x3081),
        (0x1ff4, 0x1234),
    ];
    let mut final_ram = ram;
    for (address, word) in writes {
        final_ram.push((address, (word >> 8) as u8));
        final_ram.push((address + 1, word as u8));
    }
    expected.insert("ram".into(), json!(final_ram));
    let mut phases = vec![
        json!({"kind":"write_address_error", "duration_cycles":4,
        "function_code":1, "address_bus":0x345678, "data_bus":0x1234, "uds":1, "lds":1}),
        json!({"kind":"idle", "duration_cycles":8}),
    ];
    for (address, word) in writes {
        phases.push(
            json!({"kind":"write", "duration_cycles":4, "function_code":5,
            "address_bus":address, "data_bus":word, "uds":1, "lds":1}),
        );
    }
    for (index, address, word, function_code) in [
        (9, 12, 0, 5),
        (10, 14, 0x3000, 5),
        (11, 0x3000, 0x4e71, 6),
        (13, 0x3002, 0x7001, 6),
    ] {
        if index == 13 {
            phases.push(json!({"kind":"idle", "duration_cycles":2}));
        }
        phases.push(
            json!({"kind":"read", "duration_cycles":4, "function_code":function_code,
            "address_bus":address, "data_bus":word, "uds":1, "lds":1}),
        );
    }
    json!({"cohort":"MOVE.w", "name":"synthetic odd word store", "initial":initial,
        "final":expected, "reference_total_cycles":58, "transactions":phases,
        "normalization_exclusion":"address_fault"})
}

fn stream(mut value: Value, footer: u64) -> String {
    value["type"] = json!("case");
    let header = json!({"type":"header", "schema":1, "format":intake::FORMAT,
        "source_commit":intake::COMMIT, "cohorts":["MOVE.b","MOVE.w","MOVE.l","MOVEA.w","MOVEA.l"],
        "claim":"reference", "pc_convention":"raw next-prefetch PC retained; no PC normalization",
        "clock_convention":"record total and ordered transaction durations retained separately; no synthesized timeline",
        "ram_convention":"16-bit source words retained as big-endian byte pairs"});
    format!(
        "{header}\n{value}\n{}\n",
        json!({"type":"complete","exported_records":footer})
    )
}

#[test]
fn executes_literal_full_frame_and_never_seeds_missing_reads() {
    assert!(witness::run(&serde_json::from_value::<Case>(case_value()).unwrap()).is_ok());
    for index in [0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13] {
        let mut bad = case_value();
        bad["transactions"][index]["data_bus"] = json!(0xffff);
        assert!(
            witness::run(&serde_json::from_value(bad).unwrap()).is_err(),
            "phase{index}"
        );
    }
    let mut bad = case_value();
    bad["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row[0] != 12);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = case_value();
    bad["final"]["pc"] = json!(0x3006);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = case_value();
    bad["final"]["prefetch"] = json!([0x7001, 0x4e71]);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
}

#[test]
fn read_address_marker_is_detection_without_a_fabricated_operand_value() {
    let mut value = case_value();
    value["initial"]["prefetch"][0] = json!(0x3010);
    value["final"]["sr"] = json!(0x201f);
    value["transactions"][0]["kind"] = json!("read_address_error");
    value["transactions"][0]["data_bus"] = json!(0xffff);
    for (index, word) in [(2, 0x1002), (3, 0x001f), (5, 0x3010), (7, 0x3011)] {
        value["transactions"][index]["data_bus"] = json!(word);
    }
    for state in ["initial", "final"] {
        for row in value[state]["ram"].as_array_mut().unwrap() {
            match row[0].as_u64().unwrap() {
                0x1001 => row[1] = json!(0x10),
                0x1ffb => row[1] = json!(0x1f),
                0x1ff9 => row[1] = json!(0x10),
                0x1ff3 => row[1] = json!(0x11),
                0x1fff => row[1] = json!(0x02),
                _ => {}
            }
        }
    }
    let result = witness::run(&serde_json::from_value(value).unwrap());
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn validates_marker_direction_timing_suffix_and_memory_shape() {
    for (index, field, value) in [
        (0, "kind", json!("read_address_error")),
        (0, "address_bus", json!(0)),
        (0, "function_code", json!(5)),
        (0, "duration_cycles", json!(8)),
        (1, "duration_cycles", json!(4)),
        (12, "duration_cycles", json!(4)),
    ] {
        let mut bad = case_value();
        bad["transactions"][index][field] = value;
        assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    let mut bad = case_value();
    bad["transactions"].as_array_mut().unwrap().pop();
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = case_value();
    bad["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .push(json!([12, 0]));
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
}

#[test]
fn intake_rejects_malformed_and_keeps_failures_out_of_exclusions() {
    let valid = stream(case_value(), 1);
    let report = intake::verify_reader(std::io::Cursor::new(&valid)).unwrap();
    assert_eq!(
        (
            report.records,
            report.selected,
            report.passed,
            report.failures
        ),
        (1, 1, 1, 0)
    );
    assert!(!report.full_coverage());
    let mut failed = case_value();
    failed["final"]["d0"] = json!(0);
    let report = intake::verify_reader(std::io::Cursor::new(stream(failed, 1))).unwrap();
    assert_eq!(
        (
            report.selected,
            report.selected_trace,
            report.passed,
            report.failures
        ),
        (1, 0, 0, 1)
    );
    let mut traced = case_value();
    traced["initial"]["sr"] = json!(0x801f);
    traced["normalization_exclusion"] = json!("trace");
    let report = intake::verify_reader(std::io::Cursor::new(stream(traced, 1))).unwrap();
    assert_eq!(
        (
            report.selected,
            report.selected_trace,
            report.passed,
            report.failures
        ),
        (0, 1, 0, 0)
    );
    for (field, value) in [
        ("normalization_exclusion", json!(null)),
        ("cohort", json!("MOVE.l")),
        ("reference_total_cycles", json!(57)),
        ("unrecognized", json!(true)),
    ] {
        let mut bad = case_value();
        bad[field] = value;
        assert!(intake::verify_reader(std::io::Cursor::new(stream(bad, 1))).is_err());
    }
    assert!(intake::verify_reader(std::io::Cursor::new(stream(case_value(), 2))).is_err());
    assert!(intake::verify_reader(std::io::Cursor::new(format!("{valid}{{}}\n"))).is_err());
    assert!(intake::verify_reader(std::io::Cursor::new(vec![b' '; intake::MAX_LINE + 1])).is_err());
    let truncated = valid.lines().take(2).collect::<Vec<_>>().join("\n");
    assert!(intake::verify_reader(std::io::Cursor::new(truncated)).is_err());
}

#[test]
fn receipt_requires_every_source_pin_export_hash_and_exclusion_count() {
    let report = intake::verify_reader(std::io::Cursor::new(stream(case_value(), 1))).unwrap();
    let cohorts: Vec<_> = ["MOVE.b", "MOVE.w", "MOVE.l", "MOVEA.w", "MOVEA.l"]
        .iter()
        .map(|name| {
            let selected = *name == "MOVE.w";
            json!({"cohort":name, "raw_sha256":intake::source_hash(name).unwrap(),
            "total":u64::from(selected), "exported":u64::from(selected), "eligible":0,
            "excluded":if selected {json!({"address_fault":1})} else {json!({})}})
        })
        .collect();
    let receipt = json!({"schema":1, "format":intake::FORMAT, "source_commit":intake::COMMIT,
        "source":format!("https://raw.githubusercontent.com/SingleStepTests/m68000/{}/",intake::COMMIT),
        "license":"MIT", "license_sha256":"9b9e332ee11019f131e945d40e20ab0c48b52921754dc03fe62cc85b579e6c61",
        "readme_sha256":"7f36eed9cb93f061d13bfd18dc310c9bda7b5f3ac96b6d0dfe292e00612c46b6",
        "claim":"reference", "export_sha256":report.hash, "exported_records":1, "cohorts":cohorts});
    assert!(
        intake::verify_receipt(serde_json::from_value(receipt.clone()).unwrap(), &report).is_ok()
    );
    for field in [
        "source_commit",
        "source",
        "license_sha256",
        "readme_sha256",
        "export_sha256",
    ] {
        let mut bad = receipt.clone();
        bad[field] = json!("wrong");
        assert!(intake::verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
    }
    for index in 0..5 {
        let mut bad = receipt.clone();
        bad["cohorts"][index]["raw_sha256"] = json!("wrong");
        assert!(intake::verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
    }
    for field in ["total", "exported", "eligible"] {
        let mut bad = receipt.clone();
        bad["cohorts"][1][field] = json!(9);
        assert!(intake::verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
    }
    let mut bad = receipt;
    bad["cohorts"][1]["excluded"]["address_fault"] = json!(0);
    assert!(intake::verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
}

#[path = "tests/updates.rs"]
mod updates;

#[path = "tests/extensions.rs"]
mod extensions;

#[path = "tests/movea.rs"]
mod movea;

#[path = "tests/register_moves.rs"]
mod register_moves;

#[path = "tests/immediate.rs"]
mod immediate;
#[path = "tests/memory.rs"]
mod memory;

#[path = "tests/tst.rs"]
mod tst;
