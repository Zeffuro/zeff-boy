use super::*;

pub(super) fn profile_import(original: &Settings) {
    assert!(original.emulation.md_svp_functional_profile.is_none());
    let raw_profile = serde_json::json!({
        "identity": 421,
        "numerator": 1,
        "denominator": 3,
        "costs": [1, 2, 3, 4, 5, 6],
    });
    let mut document = serde_json::json!({
        "schema_version": 3,
        "global": {"emulation": {"md_svp_functional_profile": raw_profile.clone()}},
    });
    let mut activated = prepare_import(original, &document.to_string()).unwrap();
    assert_eq!(
        activated
            .emulation
            .md_svp_functional_profile
            .unwrap()
            .core_profile(),
        zeff_md_core::svp::FunctionalProfile::new(421, 1, 3, [1, 2, 3, 4, 5, 6]).unwrap()
    );
    let enabled_json = export(&activated).unwrap();
    let exported: serde_json::Value = serde_json::from_str(&enabled_json).unwrap();
    assert_eq!(
        exported["global"]["emulation"]["md_svp_functional_profile"],
        raw_profile
    );
    let restored = decode(&enabled_json).unwrap();
    assert_eq!(
        restored.emulation.md_svp_functional_profile,
        activated.emulation.md_svp_functional_profile
    );
    document["global"]["emulation"]["md_svp_functional_profile"]["numerator"] =
        serde_json::json!(0);
    assert!(prepare_import(&activated, &document.to_string()).is_err());
    document["global"]["emulation"]["md_svp_functional_profile"] =
        serde_json::json!({"identity": 421});
    assert!(prepare_import(&activated, &document.to_string()).is_err());
    assert_eq!(export(&activated).unwrap(), enabled_json);
    activated.emulation.md_svp_functional_profile = None;
    let disabled_json = export(&activated).unwrap();
    let disabled: serde_json::Value = serde_json::from_str(&disabled_json).unwrap();
    assert_eq!(
        disabled["global"]["emulation"]["md_svp_functional_profile"],
        serde_json::Value::Null
    );
    assert!(
        decode(&disabled_json)
            .unwrap()
            .emulation
            .md_svp_functional_profile
            .is_none()
    );
}
