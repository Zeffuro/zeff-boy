use super::*;

#[test]
fn address_arithmetic_recognition_keeps_memory_execution_separate() {
    let mut case: Case = serde_json::from_value(case_value()).unwrap();
    let (mut legal, mut selected) = (0, 0);
    for opcode in 0..=u16::MAX {
        case.initial.prefetch[0] = opcode;
        legal += usize::from(profile::legal_address_arithmetic(opcode));
        selected += usize::from(intake::selected_address_arithmetic(&case));
    }
    assert_eq!((legal, selected), (1952, 1408));
    for opcode in [0x9eee, 0xd9d7, 0xdcf2, 0xdff0, 0x99f5, 0x98f3] {
        assert!(profile::known(opcode));
    }
    for opcode in [0x90fd, 0xd1fd, 0x9108, 0xd150, 0xb0d0] {
        assert!(!profile::legal_address_arithmetic(opcode));
    }
}

#[test]
fn address_arithmetic_intake_checks_family_size_and_trace_partition() {
    for (cohort, opcode, selected) in [
        ("ADDA.w", 0xd0d0, 1),
        ("ADDA.l", 0xd1d0, 1),
        ("SUBA.w", 0x90d0, 1),
        ("SUBA.l", 0x91d0, 1),
        ("ADDA.w", 0xd0c8, 0),
        ("SUBA.l", 0x91fc, 0),
    ] {
        let mut value = case_value();
        value["cohort"] = json!(cohort);
        value["initial"]["prefetch"][0] = json!(opcode);
        value["initial"]["sr"] = json!(0x801f);
        value["transactions"][0]["kind"] = json!("read_address_error");
        value["normalization_exclusion"] = json!("trace");
        let input = stream(value, 1).replace(
            "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
            "[\"ADDA.w\",\"ADDA.l\",\"SUBA.w\",\"SUBA.l\"]",
        );
        let report = intake::verify_reader_mode(
            std::io::Cursor::new(&input),
            intake::Mode::AddressArithmetic,
        )
        .unwrap();
        assert_eq!(
            (report.records, report.selected, report.selected_trace),
            (1, 0, selected)
        );
        assert!(!report.full_coverage());
        for other in [
            intake::Mode::Move,
            intake::Mode::Tst,
            intake::Mode::Cmp,
            intake::Mode::Cmpa,
        ] {
            assert!(intake::verify_reader_mode(std::io::Cursor::new(&input), other).is_err());
        }
        for wrong in [opcode ^ 0x4000, opcode ^ 0x0100] {
            let bad = input.replace(&opcode.to_string(), &wrong.to_string());
            assert!(
                intake::verify_reader_mode(
                    std::io::Cursor::new(bad),
                    intake::Mode::AddressArithmetic
                )
                .is_err()
            );
        }
    }
}

#[test]
fn address_arithmetic_first_odd_keeps_sr_and_active_bank_updates() {
    for family in [0x9000, 0xd000] {
        for supervisor in [false, true] {
            for (low, expected) in [
                (0x0edf, 0x2003),
                (0x0fdf, 0x2001),
                (0x0ee7, 0x1fff),
                (0x0fe7, 0x1ffd),
            ] {
                let mut value = case_value();
                value["initial"]["prefetch"] = json!([family | low, 0x7201]);
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
                    if low & 0x38 == 0x20 { expected } else { 0x2001 }
                );
            }
        }
    }
}

#[test]
fn address_arithmetic_processing_migrations_validate_full_frames_and_ram() {
    for text in [
        include_str!("address_arithmetic_migrations/predecrement.json"),
        include_str!("address_arithmetic_migrations/register.json"),
        include_str!("address_arithmetic_migrations/user-stack-source.json"),
        include_str!("address_arithmetic_migrations/supervisor-stack-source.json"),
    ] {
        let value: Value = serde_json::from_str(text).unwrap();
        let case: Case = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(witness::run(&case).unwrap(), witness::WitnessResult::Frame);
        let marker = case
            .transactions
            .iter()
            .position(|p| matches!(p, zeff_m68k_verify::reference::Phase::WriteAddressError(_)))
            .unwrap();
        for index in marker + 2..=marker + 8 {
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
