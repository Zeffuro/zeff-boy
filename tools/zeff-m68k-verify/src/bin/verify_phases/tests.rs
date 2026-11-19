use super::*;
use format::{COMMIT, FORMAT, MAX_LINE, source_hash};
use serde_json::{Value, json};

fn nop() -> Value {
    let mut initial = serde_json::Map::new();
    for name in [
        "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "a0", "a1", "a2", "a3", "a4", "a5", "a6",
        "usp", "ssp",
    ] {
        initial.insert(name.into(), json!(0));
    }
    initial.insert("sr".into(), json!(0x2000));
    initial.insert("pc".into(), json!(0x1004));
    initial.insert("prefetch".into(), json!([0x4e71, 0x1234]));
    initial.insert(
        "ram".into(),
        json!([
            [4096, 78],
            [4097, 113],
            [4098, 18],
            [4099, 52],
            [4100, 86],
            [4101, 120]
        ]),
    );
    let mut expected = initial.clone();
    expected.insert("pc".into(), json!(0x1006));
    expected.insert("prefetch".into(), json!([0x1234, 0x5678]));
    json!({"cohort":"NOP", "name":"synthetic NOP", "initial":initial, "final":expected,
           "reference_total_cycles":4, "normalization_exclusion":null,
           "transactions":[{"kind":"read","duration_cycles":4,"function_code":6,
                            "address_bus":4100,"data_bus":0x5678,"uds":1,"lds":1}]})
}

#[test]
fn checks_prefetch_order_clock_memory_and_bus_fields() {
    assert!(run(&serde_json::from_value(nop()).unwrap()).is_ok());
    for path in [
        "duration_cycles",
        "function_code",
        "address_bus",
        "data_bus",
        "uds",
        "lds",
    ] {
        let mut value = nop();
        value["transactions"][0][path] = json!(0);
        assert!(
            run(&serde_json::from_value(value).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut value = nop();
    value["final"]["prefetch"] = json!([0x5678, 0x1234]);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    let mut value = nop();
    value["reference_total_cycles"] = json!(8);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    let mut value = nop();
    value["initial"]["ram"] = json!([[4096, 78], [4097, 113], [4098, 18], [4099, 52]]);
    assert!(run(&serde_json::from_value(value).unwrap()).is_err());
}

fn stream(footer: u64) -> String {
    let header = json!({"type":"header","schema":1,"format":FORMAT,"source_commit":COMMIT,
                       "cohorts":["NOP"],"claim":"reference","pc_convention":"raw next-prefetch PC retained; no PC normalization",
                       "clock_convention":"durations","ram_convention":"bytes"});
    let mut case = nop();
    case["type"] = json!("case");
    format!(
        "{header}\n{case}\n{}\n",
        json!({"type":"complete","exported_records":footer})
    )
}

#[test]
fn rejects_incomplete_counts_extra_records_and_line_overflow() {
    let valid = stream(1);
    let report = verify_reader(std::io::Cursor::new(&valid)).unwrap();
    assert_eq!(report.records, 1);
    assert_eq!(report.ledger["NOP"].passed, 1);
    assert!(verify_reader(std::io::Cursor::new(stream(2))).is_err());
    assert!(verify_reader(std::io::Cursor::new(format!("{valid}{{}}\n"))).is_err());
    let truncated = valid.lines().take(2).collect::<Vec<_>>().join("\n");
    assert!(verify_reader(std::io::Cursor::new(truncated)).is_err());
    assert!(verify_reader(std::io::Cursor::new(vec![b' '; MAX_LINE + 1])).is_err());
}

#[test]
fn conditional_branches_preserve_idle_and_fetch_order() {
    for (opcode, idle, total, pc, prefetch, fetches) in [
        (0x6602, 2, 10, 0x1008, [0x5678, 0x9abc], 2),
        (0x6702, 4, 8, 0x1006, [0x1234, 0x5678], 1),
        (0x6700, 4, 12, 0x1008, [0x5678, 0x9abc], 2),
    ] {
        let mut value = nop();
        value["cohort"] = json!("Bcc");
        value["initial"]["prefetch"][0] = json!(opcode);
        for state in ["initial", "final"] {
            value[state]["ram"][0][1] = json!(opcode >> 8);
            value[state]["ram"][1][1] = json!(opcode & 255);
            value[state]["ram"]
                .as_array_mut()
                .unwrap()
                .extend([json!([4102, 154]), json!([4103, 188])]);
        }
        value["final"]["pc"] = json!(pc);
        value["final"]["prefetch"] = json!(prefetch);
        value["reference_total_cycles"] = json!(total);
        let mut phases = vec![json!({"kind":"idle", "duration_cycles":idle})];
        for index in 0..fetches {
            phases.push(json!({"kind":"read", "duration_cycles":4,
                "function_code":6, "address_bus":4100+index*2,
                "data_bus":if index==0 {0x5678} else {0x9abc}, "uds":1, "lds":1}));
        }
        value["transactions"] = json!(phases);
        assert!(
            run(&serde_json::from_value(value.clone()).unwrap()).is_ok(),
            "{opcode:04x}"
        );
        value["transactions"][0]["duration_cycles"] = json!(1);
        assert!(run(&serde_json::from_value(value).unwrap()).is_err());
    }
}

#[test]
fn receipt_requires_exact_export_pin_and_complete_ledger() {
    let report = verify_reader(std::io::Cursor::new(stream(1))).unwrap();
    let receipt = json!({"schema":1, "format":FORMAT,
        "source":format!("https://raw.githubusercontent.com/SingleStepTests/m68000/{COMMIT}/"),
        "source_commit":COMMIT, "license":"MIT", "claim":"reference",
        "license_sha256":"9b9e332ee11019f131e945d40e20ab0c48b52921754dc03fe62cc85b579e6c61",
        "readme_sha256":"7f36eed9cb93f061d13bfd18dc310c9bda7b5f3ac96b6d0dfe292e00612c46b6",
        "export_sha256":report.hash, "exported_records":1,
        "cohorts":[{"cohort":"NOP", "raw_sha256":source_hash("NOP").unwrap(),
            "total":1, "eligible":1, "excluded":{}, "exported":1}]});
    assert!(verify_receipt(serde_json::from_value(receipt.clone()).unwrap(), &report).is_ok());
    for field in ["export_sha256", "source_commit", "license_sha256"] {
        let mut bad = receipt.clone();
        bad[field] = json!("wrong");
        assert!(verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
    }
    for field in ["total", "eligible", "exported"] {
        let mut bad = receipt.clone();
        bad["cohorts"][0][field] = json!(0);
        assert!(verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
    }
    let mut bad = receipt;
    bad["cohorts"][0]["raw_sha256"] = json!("wrong");
    assert!(verify_receipt(serde_json::from_value(bad).unwrap(), &report).is_err());
}

#[test]
fn trace_checks_supervisor_stack_vector_and_preserved_intake_ledger() {
    let mut value = nop();
    value["normalization_exclusion"] = json!("trace");
    value["initial"]["sr"] = json!(0xa000);
    value["initial"]["ssp"] = json!(0x2000);
    value["final"]["ssp"] = json!(0x1ffa);
    value["final"]["pc"] = json!(0x3004);
    value["final"]["prefetch"] = json!([0x1111, 0x2222]);
    let bytes = json!([
        [36, 0],
        [37, 0],
        [38, 48],
        [39, 0],
        [12288, 17],
        [12289, 17],
        [12290, 34],
        [12291, 34]
    ]);
    for state in ["initial", "final"] {
        value[state]["ram"]
            .as_array_mut()
            .unwrap()
            .extend(bytes.as_array().unwrap().clone());
    }
    value["final"]["ram"].as_array_mut().unwrap().extend([
        json!([8190, 16]),
        json!([8191, 2]),
        json!([8186, 160]),
        json!([8187, 0]),
        json!([8188, 0]),
        json!([8189, 0]),
    ]);
    let mut phases = value["transactions"].as_array().unwrap().clone();
    phases.push(json!({"kind":"idle","duration_cycles":4}));
    for (kind, address, data, fc) in [
        ("write", 8190, 0x1002, 5),
        ("write", 8186, 0xa000, 5),
        ("write", 8188, 0, 5),
        ("read", 36, 0, 5),
        ("read", 38, 0x3000, 5),
        ("read", 12288, 0x1111, 6),
    ] {
        phases.push(json!({"kind":kind,"duration_cycles":4,"function_code":fc,
            "address_bus":address,"data_bus":data,"uds":1,"lds":1}));
    }
    phases.push(json!({"kind":"idle","duration_cycles":2}));
    phases.push(json!({"kind":"read","duration_cycles":4,"function_code":6,
        "address_bus":12290,"data_bus":0x2222,"uds":1,"lds":1}));
    value["transactions"] = json!(phases);
    value["reference_total_cycles"] = json!(38);
    assert!(run_boundary(&serde_json::from_value(value.clone()).unwrap(), true).is_ok());
    let mut prefix = nop();
    prefix["type"] = json!("case");
    prefix["normalization_exclusion"] = json!("trace");
    prefix["initial"]["sr"] = json!(0xa000);
    prefix["final"]["sr"] = json!(0xa000);
    let header = stream(1).lines().next().unwrap().to_owned();
    let text = format!(
        "{header}\n{prefix}\n{}\n",
        json!({"type":"complete","exported_records":1})
    );
    let report = verify_reader(std::io::Cursor::new(text)).unwrap();
    assert_eq!(report.ledger["NOP"].eligible, 0);
    assert_eq!(report.ledger["NOP"].excluded["trace"], 1);
    assert_eq!(report.ledger["NOP"].trace_prefix_pass, 1);
    prefix["initial"]["pc"] = json!(0x1005);
    let text = format!(
        "{header}\n{prefix}\n{}\n",
        json!({"type":"complete","exported_records":1})
    );
    let report = verify_reader(std::io::Cursor::new(text)).unwrap();
    assert_eq!(report.ledger["NOP"].physical_unsupported, 1);
    assert_eq!(report.ledger["NOP"].trace_prefix_pass, 0);
    assert_eq!(report.ledger["NOP"].excluded["trace"], 1);

    value["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|byte| byte[0] != json!(36));
    assert!(
        run_boundary(&serde_json::from_value(value).unwrap(), true)
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn rejects_fabricated_exclusions_and_missing_final_snapshot() {
    for reason in ["trace", "address_fault", "self_modifying_code"] {
        let mut case = nop();
        case["type"] = json!("case");
        case["normalization_exclusion"] = json!(reason);
        let header = stream(1).lines().next().unwrap().to_owned();
        let text = format!(
            "{header}\n{case}\n{}\n",
            json!({"type":"complete","exported_records":1})
        );
        assert!(
            verify_reader(std::io::Cursor::new(text)).is_err(),
            "{reason}"
        );
    }
    let mut case = nop();
    case["final"]["ram"] = json!([]);
    assert!(
        run(&serde_json::from_value(case).unwrap())
            .unwrap_err()
            .contains("omits initial witness")
    );
    let mut case = nop();
    case["type"] = json!("case");
    case["initial"]["sr"] = json!(0xa000);
    let header = stream(1).lines().next().unwrap().to_owned();
    let text = format!(
        "{header}\n{case}\n{}\n",
        json!({"type":"complete","exported_records":1})
    );
    assert!(verify_reader(std::io::Cursor::new(text)).is_err());
}

#[test]
fn rejects_duplicate_or_out_of_range_memory() {
    assert!(memory_map(&[(1, 0), (1, 0)]).is_err());
    assert!(memory_map(&[(0x100_0000, 0)]).is_err());
}

#[test]
fn idle_partitions_preserve_intervening_transfers() {
    let phases: Vec<Phase> = serde_json::from_value(json!([
        {"kind":"idle","duration_cycles":2}, {"kind":"idle","duration_cycles":2}
    ]))
    .unwrap();
    let mut bus = WitnessBus {
        memory: BTreeMap::new(),
        phases: &phases,
        cursor: 0,
        elapsed: 0,
        error: None,
        trace_stack: None,
        control_stack: None,
        status_contract: None,
        short_vector: None,
        transfer_contract: None,
        held_contract: None,
        ordinary_contract: None,
        reset_owned: false,
        reset_pairs: 0,
        idle_remaining: 0,
        operand_data: false,
        pc_source: false,
    };
    bus.idle(CpuClocks(0), 1);
    bus.idle(CpuClocks(1), 3);
    assert_eq!((bus.cursor, bus.elapsed, bus.idle_remaining), (2, 4, 0));
    assert!(bus.error.is_none());
    let phases: Vec<Phase> = serde_json::from_value(json!([
        {"kind":"idle","duration_cycles":2},
        {"kind":"read","duration_cycles":4,"function_code":6,
         "address_bus":4100,"data_bus":0x5678,"uds":1,"lds":1},
        {"kind":"idle","duration_cycles":2}
    ]))
    .unwrap();
    let mut bus = WitnessBus {
        memory: BTreeMap::new(),
        phases: &phases,
        cursor: 0,
        elapsed: 0,
        error: None,
        trace_stack: None,
        control_stack: None,
        status_contract: None,
        short_vector: None,
        transfer_contract: None,
        held_contract: None,
        ordinary_contract: None,
        reset_owned: false,
        reset_pairs: 0,
        idle_remaining: 0,
        operand_data: false,
        pc_source: false,
    };
    bus.idle(CpuClocks(0), 4);
    assert!(bus.error.unwrap().contains("crosses transfer"));
    assert_eq!((bus.cursor, bus.elapsed), (1, 2));
}

#[test]
fn register_normalization_tags_do_not_hide_execution() {
    let mut case = nop();
    case["cohort"] = json!("ADD.l");
    case["initial"]["prefetch"][0] = json!(0xd080);
    for state in ["initial", "final"] {
        case[state]["ram"][0][1] = json!(0xd0);
        case[state]["ram"][1][1] = json!(0x80);
    }
    case["final"]["sr"] = json!(0x2004);
    case["reference_total_cycles"] = json!(8);
    case["transactions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"idle","duration_cycles":4}));
    case["normalization_exclusion"] = json!("data_prefetch_overlap");
    case["type"] = json!("case");
    let mut header: Value = serde_json::from_str(stream(1).lines().next().unwrap()).unwrap();
    header["cohorts"] = json!(["ADD.l"]);
    let footer = json!({"type":"complete","exported_records":1});
    let text = format!("{header}\n{case}\n{footer}\n");
    let report = verify_reader(std::io::Cursor::new(text)).unwrap();
    assert_eq!(report.ledger["ADD.l"].passed, 1);
    assert_eq!(report.ledger["ADD.l"].excluded["data_prefetch_overlap"], 1);
    case["final"]["d0"] = json!(1);
    let report = verify_reader(std::io::Cursor::new(format!(
        "{header}\n{case}\n{footer}\n"
    )))
    .unwrap();
    assert_eq!(report.failures, 1);
}

fn cohort_stream(mut case: Value) -> String {
    let mut header: Value = serde_json::from_str(stream(1).lines().next().unwrap()).unwrap();
    header["cohorts"] = json!([case["cohort"]]);
    case["type"] = json!("case");
    format!(
        "{header}\n{case}\n{}\n",
        json!({"type":"complete","exported_records":1})
    )
}

#[test]
fn rejects_opcode_family_and_size_before_unsupported_classification() {
    for (cohort, opcode) in [
        ("ASL.b", 0xe080),
        ("ASL.b", 0xe140),
        ("ROR.l", 0xe1b8),
        ("NEGX.l", 0x4080 - 0x40),
        ("NEGX.l", 0x4480),
        ("ADD.w", 0xd080),
        ("ADD.l", 0x5180),
        ("ADD.b", 0xd0c0),
        ("MULS", 0xc0c0),
        ("BCLR", 0x01c0),
    ] {
        let mut case = nop();
        case["cohort"] = json!(cohort);
        case["initial"]["prefetch"][0] = json!(opcode);
        assert!(
            verify_reader(std::io::Cursor::new(cohort_stream(case))).is_err(),
            "{cohort}/{opcode:04x}"
        );
    }
}

#[test]
fn add_cohorts_execute_supported_quick_extend_and_address_aliases() {
    for (cohort, opcode, initial_sr, destination, total) in [
        ("ADD.l", 0x5280, 0x2000, "d0", 8),
        ("ADD.l", 0xd180, 0x2010, "d0", 8),
        ("ADD.w", 0xd0c0, 0x2000, "a0", 8),
    ] {
        let mut case = nop();
        case["cohort"] = json!(cohort);
        case["initial"]["prefetch"][0] = json!(opcode);
        case["initial"]["sr"] = json!(initial_sr);
        if destination == "a0" {
            case["initial"]["d0"] = json!(1);
            case["final"]["d0"] = json!(1);
        }
        case["final"][destination] = json!(1);
        for state in ["initial", "final"] {
            case[state]["ram"][0][1] = json!(opcode >> 8);
            case[state]["ram"][1][1] = json!(opcode & 255);
        }
        case["reference_total_cycles"] = json!(total);
        case["transactions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"idle","duration_cycles":4}));
        let report = verify_reader(std::io::Cursor::new(cohort_stream(case))).unwrap();
        assert_eq!(report.failures, 0, "{opcode:04x}");
        assert_eq!(report.ledger[cohort].passed, 1);
        assert!(report.ledger[cohort].unsupported_routes.is_empty());
    }
}

#[path = "move_tests.rs"]
mod move_tests;

#[path = "unary_tests.rs"]
mod unary_tests;

#[path = "binary_tests.rs"]
mod binary_tests;

#[path = "immediate_tests.rs"]
mod immediate_tests;

#[path = "bit_shift_tests.rs"]
mod bit_shift_tests;

#[path = "paired_tests.rs"]
mod paired_tests;

#[path = "arithmetic_tests.rs"]
mod arithmetic_tests;
#[path = "branch_tests.rs"]
mod branch_tests;
#[path = "stack_status_tests.rs"]
mod stack_status_tests;
#[path = "status_move_tests.rs"]
mod status_move_tests;
#[path = "transfer_tests.rs"]
mod transfer_tests;

#[path = "held_control_tests.rs"]
mod held_control_tests;

#[path = "ordinary_register_tests.rs"]
mod ordinary_register_tests;
#[path = "ordinary_unary_tests.rs"]
mod ordinary_unary_tests;
