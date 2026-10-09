use super::*;

const FEATURES: &str = "multivalue,mutable-globals,nontrapping-fptoint,reference-types,sign-ext";

fn mixed_payload() -> Value {
    let mut payload = payload();
    payload["format"] = json!(3);
    payload["fp_controls"] = json!(0);
    payload["members"][0]["execution"] = json!({
        "fp_contract": "x86_64-mxcsr-rne-gradual-v1",
        "compiler": "ab".repeat(32), "target_features": "fxsr,sse,sse2"
    });
    payload["members"][1]["target"] = json!(WASM);
    payload["members"][1]["execution"] = json!({
        "fp_contract": "wasm32-core-rne-gradual-v1",
        "compiler": "cd".repeat(32), "target_features": FEATURES
    });
    payload
}

fn wasm_facts() -> LocalFacts<'static> {
    LocalFacts {
        artifact: [0x23; 32],
        target: WASM,
        test: false,
        profile: "release",
        opt_level: "3",
        debug: "false",
        compiler: "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        target_features: FEATURES,
        ..facts()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn browser_netplay_certificate_mixed_members_are_mutual_and_game_specific() {
    let payload = mixed_payload();
    let native = parse(&payload, &facts()).unwrap();
    let wasm = parse(&payload, &wasm_facts()).unwrap();
    assert_eq!(native.summary().certificate, wasm.summary().certificate);
    assert_eq!(native.summary().peer_build, wasm_facts().artifact);
    assert_eq!(wasm.summary().peer_build, facts().artifact);
    assert!(wasm.permits_content([0x45; 32], 0, 0, 0, true));
    assert!(!wasm.permits_content([0x99; 32], 0, 0, 0, true));
    assert!(!wasm.permits_content([0x45; 32], 0, 0, 1, true));
    assert!(!wasm.permits_content([0x45; 32], 0, 1, 0, true));
    for (source, mapper) in [(0x4a, 1), (0x4b, 3)] {
        assert!(native.permits_content([source; 32], mapper, 0, 0, true));
        assert!(wasm.permits_content([source; 32], mapper, 0, 0, true));
        assert!(!wasm.permits_content([0x99; 32], mapper, 0, 0, true));
        assert!(!wasm.permits_content([source; 32], mapper, 1, 0, true));
        assert!(!wasm.permits_content([source; 32], mapper, 0, 1, true));
        assert!(!wasm.permits_content([source; 32], mapper + 1, 0, 0, true));
    }
    let mut reversed = payload.clone();
    reversed["members"].as_array_mut().unwrap().reverse();
    assert_eq!(
        parse(&reversed, &facts()).unwrap().summary().peer_build,
        wasm_facts().artifact
    );
    let mut linux = payload;
    linux["members"][0]["target"] = json!(LINUX);
    assert!(
        parse(
            &linux,
            &LocalFacts {
                target: LINUX,
                ..facts()
            }
        )
        .is_ok()
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn browser_netplay_certificate_mixed_execution_and_coverage_fail_closed() {
    for (pointer, value) in [
        ("/fp_controls", json!(REQUIRED_CONTROLS)),
        ("/core", json!("nes-standard-portable-rollback-v1")),
        ("/games", json!([])),
        ("/members/0/target", json!(WASM)),
        ("/members/1/target", json!(LINUX)),
        ("/members/0/execution", Value::Null),
        ("/members/1/execution", Value::Null),
        (
            "/members/0/execution/fp_contract",
            json!("wasm32-core-rne-gradual-v1"),
        ),
        (
            "/members/1/execution/fp_contract",
            json!("x86_64-mxcsr-rne-gradual-v1"),
        ),
        ("/members/1/execution/fp_contract", json!("any")),
        ("/members/1/execution/compiler", json!("00".repeat(32))),
        ("/members/1/execution/compiler", json!("ef".repeat(32))),
        ("/members/1/execution/target_features", json!("simd128")),
        (
            "/members/1/execution/target_features",
            json!("relaxed-simd"),
        ),
        ("/members/1/execution/target_features", json!("bulk-memory")),
        (
            "/members/1/execution/target_features",
            json!("unknown-future"),
        ),
        (
            "/members/1/execution/target_features",
            json!("sign-ext,reference-types"),
        ),
        (
            "/members/1/execution/target_features",
            json!("sign-ext,sign-ext"),
        ),
        ("/members/1/execution/target_features", json!("Sign-Ext")),
        ("/members/1/execution/target_features", json!("sign-ext,")),
    ] {
        let mut changed = mixed_payload();
        if pointer == "/core" {
            changed["core"] = value;
        } else {
            *changed.pointer_mut(pointer).unwrap() = value;
        }
        assert!(parse(&changed, &wasm_facts()).is_err(), "{pointer}");
    }
    let mut legacy = mixed_payload();
    legacy["format"] = json!(1);
    legacy["fp_controls"] = json!(REQUIRED_CONTROLS);
    assert!(parse(&legacy, &facts()).is_err());
    for changed in [
        LocalFacts {
            compiler: "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
            ..wasm_facts()
        },
        LocalFacts {
            target_features: "sign-ext",
            ..wasm_facts()
        },
        LocalFacts {
            artifact: [0x98; 32],
            ..wasm_facts()
        },
    ] {
        assert!(parse(&mixed_payload(), &changed).is_err());
    }
    assert!(verify(&signed(&mixed_payload().to_string()), &wasm_facts()).is_err());
}
