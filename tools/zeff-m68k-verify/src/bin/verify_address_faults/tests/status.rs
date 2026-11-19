use super::*;

fn rows(source: &str) -> Vec<Value> {
    serde_json::from_str(source).unwrap()
}

fn status_stream(mut value: Value) -> String {
    value["type"] = json!("case");
    let header = json!({"type":"header", "schema":1, "format":intake::FORMAT,
        "source_commit":intake::COMMIT,
        "cohorts":["LINK","UNLINK","RTR","TRAP","MOVEfromUSP","MOVEtoUSP",
            "MOVEfromSR","MOVEtoSR","MOVEtoCCR"],
        "claim":"reference", "pc_convention":"raw next-prefetch PC retained; no PC normalization",
        "clock_convention":"record total and ordered transaction durations retained separately; no synthesized timeline",
        "ram_convention":"16-bit source words retained as big-endian byte pairs"});
    format!(
        "{header}\n{value}\n{}\n",
        json!({"type":"complete","exported_records":1})
    )
}

#[test]
fn status_processing_legality_and_privileged_execution_are_separate() {
    let mut case: Case = serde_json::from_value(case_value()).unwrap();
    let (mut legal, mut user, mut supervisor) = (0, 0, 0);
    for opcode in 0..=u16::MAX {
        case.initial.prefetch[0] = opcode;
        legal += usize::from(profile::legal_status(opcode));
        case.initial.sr = 0;
        user += usize::from(intake::selected_status_memory(&case));
        case.initial.sr = 0x2000;
        supervisor += usize::from(intake::selected_status_memory(&case));
    }
    assert_eq!((legal, user, supervisor), (106, 44, 88));
    for opcode in [0x44c0, 0x44d0, 0x44fc, 0x46c0, 0x46fb, 0x46fc] {
        assert!(profile::known(opcode));
    }
    for opcode in [0x44c8, 0x46c8, 0x44fd, 0x46ff, 0x4afc] {
        assert!(!profile::known(opcode));
    }
}

#[test]
fn original_status_first_odd_frames_preserve_old_sr_source_bank_and_pc_fc() {
    for value in rows(include_str!("status/literals.json")) {
        let case: Case = serde_json::from_value(value.clone()).unwrap();
        let p = profile::Profile::new(&case).unwrap();
        let op = case.initial.prefetch[0];
        let mode = op >> 3 & 7;
        let reg = usize::from(op & 7);
        assert_eq!(p.phase.status_register(), p.initial.status_register());
        assert_eq!(p.operand_program, mode == 7 && matches!(reg, 2 | 3));
        if mode == 3 {
            assert_eq!(
                p.phase.address_register(reg),
                p.initial.address_register(reg).map(|a| a.wrapping_add(2))
            );
        }
        if mode == 4 {
            assert_eq!(p.phase.address_register(reg), Some(p.operand));
        }
        assert_eq!(witness::run(&case).unwrap(), witness::WitnessResult::Frame);
        for index in p.marker + 2..=p.marker + 8 {
            let mut bad = value.clone();
            bad["transactions"][index]["data_bus"] =
                json!(bad["transactions"][index]["data_bus"].as_u64().unwrap() ^ 1);
            assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
        }
        let mut marker = value.clone();
        marker["transactions"][p.marker]["data_bus"] = json!(0xffff);
        assert_eq!(
            witness::run(&serde_json::from_value(marker).unwrap()).unwrap(),
            witness::WitnessResult::Frame
        );
        let mut bad = value;
        bad["final"]["ssp"] = json!(0);
        assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    }
}

#[test]
fn status_full_nine_cohort_intake_uses_exporter_exclusion_rules() {
    for value in rows(include_str!("status/cohorts.json")) {
        let input = status_stream(value.clone());
        let report =
            intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::StatusMemory)
                .unwrap();
        assert_eq!(report.records, 1);
        assert!(!report.full_coverage());
        let mut bad = value;
        bad["normalization_exclusion"] = json!("invalid");
        assert!(
            intake::verify_reader_mode(
                std::io::Cursor::new(status_stream(bad)),
                intake::Mode::StatusMemory
            )
            .is_err()
        );
        for mode in [
            intake::Mode::Move,
            intake::Mode::Tst,
            intake::Mode::Cmpi,
            intake::Mode::DataArithmetic,
        ] {
            assert!(intake::verify_reader_mode(std::io::Cursor::new(&input), mode).is_err());
        }
    }
}

#[test]
fn status_trace_frame_is_validated_then_excluded_from_runtime() {
    let value = rows(include_str!("status/trace.json")).remove(0);
    let report = intake::verify_reader_mode(
        std::io::Cursor::new(status_stream(value)),
        intake::Mode::StatusMemory,
    )
    .unwrap();
    assert_eq!(
        (
            report.records,
            report.selected,
            report.selected_trace,
            report.passed
        ),
        (1, 0, 1, 0)
    );
}

#[test]
fn status_prefix_requires_actual_ram_and_order_and_user_sr_has_no_profile() {
    let values = rows(include_str!("status/literals.json"));
    let value = values
        .iter()
        .find(|r| r["initial"]["prefetch"][0].as_u64().unwrap() & 0x3f == 0x39)
        .unwrap()
        .clone();
    let case: Case = serde_json::from_value(value.clone()).unwrap();
    let at = case.initial.pc & 0x00ff_ffff;
    let mut missing = value.clone();
    missing["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p[0] != at);
    assert!(witness::run(&serde_json::from_value(missing).unwrap()).is_err());
    let mut reordered = value.clone();
    reordered["transactions"].as_array_mut().unwrap().swap(0, 1);
    assert!(witness::run(&serde_json::from_value(reordered).unwrap()).is_err());
    let mut user: Case = serde_json::from_value(
        values
            .into_iter()
            .find(|r| r["initial"]["prefetch"][0].as_u64().unwrap() & 0xffc0 == 0x46c0)
            .unwrap(),
    )
    .unwrap();
    user.initial.sr &= !0x2000;
    assert!(profile::legal_status(user.initial.prefetch[0]));
    assert!(!intake::selected_status_memory(&user));
    assert!(witness::validate_profile(&user).is_err());
}
