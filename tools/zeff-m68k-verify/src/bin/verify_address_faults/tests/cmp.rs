use super::*;

#[test]
fn cmp_processing_and_execution_masks_preserve_other_compare_families() {
    let (mut legal, mut selected) = (0, 0);
    for opcode in 0..=u16::MAX {
        let mut value = case_value();
        value["initial"]["prefetch"][0] = json!(opcode);
        let case: Case = serde_json::from_value(value).unwrap();
        legal += usize::from(profile::legal_cmp(opcode));
        selected += usize::from(intake::selected_cmp(&case));
    }
    assert_eq!((legal, selected), (1400, 704));
    for opcode in [0xb000, 0xb050, 0xb088, 0xb07c, 0xb0bb] {
        assert!(profile::known(opcode));
    }
    for opcode in [0xb008, 0xb0bd, 0xb0fd, 0xb108, 0xb150] {
        assert!(!profile::known(opcode));
    }
}

#[test]
fn cmp_intake_accepts_original_cmpi_cmpm_rows_without_selecting_them() {
    for opcode in [0x0c40, 0xb148, 0xb040] {
        let mut value = case_value();
        value["cohort"] = json!("CMP.w");
        value["initial"]["prefetch"][0] = json!(opcode);
        value["initial"]["sr"] = json!(0xa000);
        value["normalization_exclusion"] = json!("trace");
        let input = stream(value, 1).replace(
            "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
            "[\"CMP.b\",\"CMP.w\",\"CMP.l\"]",
        );
        let report =
            intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Cmp).unwrap();
        assert_eq!(
            (report.records, report.selected, report.selected_trace),
            (1, 0, 0)
        );
        assert!(!report.full_coverage());
        assert!(intake::verify_reader(std::io::Cursor::new(&input)).is_err());
        assert!(
            intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Tst).is_err()
        );
    }
}

#[test]
fn cmp_pc_index_profile_uses_original_dn_and_program_data_operand() {
    let mut value = case_value();
    value["initial"]["prefetch"] = json!([0xb07b, 0x0071]);
    value["initial"]["d0"] = json!(0xaabb0000u32);
    let case: Case = serde_json::from_value(value).unwrap();
    let p = profile::Profile::new(&case).unwrap();
    assert_eq!(p.operand, 0x1073);
    assert_eq!(p.phase, p.initial);
    assert!(p.operand_program);
    assert_eq!(p.prefix_clocks, 6);
    assert!(matches!(p.prefix[0], profile::Prefix::Idle(2)));
    assert!(matches!(p.prefix[1], profile::Prefix::Read(0x1004)));
}
