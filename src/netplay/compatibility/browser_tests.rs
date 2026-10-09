use super::*;
use wasm_bindgen::JsValue;

#[wasm_bindgen_test::wasm_bindgen_test]
fn browser_netplay_certificate_missing_and_invalid_metadata_keep_exact_builds() {
    struct Restore(Vec<(&'static str, JsValue)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                js_sys::Reflect::set(&js_sys::global(), &(*name).into(), value).unwrap();
            }
        }
    }
    let names = [
        "zeffBoyBundleIdentity",
        "zeffBoyPairCertificate",
        "zeffBoyPairCertificateError",
    ];
    let _restore = Restore(
        names
            .iter()
            .map(|name| (*name, global(name).unwrap()))
            .collect(),
    );
    let set = |name: &str, value: JsValue| {
        js_sys::Reflect::set(&js_sys::global(), &name.into(), &value).unwrap();
    };
    let bytes = zeff_netplay::fixture::rom();
    let path = std::path::Path::new("browser-pair-metadata.nes");
    let backend = crate::emu_backend::load_backend_from_rom_source(
        crate::emu_backend::ActiveSystem::Nes,
        path,
        path,
        Some(bytes.clone()),
        crate::emu_backend::BackendLoadConfig {
            sample_rate: Some(48_000),
            netplay_browser_media: crate::emu_backend::loader::NetplayRomMedia::browser(&bytes),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    set(names[0], const_hex::encode([7; 32]).into());
    set(names[1], JsValue::NULL);
    set(names[2], JsValue::NULL);
    assert!(certificate_status().unwrap().is_none());
    assert!(!signed_pair_available());
    assert!(!available());
    for consent in [false, true] {
        assert_eq!(
            describe(&backend, consent),
            BuildInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                allow_different_versions: consent,
                ..BuildInfo::default()
            }
        );
    }
    for (certificate, error) in [
        (JsValue::from_str("{}"), JsValue::NULL),
        (JsValue::from_str(&"x".repeat(64 * 1024 + 1)), JsValue::NULL),
        (JsValue::TRUE, JsValue::NULL),
        (JsValue::NULL, JsValue::from_str("timeout")),
    ] {
        set(names[1], certificate);
        set(names[2], error);
        assert!(certificate_status().is_err());
        assert!(!signed_pair_available());
        assert!(!available());
        assert_eq!(describe(&backend, false).contract, [0; 32]);
        assert_eq!(describe(&backend, false).platform, 0);
    }
}
