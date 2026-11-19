use super::*;

#[test]
fn cmpa_classification_is_legal_and_execution_stays_memory_only() {
    let (mut legal, mut selected, mut cmp) = (0, 0, 0);
    for opcode in 0..=u16::MAX {
        let mut value = case_value();
        value["initial"]["prefetch"][0] = json!(opcode);
        let case: Case = serde_json::from_value(value).unwrap();
        legal += usize::from(profile::legal_cmpa(opcode));
        selected += usize::from(intake::selected_cmpa(&case));
        cmp += usize::from(profile::legal_cmp(opcode));
    }
    assert_eq!((legal, selected, cmp), (976, 704, 1400));
    for opcode in [0xb0c0, 0xb0c8, 0xb0fc, 0xb1fb, 0xbac4, 0xb9fc] {
        assert!(profile::known(opcode));
    }
    for opcode in [0xb0fd, 0xb1fd, 0xb108, 0xb150, 0x0c50] {
        assert!(!profile::legal_cmpa(opcode));
    }
}

#[test]
fn cmpa_mode_requires_exact_cohorts_and_keeps_trace_unexecuted() {
    for (cohort, opcode, selected) in [
        ("CMPA.w", 0xb0d0, 1),
        ("CMPA.l", 0xb1d0, 1),
        ("CMPA.w", 0xb0c0, 0),
        ("CMPA.l", 0xb1fc, 0),
    ] {
        let mut value = case_value();
        value["cohort"] = json!(cohort);
        value["initial"]["prefetch"][0] = json!(opcode);
        value["initial"]["sr"] = json!(0x801f);
        value["transactions"][0]["kind"] = json!("read_address_error");
        value["normalization_exclusion"] = json!("trace");
        let input = stream(value, 1).replace(
            "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
            "[\"CMPA.w\",\"CMPA.l\"]",
        );
        let report =
            intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Cmpa).unwrap();
        assert_eq!(
            (report.records, report.selected, report.selected_trace),
            (1, 0, selected)
        );
        assert!(!report.full_coverage());
        for mode in [intake::Mode::Move, intake::Mode::Tst, intake::Mode::Cmp] {
            assert!(intake::verify_reader_mode(std::io::Cursor::new(&input), mode).is_err());
        }
        let wrong = input.replace(
            cohort,
            if cohort == "CMPA.w" {
                "CMPA.l"
            } else {
                "CMPA.w"
            },
        );
        assert!(
            intake::verify_reader_mode(std::io::Cursor::new(wrong), intake::Mode::Cmpa).is_err()
        );
    }
}

#[test]
fn cmpa_odd_a7_profile_uses_active_bank_and_sized_update() {
    for supervisor in [false, true] {
        for (opcode, expected) in [
            (0xbfdf, 0x2001),
            (0xbedf, 0x2003),
            (0xbee7, 0x1fff),
            (0xbfe7, 0x1ffd),
        ] {
            let mut value = case_value();
            value["initial"]["prefetch"] = json!([opcode, 0x7201]);
            value["initial"]["sr"] = json!(if supervisor { 0x201f } else { 0x001f });
            value["initial"]["ssp"] = json!(0x2001);
            value["initial"]["usp"] = json!(0x2001);
            let case: Case = serde_json::from_value(value).unwrap();
            let p = profile::Profile::new(&case).unwrap();
            assert_eq!(p.phase.address_register(7), Some(expected));
            assert_eq!(p.phase.d, p.initial.d);
            assert_eq!(p.phase.status_register(), p.initial.status_register());
            assert_eq!(
                p.operand,
                if opcode & 0x38 == 0x20 {
                    expected
                } else {
                    0x2001
                }
            );
            assert!(!p.operand_program);
        }
    }
}

#[test]
fn cmpa_retained_processing_migrations_use_literal_full_raw_frames() {
    for text in [
        include_str!("cmpa_migrations/retained-register.json"),
        include_str!("cmpa_migrations/refilled-immediate.json"),
    ] {
        let value: Value = serde_json::from_str(text).unwrap();
        let case: Case = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(witness::run(&case).unwrap(), witness::WitnessResult::Frame);
        let marker = case
            .transactions
            .iter()
            .position(|p| matches!(p, zeff_m68k_verify::reference::Phase::WriteAddressError(_)))
            .unwrap();
        let ird = value["transactions"][marker + 5]["data_bus"]
            .as_u64()
            .unwrap() as u16;
        assert!(matches!(ird, 0xbac4 | 0xb9fc));
        assert!(profile::legal_cmpa(ird));
        let mut successor = value.clone();
        successor["initial"]["prefetch"][0] = json!(ird);
        assert!(!intake::selected_cmpa(
            &serde_json::from_value(successor).unwrap()
        ));
        for index in [
            marker + 2,
            marker + 3,
            marker + 4,
            marker + 5,
            marker + 6,
            marker + 7,
            marker + 8,
        ] {
            let mut bad = value.clone();
            let word = bad["transactions"][index]["data_bus"].as_u64().unwrap();
            bad["transactions"][index]["data_bus"] = json!(word ^ 1);
            assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
        }
        let mut bad = value;
        bad["initial"]["ram"]
            .as_array_mut()
            .unwrap()
            .retain(|p| p[0] != 12);
        assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    }
}
