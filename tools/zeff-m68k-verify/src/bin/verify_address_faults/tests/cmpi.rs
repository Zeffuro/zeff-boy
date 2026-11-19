use super::*;

fn literal(long_absolute: bool, supervisor: bool) -> Value {
    let mut value = case_value();
    let (opcode, pc, source_base, words): (u16, u16, u32, Vec<(u32, u16)>) = if long_absolute {
        (
            0x0cb9,
            0x100a,
            0x1234_5679,
            vec![
                (0x1000, 0x0cb9),
                (0x1002, 0),
                (0x1004, 1),
                (0x1006, 0x1234),
                (0x1008, 0x5679),
                (0x100a, 0x4e71),
            ],
        )
    } else {
        (
            0x0c60,
            0x1006,
            0x1234_567b,
            vec![(0x1000, 0x0c60), (0x1002, 0x8000), (0x1004, 0x4e71)],
        )
    };
    let sr: u16 = if supervisor { 0x201f } else { 0x001f };
    let pfc = if supervisor { 6 } else { 2 };
    let dfc: u16 = if supervisor { 5 } else { 1 };
    let mut ram = Vec::new();
    for (address, word) in [(12, 0), (14, 0x3000), (0x3000, 0x4e71), (0x3002, 0x7001)]
        .into_iter()
        .chain(words.iter().copied())
    {
        ram.push((address, (word >> 8) as u8));
        ram.push((address + 1, word as u8));
    }
    value["cohort"] = json!(if long_absolute { "CMP.l" } else { "CMP.w" });
    value["initial"]["a0"] = json!(source_base);
    value["initial"]["sr"] = json!(sr);
    value["initial"]["prefetch"] = json!([opcode, words[1].1]);
    value["initial"]["ram"] = json!(ram);
    let mut final_state = value["initial"].clone();
    final_state["a0"] = json!(0x1234_5679u32);
    final_state["ssp"] = json!(0x1ff2);
    final_state["sr"] = json!(sr | 0x2000);
    final_state["pc"] = json!(0x3004);
    final_state["prefetch"] = json!([0x4e71, 0x7001]);
    let captured = [
        pc,
        sr,
        0,
        opcode,
        0x5679,
        (opcode & 0xffe0) | 0x10 | dfc,
        0x1234,
    ];
    let mut phases = Vec::new();
    for (address, word) in words.iter().skip(2) {
        phases.push(json!({"kind":"read", "duration_cycles":4,
            "function_code":pfc, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
    }
    if !long_absolute {
        phases.push(json!({"kind":"idle", "duration_cycles":2}));
    }
    phases.push(json!({"kind":"read_address_error", "duration_cycles":4,
        "function_code":dfc, "address_bus":0x345678, "data_bus":0, "uds":1, "lds":1}));
    phases.push(json!({"kind":"idle", "duration_cycles":8}));
    for (offset, word) in [2, 6, 4, 8, 10, 14, 12].into_iter().zip(captured) {
        let address = 0x2000 - offset;
        ram.push((address, (word >> 8) as u8));
        ram.push((address + 1, word as u8));
        phases.push(json!({"kind":"write", "duration_cycles":4,
            "function_code":5, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
    }
    for (address, word, fc) in [(12, 0, 5), (14, 0x3000, 5), (0x3000, 0x4e71, 6)] {
        phases.push(json!({"kind":"read", "duration_cycles":4,
            "function_code":fc, "address_bus":address, "data_bus":word, "uds":1, "lds":1}));
    }
    phases.push(json!({"kind":"idle", "duration_cycles":2}));
    phases.push(json!({"kind":"read", "duration_cycles":4,
        "function_code":6, "address_bus":0x3002, "data_bus":0x7001, "uds":1, "lds":1}));
    final_state["ram"] = json!(ram);
    value["final"] = final_state;
    value["transactions"] = json!(phases);
    value["reference_total_cycles"] = json!(if long_absolute { 74 } else { 64 });
    value
}

#[test]
fn cmpi_processing_and_memory_execution_masks_are_separate() {
    let mut case: Case = serde_json::from_value(case_value()).unwrap();
    let (mut legal, mut selected) = (0, 0);
    for opcode in 0..=u16::MAX {
        case.initial.prefetch[0] = opcode;
        legal += usize::from(profile::legal_cmpi(opcode));
        selected += usize::from(intake::selected_cmpi(&case));
    }
    assert_eq!((legal, selected), (150, 84));
    for opcode in [0x0c24, 0x0c40, 0x0c50, 0x0cb9] {
        assert!(profile::known(opcode));
    }
    for opcode in [0x0c08, 0x0c48, 0x0c7a, 0x0cc0, 0x4afc] {
        assert!(!profile::known(opcode));
    }
}

#[test]
fn cmpi_immediate_prefix_then_source_idle_and_full_literal_frames() {
    for long in [false, true] {
        for supervisor in [false, true] {
            let value = literal(long, supervisor);
            let case: Case = serde_json::from_value(value.clone()).unwrap();
            let p = profile::Profile::new(&case).unwrap();
            assert_eq!(p.operand, 0x1234_5679);
            assert_eq!(p.prefix_clocks, if long { 16 } else { 6 });
            assert_eq!(p.marker, if long { 4 } else { 2 });
            assert_eq!(p.phase.status_register(), p.initial.status_register());
            assert!(!p.operand_program);
            assert_eq!(witness::run(&case).unwrap(), witness::WitnessResult::Frame);
            for index in [0, p.marker + 2, p.marker + 3, p.marker + 5, p.marker + 7] {
                let mut bad = value.clone();
                bad["transactions"][index]["data_bus"] = json!(0xffff);
                assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
            }
            let mut bad = value.clone();
            bad["transactions"].as_array_mut().unwrap().swap(0, 1);
            assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
            let mut bad = value;
            bad["initial"]["ram"]
                .as_array_mut()
                .unwrap()
                .retain(|pair| pair[0] != 0x1004);
            assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
        }
    }
}

#[test]
fn cmpi_intake_excludes_trace_and_preserves_cmp_selection() {
    let value = literal(false, false);
    let make_stream = |value: Value| {
        stream(value, 1).replace(
            "[\"MOVE.b\",\"MOVE.w\",\"MOVE.l\",\"MOVEA.w\",\"MOVEA.l\"]",
            "[\"CMP.b\",\"CMP.w\",\"CMP.l\"]",
        )
    };
    let report = intake::verify_reader_mode(
        std::io::Cursor::new(make_stream(value.clone())),
        intake::Mode::Cmpi,
    )
    .unwrap();
    assert_eq!(
        (report.records, report.selected, report.selected_trace),
        (1, 1, 0)
    );
    assert_eq!(
        (report.passed, report.capture_gaps, report.failures),
        (1, 0, 0)
    );
    assert!(!report.full_coverage());
    let mut traced = value;
    traced["initial"]["sr"] = json!(0x801f);
    traced["normalization_exclusion"] = json!("trace");
    traced["transactions"][5]["data_bus"] = json!(0x801f);
    let input = make_stream(traced);
    let report =
        intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Cmpi).unwrap();
    assert_eq!(
        (report.selected, report.selected_trace, report.passed),
        (0, 1, 0)
    );
    let report =
        intake::verify_reader_mode(std::io::Cursor::new(&input), intake::Mode::Cmp).unwrap();
    assert_eq!((report.selected, report.selected_trace), (0, 0));
}

#[test]
fn original_retained_byte_cmpi_qualifies_the_unchanged_full_move_frame() {
    let value: Value =
        serde_json::from_str(include_str!("cmpi_migrations/retained-byte.json")).unwrap();
    let case: Case = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(case.name, "1161 MOVE.w A4, -(A1) 330c");
    assert_eq!(case.reference_total_cycles, 62);
    let marker = case
        .transactions
        .iter()
        .position(|p| matches!(p, zeff_m68k_verify::reference::Phase::WriteAddressError(_)))
        .unwrap();
    let frame_words: Vec<u32> = case.transactions[marker + 2..=marker + 8]
        .iter()
        .map(|p| match p {
            zeff_m68k_verify::reference::Phase::Write(t) => t.data_bus,
            _ => panic!("missing original frame write"),
        })
        .collect();
    assert_eq!(
        frame_words,
        [52028, 8472, 192, 0x0c24, 38139, 0x0c25, 34628]
    );
    assert!(profile::legal_cmpi(0x0c24));
    let mut processing: Case = serde_json::from_value(value.clone()).unwrap();
    processing.initial.prefetch[0] = 0x0c24;
    assert!(!intake::selected_cmpi(&processing));
    assert_eq!(witness::run(&case).unwrap(), witness::WitnessResult::Frame);
    for index in marker + 2..=marker + 8 {
        let mut bad = value.clone();
        let word = bad["transactions"][index]["data_bus"].as_u64().unwrap();
        bad["transactions"][index]["data_bus"] = json!(word ^ 1);
        assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
    }
    let mut bad = value;
    bad["final"]["ssp"] = json!(0);
    assert!(witness::run(&serde_json::from_value(bad).unwrap()).is_err());
}
