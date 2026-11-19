use crate::negate::{Selection, verify, verify_ordinary_completion};
use crate::reference::Case;
use serde_json::{Value, json};

fn byte_case(extend: bool, lower: bool, supervisor: bool) -> Value {
    let opcode = if extend { 0x4010 } else { 0x4410 };
    let address = 0x2000 + u32::from(lower);
    let sr = 0x14 | if supervisor { 0x2000 } else { 0 };
    let bytes = if lower { [0xa5, 0xff] } else { [0xff, 0xa5] };
    let initial = json!({
        "d0":0,"d1":0,"d2":0,"d3":0,"d4":0,"d5":0,"d6":0,"d7":0,
        "a0":address,"a1":0,"a2":0,"a3":0,"a4":0,"a5":0,"a6":0,
        "usp":0x4000,"ssp":0x6000,"sr":sr,"pc":0x1004,"prefetch":[opcode,0x4e71],
        "ram":[[0x1000,opcode >> 8],[0x1001,opcode & 255],[0x1002,0x4e],[0x1003,0x71],
               [0x1004,0x70],[0x1005,1],[0x2000,bytes[0]],[0x2001,bytes[1]]]
    });
    let result = u16::from(!extend);
    let mut expected = initial.clone();
    expected["pc"] = json!(0x1006);
    expected["sr"] = json!((sr & !31) | if extend { 0x15 } else { 0x11 });
    expected["prefetch"] = json!([0x4e71, 0x7001]);
    expected["ram"][6 + usize::from(lower)][1] = json!(result);
    let data_fc = if supervisor { 5 } else { 1 };
    let program_fc = if supervisor { 6 } else { 2 };
    let read = if lower { 255 } else { 0xff00 };
    let write = if lower { result } else { result << 8 };
    json!({"cohort":if extend {"NEGX.b"} else {"NEG.b"},"name":"byte-lane",
    "initial":initial,"final":expected,"reference_total_cycles":12,"normalization_exclusion":null,
    "transactions":[
        {"kind":"read","duration_cycles":4,"function_code":data_fc,"address_bus":0x2000,
         "data_bus":read,"uds":u8::from(!lower),"lds":u8::from(lower)},
        {"kind":"read","duration_cycles":4,"function_code":program_fc,"address_bus":0x1004,
         "data_bus":0x7001,"uds":1,"lds":1},
        {"kind":"write","duration_cycles":4,"function_code":data_fc,"address_bus":0x2000,
         "data_bus":write,"uds":u8::from(!lower),"lds":u8::from(lower)}
    ]})
}

fn run(value: Value) -> Result<(), String> {
    let case: Case = serde_json::from_value(value).unwrap();
    verify(&case, Selection::Completion)?;
    verify_ordinary_completion(&case)
}

fn reject_both(value: Value) {
    let case: Case = serde_json::from_value(value).unwrap();
    assert!(verify(&case, Selection::Completion).is_err());
    assert!(verify_ordinary_completion(&case).is_err());
}

#[test]
fn byte_lanes_require_selected_memory_and_preserve_other_memory() {
    for extend in [false, true] {
        for lower in [false, true] {
            for supervisor in [false, true] {
                let original = byte_case(extend, lower, supervisor);
                run(original.clone()).unwrap();
                let mut raw = original.clone();
                raw["initial"]["ram"]
                    .as_array_mut()
                    .unwrap()
                    .remove(6 + usize::from(lower));
                reject_both(raw);
                let mut raw = original.clone();
                for state in ["initial", "final"] {
                    raw[state]["ram"]
                        .as_array_mut()
                        .unwrap()
                        .remove(6 + usize::from(!lower));
                }
                run(raw).unwrap();
                let mut raw = original.clone();
                raw["final"]["ram"][6 + usize::from(!lower)][1] = json!(0);
                reject_both(raw);
                let mut raw = original.clone();
                raw["transactions"][0]["data_bus"] = json!(if lower { 0xa5ff } else { 0xffa5 });
                reject_both(raw);
                for index in [0, 2] {
                    let mut raw = original.clone();
                    raw["transactions"][index]["uds"] = json!(u8::from(lower));
                    raw["transactions"][index]["lds"] = json!(u8::from(!lower));
                    reject_both(raw);
                }
            }
        }
    }
}
