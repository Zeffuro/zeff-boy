use super::{cohort_stream, nop};
use crate::*;
use serde_json::{Value, json};

fn move_case(opcode: u16, cohort: &str) -> Value {
    let mut case = nop();
    case["cohort"] = json!(cohort);
    case["initial"]["prefetch"][0] = json!(opcode);
    for state in ["initial", "final"] {
        case[state]["ram"][0][1] = json!(opcode >> 8);
        case[state]["ram"][1][1] = json!(opcode & 255);
    }
    case
}

#[test]
fn move_byte_lanes_preserve_neighbors_and_validate_padding() {
    for address in [8192, 8193] {
        let mut case = move_case(0x1080, "MOVE.b");
        case["initial"]["d0"] = json!(0xab);
        case["final"]["d0"] = json!(0xab);
        case["initial"]["a0"] = json!(address);
        case["final"]["a0"] = json!(address);
        case["final"]["sr"] = json!(0x2008);
        case["reference_total_cycles"] = json!(8);
        let upper = address == 8192;
        let write = json!({"kind":"write","duration_cycles":4,"function_code":5,
            "address_bus":8192,"data_bus":if upper {0xab00} else {0xab},
            "uds":if upper {1} else {0},"lds":if upper {0} else {1}});
        case["transactions"]
            .as_array_mut()
            .unwrap()
            .insert(0, write);
        case["final"]["ram"]
            .as_array_mut()
            .unwrap()
            .extend([json!([address, 0xab]), json!([address ^ 1, 0])]);
        assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
        let mut bad = case.clone();
        bad["final"]["ram"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()[1] = json!(1);
        assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
        for state in ["initial", "final"] {
            case[state]["ram"]
                .as_array_mut()
                .unwrap()
                .retain(|byte| byte[0] != json!(address ^ 1));
            case[state]["ram"]
                .as_array_mut()
                .unwrap()
                .push(json!([address ^ 1, 0x7e]));
        }
        assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
        case["final"]["ram"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()[1] = json!(0);
        assert!(run(&serde_json::from_value(case).unwrap()).is_err());
    }
}

#[test]
fn move_long_predecrement_requires_low_word_before_high_word() {
    let mut case = move_case(0x2100, "MOVE.l");
    case["initial"]["d0"] = json!(0x11223344);
    case["final"]["d0"] = json!(0x11223344);
    case["initial"]["a0"] = json!(0x2004);
    case["final"]["a0"] = json!(0x2000);
    case["reference_total_cycles"] = json!(12);
    for (address, data) in [(8194, 0x3344), (8192, 0x1122)] {
        case["transactions"].as_array_mut().unwrap().push(json!({"kind":"write",
            "duration_cycles":4,"function_code":5,"address_bus":address,"data_bus":data,"uds":1,"lds":1}));
    }
    case["final"]["ram"].as_array_mut().unwrap().extend([
        json!([8192, 0x11]),
        json!([8193, 0x22]),
        json!([8194, 0x33]),
        json!([8195, 0x44]),
    ]);
    assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
    case["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(&serde_json::from_value(case).unwrap()).is_err());
}

#[test]
fn self_modifying_move_requires_coherent_postwrite_prefetch() {
    let mut case = move_case(0x1080, "MOVE.b");
    case["initial"]["d0"] = json!(0xab);
    case["final"]["d0"] = json!(0xab);
    case["initial"]["a0"] = json!(4100);
    case["final"]["a0"] = json!(4100);
    case["final"]["sr"] = json!(0x2008);
    case["final"]["ram"][4][1] = json!(0xab);
    case["final"]["prefetch"][1] = json!(0xab78);
    case["reference_total_cycles"] = json!(8);
    case["transactions"][0]["data_bus"] = json!(0xab78);
    case["transactions"].as_array_mut().unwrap().insert(0,json!({"kind":"write",
        "duration_cycles":4,"function_code":5,"address_bus":4100,"data_bus":0xab00,"uds":1,"lds":0}));
    case["normalization_exclusion"] = json!("data_prefetch_overlap");
    let report = verify_reader(std::io::Cursor::new(cohort_stream(case.clone()))).unwrap();
    assert_eq!(report.ledger["MOVE.b"].passed, 1);
    case["transactions"][1]["data_bus"] = json!(0x5678);
    let report = verify_reader(std::io::Cursor::new(cohort_stream(case))).unwrap();
    assert_eq!(report.failures, 1);
}

fn set_word(case: &mut Value, state: &str, address: u32, word: u16) {
    let ram = case[state]["ram"].as_array_mut().unwrap();
    ram.retain(|byte| byte[0] != json!(address) && byte[0] != json!(address + 1));
    ram.extend([
        json!([address, word >> 8]),
        json!([address + 1, word & 255]),
    ]);
}

fn extended_case(opcode: u16, extensions: &[u16]) -> Value {
    let mut case = move_case(opcode, "MOVE.w");
    case["initial"]["prefetch"][1] = json!(extensions[0]);
    let words: Vec<_> = extensions.iter().copied().chain([0x5555, 0x6666]).collect();
    for (index, word) in words.into_iter().enumerate() {
        for state in ["initial", "final"] {
            set_word(&mut case, state, 0x1002 + index as u32 * 2, word);
        }
    }
    case["final"]["pc"] = json!(0x1006 + extensions.len() * 2);
    case["final"]["prefetch"] = json!([0x5555, 0x6666]);
    case
}

fn read_phase(address: u32, word: u16, fc: u32) -> Value {
    json!({"kind":"read","duration_cycles":4,"function_code":fc,
        "address_bus":address,"data_bus":word,"uds":1,"lds":1})
}

fn write_phase(address: u32, word: u16) -> Value {
    json!({"kind":"write","duration_cycles":4,"function_code":5,
        "address_bus":address,"data_bus":word,"uds":1,"lds":1})
}

#[test]
fn move_extensions_require_fetched_words_and_initial_operand_memory() {
    let mut case = extended_case(0x3028, &[0xfffe]);
    for state in ["initial", "final"] {
        case[state]["a0"] = json!(0x2002);
        set_word(&mut case, state, 0x2000, 0x1234);
    }
    case["final"]["d0"] = json!(0x1234);
    case["reference_total_cycles"] = json!(12);
    case["transactions"] = json!([
        read_phase(0x1004, 0x5555, 6),
        read_phase(0x2000, 0x1234, 5),
        read_phase(0x1006, 0x6666, 6)
    ]);
    assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
    let mut bad = case.clone();
    bad["transactions"][0]["data_bus"] = json!(0xabcd);
    assert!(run(&serde_json::from_value(bad).unwrap()).is_err());
    case["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .retain(|byte| byte[0] != json!(0x2000));
    assert!(
        run(&serde_json::from_value(case).unwrap())
            .unwrap_err()
            .contains("unobserved read")
    );
}

#[test]
fn pc_relative_operand_uses_extension_address_and_program_space() {
    let mut case = extended_case(0x303a, &[0x100]);
    for state in ["initial", "final"] {
        set_word(&mut case, state, 0x1102, 0x1234);
    }
    case["final"]["d0"] = json!(0x1234);
    case["reference_total_cycles"] = json!(12);
    case["transactions"] = json!([
        read_phase(0x1004, 0x5555, 6),
        read_phase(0x1102, 0x1234, 6),
        read_phase(0x1006, 0x6666, 6)
    ]);
    assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
    case["transactions"][1]["function_code"] = json!(5);
    assert!(run(&serde_json::from_value(case).unwrap()).is_err());
}

#[test]
fn index_word_sign_extension_ignored_bits_and_pc_base_are_witnessed() {
    for (opcode, address, operand, fc) in [(0x3030, 0x2000, 0x1234, 5), (0x303b, 0x1002, 0x1702, 6)]
    {
        let mut case = extended_case(opcode, &[0x1702]);
        for state in ["initial", "final"] {
            case[state]["a0"] = json!(0x2000);
            case[state]["d1"] = json!(0xbeeffffe_u32);
            set_word(&mut case, state, address, operand);
        }
        case["final"]["d0"] = json!(operand);
        case["reference_total_cycles"] = json!(14);
        case["transactions"] = json!([{"kind":"idle","duration_cycles":2},
            read_phase(0x1004,0x5555,6),read_phase(address,operand,fc),read_phase(0x1006,0x6666,6)]);
        assert!(run(&serde_json::from_value(case.clone()).unwrap()).is_ok());
        case["transactions"][2]["address_bus"] = json!(address + 2);
        assert!(run(&serde_json::from_value(case).unwrap()).is_err());
    }
}

#[test]
fn absolute_long_destination_distinguishes_memory_register_and_immediate_sources() {
    for source in ["register", "memory", "immediate"] {
        let opcode = match source {
            "register" => 0x33c0,
            "memory" => 0x33d8,
            _ => 0x33fc,
        };
        let extensions = if source == "immediate" {
            vec![0x1234, 0, 0x2000]
        } else {
            vec![0, 0x2000]
        };
        let mut case = extended_case(opcode, &extensions);
        let mut phases = Vec::new();
        match source {
            "register" => {
                case["initial"]["d0"] = json!(0x1234);
                case["final"]["d0"] = json!(0x1234);
                phases.extend([
                    read_phase(0x1004, 0x2000, 6),
                    read_phase(0x1006, 0x5555, 6),
                    write_phase(0x2000, 0x1234),
                    read_phase(0x1008, 0x6666, 6),
                ]);
            }
            "memory" => {
                case["initial"]["a0"] = json!(0x3000);
                case["final"]["a0"] = json!(0x3002);
                for state in ["initial", "final"] {
                    set_word(&mut case, state, 0x3000, 0x1234);
                }
                phases.extend([
                    read_phase(0x3000, 0x1234, 5),
                    read_phase(0x1004, 0x2000, 6),
                    write_phase(0x2000, 0x1234),
                    read_phase(0x1006, 0x5555, 6),
                    read_phase(0x1008, 0x6666, 6),
                ]);
            }
            _ => phases.extend([
                read_phase(0x1004, 0, 6),
                read_phase(0x1006, 0x2000, 6),
                read_phase(0x1008, 0x5555, 6),
                write_phase(0x2000, 0x1234),
                read_phase(0x100a, 0x6666, 6),
            ]),
        }
        set_word(&mut case, "final", 0x2000, 0x1234);
        case["reference_total_cycles"] = json!(if source == "register" { 16 } else { 20 });
        case["transactions"] = json!(phases);
        assert!(
            run(&serde_json::from_value(case.clone()).unwrap()).is_ok(),
            "{source}"
        );
        case["transactions"].as_array_mut().unwrap().swap(1, 2);
        assert!(run(&serde_json::from_value(case).unwrap()).is_err());
    }
}
