use super::*;

#[test]
fn processing_masks_and_memory_execution_are_independent() {
    let mut case: Case = serde_json::from_value(case_value()).unwrap();
    let (mut legal, mut selected) = (0, 0);
    for opcode in 0..=u16::MAX {
        case.initial.prefetch[0] = opcode;
        legal += usize::from(profile::legal_data_arithmetic(opcode));
        selected += usize::from(intake::selected_data_arithmetic(&case));
    }
    assert_eq!((legal, selected), (5344, 2816));
    for opcode in [0x8aa6, 0x9489, 0x8000, 0xc000, 0xd07c, 0x9088] {
        assert!(profile::known(opcode));
    }
    for opcode in [0x8008, 0xc048, 0xd008, 0x90bd, 0xd150, 0x4afc] {
        assert!(!profile::legal_data_arithmetic(opcode));
    }
}

#[test]
fn complete_source_cohorts_preserve_aliases_and_select_only_memory_word_long() {
    for (family, top) in [("ADD", 13), ("SUB", 9), ("AND", 12), ("OR", 8)] {
        for (suffix, size) in [("b", 0), ("w", 1), ("l", 2)] {
            let mut value = case_value();
            let opcode = top << 12 | size << 6 | 0x10;
            value["cohort"] = json!(format!("{family}.{suffix}"));
            value["initial"]["prefetch"][0] = json!(opcode);
            value["initial"]["sr"] = json!(0x801f);
            value["transactions"][0]["kind"] = json!("read_address_error");
            value["normalization_exclusion"] = json!("trace");
            let input = stream(value.clone(), 1).replace(
                "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
                "[\"ADD.b\",\"ADD.w\",\"ADD.l\",\"SUB.b\",\"SUB.w\",\"SUB.l\",\"AND.b\",\"AND.w\",\"AND.l\",\"OR.b\",\"OR.w\",\"OR.l\"]",
            );
            let report = intake::verify_reader_mode(
                std::io::Cursor::new(&input),
                intake::Mode::DataArithmetic,
            )
            .unwrap();
            assert_eq!(
                (report.records, report.selected, report.selected_trace),
                (1, 0, u64::from(size != 0))
            );
            assert!(!report.full_coverage());
            let wrong = input.replace(&opcode.to_string(), &(opcode ^ 0x4000).to_string());
            assert!(
                intake::verify_reader_mode(
                    std::io::Cursor::new(wrong),
                    intake::Mode::DataArithmetic
                )
                .is_err()
            );
            let mut case: Case = serde_json::from_value(value).unwrap();
            for low in [0x00, 0x10, 0x110] {
                case.initial.prefetch[0] = top << 12 | size << 6 | low;
                assert!(profile::data_cohort_matches(&case));
            }
            let immediate = match top {
                13 => 0x0600,
                9 => 0x0400,
                12 => 0x0200,
                _ => 0,
            };
            case.initial.prefetch[0] = immediate | size << 6 | 0x10;
            assert!(profile::data_cohort_matches(&case));
            assert!(!intake::selected_data_arithmetic(&case));
        }
    }
}

#[test]
fn first_odd_preserves_flags_and_data_while_updating_the_active_a7_bank() {
    for family in [0x8000, 0x9000, 0xc000, 0xd000] {
        for supervisor in [false, true] {
            for (low, expected) in [
                (0x5f, 0x2003),
                (0x9f, 0x2001),
                (0x67, 0x1fff),
                (0xa7, 0x1ffd),
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
fn original_retained_memory_and_register_opcodes_qualify_complete_move_frames() {
    for text in [
        include_str!("data_arithmetic_migrations/retained-memory.json"),
        include_str!("data_arithmetic_migrations/retained-register.json"),
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
