use super::*;

fn byte(address: u32, sr: u16, reason: Option<&str>) -> String {
    let mut value = case_value();
    value["cohort"] = json!("TST.b");
    value["initial"]["prefetch"] = json!([0x4a38, address]);
    value["initial"]["sr"] = json!(sr);
    value["transactions"] = json!([{"kind":"read", "duration_cycles":4,
        "function_code":5, "address_bus":address, "data_bus":0x8500, "uds":1, "lds":0}]);
    value["reference_total_cycles"] = json!(4);
    value["normalization_exclusion"] = json!(reason);
    let input = stream(value, 1);
    input.replace(
        "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
        "[\"TST.b\",\"TST.w\",\"TST.l\"]",
    )
}

#[test]
fn tst_intake_preserves_exact_low_supervisor_boundary_and_trace_priority() {
    for (address, reason) in [
        (0x288, Some("exception_or_low_supervisor_data")),
        (0x3fe, Some("exception_or_low_supervisor_data")),
        (0x400, None),
        (0x6fe, None),
    ] {
        let input = byte(address, 0x201f, reason);
        let report =
            intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Tst).unwrap();
        assert_eq!((report.records, report.selected), (1, 0));
        assert!(!report.full_coverage());
        assert!(intake::verify_reader(std::io::Cursor::new(input)).is_err());
        let wrong = if reason.is_some() {
            byte(address, 0x201f, None)
        } else {
            byte(address, 0x201f, Some("exception_or_low_supervisor_data"))
        };
        assert!(
            intake::verify_reader_mode(std::io::Cursor::new(wrong), intake::Mode::Tst).is_err()
        );
    }
    assert!(
        intake::verify_reader_mode(
            std::io::Cursor::new(byte(0x288, 0xa01f, Some("trace"))),
            intake::Mode::Tst
        )
        .is_ok()
    );
    assert!(
        intake::verify_reader_mode(
            std::io::Cursor::new(byte(
                0x288,
                0xa01f,
                Some("exception_or_low_supervisor_data")
            )),
            intake::Mode::Tst
        )
        .is_err()
    );
}

#[test]
fn tst_classification_and_execution_selection_have_distinct_legal_boundaries() {
    for opcode in [0x4a00, 0x4a45, 0x4a80, 0x4a10, 0x4a79, 0x4ab9] {
        assert!(profile::known(opcode));
    }
    for opcode in [0x4a08, 0x4a7a, 0x4a7b, 0x4a7c, 0x4ac0, 0x4afc] {
        assert!(!profile::known(opcode));
    }
    for (opcode, selected) in [
        (0x4a45, false),
        (0x4a10, false),
        (0x4a50, true),
        (0x4ab9, true),
        (0x4a7a, false),
    ] {
        let mut value = case_value();
        value["initial"]["prefetch"][0] = json!(opcode);
        let case: Case = serde_json::from_value(value).unwrap();
        assert_eq!(intake::selected_tst(&case), selected);
        assert!(!intake::selected(&case));
    }
}
