use super::*;
use serde_json::{Value, json};

fn lower_byte() -> Value {
    let initial = json!({
        "d0":0,"d1":0,"d2":0,"d3":0,"d4":0,"d5":0,"d6":0,"d7":0,
        "a0":8193,"a1":0,"a2":0,"a3":0,"a4":0,"a5":0,"a6":0,
        "usp":0,"ssp":12288,"sr":8208,"pc":4100,"prefetch":[17936,20081],
        "ram":[[4096,70],[4097,16],[4098,78],[4099,113],[4100,112],[4101,1],[8192,170],[8193,127]]
    });
    let mut expected = initial.clone();
    expected["sr"] = json!(8216);
    expected["pc"] = json!(4102);
    expected["prefetch"] = json!([20081, 28673]);
    expected["ram"][7][1] = json!(128);
    json!({"cohort":"NOT.b","name":"lower-byte","initial":initial,"final":expected,
    "reference_total_cycles":12,"normalization_exclusion":null,
    "transactions":[
        {"kind":"read","duration_cycles":4,"function_code":5,"address_bus":8192,"data_bus":127,"uds":0,"lds":1},
        {"kind":"read","duration_cycles":4,"function_code":6,"address_bus":4100,"data_bus":28673,"uds":1,"lds":1},
        {"kind":"write","duration_cycles":4,"function_code":5,"address_bus":8192,"data_bus":128,"uds":0,"lds":1}
    ]})
}

fn run(raw: Value) -> Result<(), String> {
    verify(&serde_json::from_value(raw).unwrap())
}

#[test]
fn byte_completion_and_corrupted_reference_are_distinguished() {
    let original = lower_byte();
    assert!(run(original.clone()).is_ok());
    for (field, value) in [
        ("data_bus", 0xaa7f),
        ("lds", 0),
        ("function_code", 6),
        ("duration_cycles", 5),
    ] {
        let mut raw = original.clone();
        raw["transactions"][0][field] = json!(value);
        assert!(run(raw).is_err(), "{field}");
    }
    let mut raw = original.clone();
    raw["transactions"].as_array_mut().unwrap().swap(1, 2);
    assert!(run(raw).is_err());
    let mut raw = original.clone();
    raw["transactions"][2]["data_bus"] = json!(129);
    assert!(run(raw).is_err());
    let mut raw = original.clone();
    raw["final"]["ram"][6][1] = json!(0);
    assert!(run(raw).is_err());
    let mut raw = original.clone();
    raw["final"]["prefetch"][0] = json!(0);
    assert!(run(raw).is_err());
    let mut raw = original.clone();
    raw["initial"]["ram"]
        .as_array_mut()
        .unwrap()
        .push(json!([8193, 127]));
    assert!(run(raw).is_err());
    let mut raw = original;
    raw["initial"]["sr"] = json!(40976);
    assert!(run(raw).is_err());
}
