#[test]
fn browser_pending_and_failed_saves_do_not_advance_durable_metadata() {
    let mut state = super::PersistenceState {
        previous_json: Some("old".into()),
        template: Some(serde_json::json!({"old": true})),
        preserve_original: true,
        browser_pending_sequence: Some(2),
        ..Default::default()
    };
    super::apply_browser_status(
        &mut state,
        crate::platform::SettingsStorageStatus {
            generation: 1,
            pending: 1,
            latest_requested_sequence: 2,
            ..Default::default()
        },
    );
    assert_eq!(state.previous_json.as_deref(), Some("old"));
    assert!(state.preserve_original);
    super::apply_browser_status(
        &mut state,
        crate::platform::SettingsStorageStatus {
            generation: 2,
            error_sequence: Some(2),
            error: Some("full".into()),
            ..Default::default()
        },
    );
    assert_eq!(state.previous_json.as_deref(), Some("old"));
    assert!(state.preserve_original);
    assert!(state.save_error.as_deref().unwrap().contains("full"));
    super::apply_browser_status(
        &mut state,
        crate::platform::SettingsStorageStatus {
            generation: 3,
            persisted_sequence: 3,
            persisted_json: Some(r#"{"saved":true}"#.into()),
            ..Default::default()
        },
    );
    assert_eq!(state.previous_json.as_deref(), Some(r#"{"saved":true}"#));
    assert_eq!(state.template, Some(serde_json::json!({"saved": true})));
    assert!(!state.preserve_original);
    assert!(state.save_error.is_none());
    assert!(state.browser_pending_sequence.is_none());
}

#[test]
fn newer_browser_storage_blocks_writes_but_exports_current_local_edits() {
    let mut settings = super::decode(r#"{"schema_version":3}"#).unwrap();
    settings.audio.volume = 0.4;
    super::apply_browser_status(
        &mut settings.persistence.0.lock().unwrap(),
        crate::platform::SettingsStorageStatus {
            generation: 1,
            write_protected: true,
            ..Default::default()
        },
    );
    assert!(!settings.can_retry_settings_save());
    assert!(super::prepare_import(&settings, r#"{"schema_version":3}"#).is_err());
    assert!(super::try_save(&settings).is_err());
    let exported: serde_json::Value =
        serde_json::from_str(&super::export(&settings).unwrap()).unwrap();
    assert_eq!(
        exported["global"]["audio"]["master_volume"],
        serde_json::json!(0.4_f32)
    );
}

#[test]
fn import_preparation_validates_without_mutating_preferences_or_backup() {
    let original =
        super::decode(r#"{"schema_version":2,"global":{"audio":{"volume":0.4}}}"#).unwrap();
    let before = super::export(&original).unwrap();
    let candidate = super::prepare_import(
        &original,
        r#"{"schema_version":3,"extensions":{"kept":true}}"#,
    )
    .unwrap();
    assert_eq!(super::export(&original).unwrap(), before);
    assert_eq!(
        candidate.persistence.0.lock().unwrap().previous_json,
        original.persistence.0.lock().unwrap().previous_json
    );
    assert!(super::export(&candidate).unwrap().contains("kept"));
    assert!(super::prepare_import(&original, "broken").is_err());
    assert!(super::prepare_import(&original, r#"{"schema_version":999}"#).is_err());
    assert!(super::prepare_import(&original, &" ".repeat(4 * 1024 * 1024 + 1)).is_err());
    assert_eq!(super::export(&original).unwrap(), before);
    super::svp_tests::profile_import(&original);
}

#[test]
fn typed_document_reorder_delete_and_unbind_preserve_only_surviving_extensions() {
    use crate::settings::{
        BindingAction, BindingExpression, BindingSet, BindingTarget, GameplayBindingSource,
        InputScope, Settings,
    };
    use winit::keyboard::KeyCode;
    let mut settings = Settings::default();
    let target = BindingTarget::Joypad {
        player: 1,
        action: BindingAction::A,
    };
    let source = GameplayBindingSource::Keyboard;
    let mut set = BindingSet::new(BindingExpression::keyboard(KeyCode::KeyX));
    set.add_expression(BindingExpression::keyboard(KeyCode::KeyZ))
        .unwrap();
    settings
        .set_binding_set(&InputScope::Global, target, source, Some(set))
        .unwrap();
    let mut original: serde_json::Value =
        serde_json::from_str(&super::encode(&settings, None).unwrap()).unwrap();
    let record = &mut original["input_overrides"]["global_binding_sets"][source.key(target)];
    record["owner_extension"] = serde_json::json!(42);
    record["value"]["alternatives"][0]["extension"] = serde_json::json!("first");
    record["value"]["alternatives"][1]["extension"] = serde_json::json!("second");
    let mut decoded = super::decode(&original.to_string()).unwrap();
    let mut set = decoded
        .binding_set(&InputScope::Global, target, source)
        .value
        .unwrap();
    set.alternatives.reverse();
    set.alternatives.pop();
    decoded
        .set_binding_set(&InputScope::Global, target, source, Some(set))
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&super::encode(&decoded, Some(&original)).unwrap()).unwrap();
    let record = &saved["input_overrides"]["global_binding_sets"][source.key(target)];
    assert_eq!(record["value"]["alternatives"].as_array().unwrap().len(), 1);
    assert_eq!(record["value"]["alternatives"][0]["extension"], "second");
    assert_eq!(record["owner_extension"], 42);
    decoded
        .set_binding_set(&InputScope::Global, target, source, None)
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&super::encode(&decoded, Some(&original)).unwrap()).unwrap();
    let record = &saved["input_overrides"]["global_binding_sets"][source.key(target)];
    assert_eq!(record["state"], "unbound");
    assert!(record.get("value").is_none());
}

#[test]
fn v1_migration_retains_hotkeys_and_starts_with_no_scoped_overrides() {
    use crate::settings::{
        BindingAction, BindingTarget, GameplayBindingSource, InputScope, InputSystem,
        PhysicalBinding,
    };
    use winit::keyboard::KeyCode;
    let original = serde_json::json!({
        "schema_version": 1,
        "global": { "input": { "speedup_key": "KeyQ", "keyboard": { "a": "KeyW" } } },
        "input_profiles": { "profiles": [{"id":"old", "name":"Legacy", "bindings": {"speedup_key":"KeyE", "gamepad":{"pause":"North"}}}] }
    });
    let settings = super::decode(&original.to_string()).unwrap();
    assert_eq!(settings.speedup_key, "KeyQ");
    assert_eq!(
        settings.input_profiles.profiles[0].bindings.speedup_key,
        "KeyE"
    );
    assert_eq!(
        settings.input_profiles.profiles[0].bindings.gamepad.pause,
        "North"
    );
    let scope = InputScope::System(InputSystem::GameBoyAdvance);
    let binding = settings.binding(
        &scope,
        BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        },
        GameplayBindingSource::Keyboard,
    );
    assert_eq!(
        binding.value,
        Some(PhysicalBinding::Keyboard(KeyCode::KeyW))
    );
    assert_eq!(binding.origin, InputScope::Global);
    let encoded: serde_json::Value =
        serde_json::from_str(&super::encode(&settings, Some(&original)).unwrap()).unwrap();
    assert_eq!(encoded["schema_version"], SCHEMA_VERSION);
    assert!(
        encoded["input_overrides"]["systems"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sparse_override_deletions_survive_save_without_losing_surviving_extensions() {
    use crate::settings::{
        BindingAction, BindingTarget, GameplayBindingSource, InputScope, InputSystem,
        PhysicalBinding, Settings,
    };
    use winit::keyboard::KeyCode;
    let mut settings = Settings::default();
    let scope = InputScope::System(InputSystem::GameBoyAdvance);
    let target = BindingTarget::Joypad {
        player: 1,
        action: BindingAction::A,
    };
    settings
        .set_binding(
            &scope,
            target,
            GameplayBindingSource::Keyboard,
            Some(PhysicalBinding::Keyboard(KeyCode::KeyX)),
        )
        .unwrap();
    settings
        .set_binding(
            &InputScope::Global,
            target,
            GameplayBindingSource::Keyboard,
            None,
        )
        .unwrap();
    let mut template: serde_json::Value =
        serde_json::from_str(&super::encode(&settings, None).unwrap()).unwrap();
    template["input_overrides"]["systems"]["gba"]["future_field"] =
        serde_json::json!({"keep": true});
    template["input_overrides"]["systems"]["gba"]["bindings"]["keyboard/p1.a"]["extension"] =
        7.into();
    let mut loaded = super::decode(&template.to_string()).unwrap();
    assert!(
        loaded
            .binding(&InputScope::Global, target, GameplayBindingSource::Keyboard)
            .value
            .is_none()
    );
    loaded
        .set_binding(&scope, target, GameplayBindingSource::Keyboard, None)
        .unwrap();
    let unbound: serde_json::Value =
        serde_json::from_str(&super::encode(&loaded, Some(&template)).unwrap()).unwrap();
    let entry = &unbound["input_overrides"]["systems"]["gba"]["bindings"]["keyboard/p1.a"];
    assert_eq!(entry["state"], "unbound");
    assert_eq!(entry["extension"], 7);
    assert!(entry.get("value").is_none());
    loaded.inherit_binding(&scope, target, GameplayBindingSource::Keyboard);
    let encoded: serde_json::Value =
        serde_json::from_str(&super::encode(&loaded, Some(&unbound)).unwrap()).unwrap();
    assert!(
        encoded["input_overrides"]["systems"]["gba"]["bindings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        encoded["input_overrides"]["systems"]["gba"]["future_field"]["keep"],
        true
    );
    loaded.reset_input_scope(&scope);
    let reset: serde_json::Value =
        serde_json::from_str(&super::encode(&loaded, Some(&encoded)).unwrap()).unwrap();
    assert!(reset["input_overrides"]["systems"].get("gba").is_none());
}
use super::*;
use winit::keyboard::KeyCode;

#[test]
fn profile_extensions_follow_identity_and_deleted_ids_are_not_reused() {
    let mut settings = Settings::default();
    let removed = settings.save_input_profile("Removed").unwrap();
    let surviving = settings.save_input_profile("Surviving").unwrap();
    let mut raw: Value = serde_json::from_str(&encode(&settings, None).unwrap()).unwrap();
    raw["input_profiles"]["profiles"][0]["future"] = "removed".into();
    raw["input_profiles"]["profiles"][1]["bindings"]["future"] = "survives".into();
    raw["input_profiles"]["profiles"][1]["bindings"]["gamepad"]["pce_multitap"][0]["future_port"] =
        true.into();
    let mut restored = decode(&raw.to_string()).unwrap();
    restored.delete_input_profile(&removed);
    let added = restored.save_input_profile("Added").unwrap();
    assert_ne!(removed, added);
    let encoded = encode(&restored, Some(&raw)).unwrap();
    let output: Value = serde_json::from_str(&encoded).unwrap();
    let profiles = output["input_profiles"]["profiles"].as_array().unwrap();
    assert_eq!(profiles[0]["id"], surviving);
    assert_eq!(profiles[0]["bindings"]["future"], "survives");
    assert_eq!(
        profiles[0]["bindings"]["gamepad"]["pce_multitap"][0]["future_port"],
        true
    );
    assert!(profiles[1].get("future").is_none());
    let mut reloaded = decode(&encoded).unwrap();
    reloaded.reset_preferences();
    assert_ne!(reloaded.save_input_profile("After reset").unwrap(), removed);
}

#[test]
fn calibration_extensions_follow_fingerprint_identity_and_deletion() {
    let alpha = crate::settings::GamepadFingerprint {
        name: "Alpha Controller".into(),
        uuid: "alpha-model".into(),
    };
    let beta = crate::settings::GamepadFingerprint {
        name: "Beta Controller".into(),
        uuid: "beta-model".into(),
    };
    let mut settings = Settings::default();
    settings.input_devices.set_calibration(
        alpha.clone(),
        crate::settings::GamepadCalibration::default(),
    );
    settings
        .input_devices
        .set_calibration(beta.clone(), crate::settings::GamepadCalibration::default());
    let mut raw: Value = serde_json::from_str(&encode(&settings, None).unwrap()).unwrap();
    let records = raw["input_devices"]["calibrations"].as_array_mut().unwrap();
    for record in records {
        let model = record["fingerprint"]["uuid"].as_str().unwrap().to_owned();
        record["future_model"] = format!("{model}-outer").into();
        record["fingerprint"]["future_fingerprint"] = format!("{model}-fingerprint").into();
        record["calibration"]["left"]["x"]["future_axis"] = format!("{model}-axis").into();
    }

    let mut restored = decode(&raw.to_string()).unwrap();
    restored.input_devices.calibrations.reverse();
    restored
        .input_devices
        .calibrations
        .iter_mut()
        .find(|entry| entry.fingerprint == alpha)
        .unwrap()
        .calibration
        .left
        .x
        .center = 0.2;
    let reordered_json = encode(&restored, Some(&raw)).unwrap();
    let reordered: Value = serde_json::from_str(&reordered_json).unwrap();
    let records = reordered["input_devices"]["calibrations"]
        .as_array()
        .unwrap();
    assert_eq!(records[0]["fingerprint"]["uuid"], "beta-model");
    assert_eq!(records[0]["future_model"], "beta-model-outer");
    assert_eq!(
        records[0]["fingerprint"]["future_fingerprint"],
        "beta-model-fingerprint"
    );
    assert_eq!(
        records[0]["calibration"]["left"]["x"]["future_axis"],
        "beta-model-axis"
    );
    assert_eq!(records[1]["fingerprint"]["uuid"], "alpha-model");
    assert_eq!(records[1]["future_model"], "alpha-model-outer");
    assert_eq!(
        records[1]["fingerprint"]["future_fingerprint"],
        "alpha-model-fingerprint"
    );
    assert_eq!(
        records[1]["calibration"]["left"]["x"]["future_axis"],
        "alpha-model-axis"
    );
    assert_eq!(
        records[1]["calibration"]["left"]["x"]["center"],
        serde_json::json!(0.2_f32)
    );

    let mut roundtrip = decode(&reordered_json).unwrap();
    assert!(roundtrip.input_devices.remove_calibration(&beta));
    let surviving_json = encode(&roundtrip, Some(&reordered)).unwrap();
    let surviving: Value = serde_json::from_str(&surviving_json).unwrap();
    let records = surviving["input_devices"]["calibrations"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["fingerprint"]["uuid"], "alpha-model");
    assert_eq!(records[0]["future_model"], "alpha-model-outer");
    assert_eq!(
        records[0]["fingerprint"]["future_fingerprint"],
        "alpha-model-fingerprint"
    );
    assert_eq!(
        records[0]["calibration"]["left"]["x"]["future_axis"],
        "alpha-model-axis"
    );
    assert_eq!(decode(&surviving_json).unwrap(), roundtrip);
}

#[test]
fn invalid_numeric_settings_recover_and_future_documents_remain_read_only() {
    let mut invalid: Value =
        serde_json::from_str(&encode(&Settings::default(), None).unwrap()).unwrap();
    invalid["global"]["rewind"]["seconds"] = 999999999u64.into();
    assert!(decode(&invalid.to_string()).is_err());
    let recovered = load_from(Ok(Some(invalid.to_string())), || Ok(None));
    assert_eq!(recovered.rewind.seconds, Settings::default().rewind.seconds);
    assert!(recovered.take_persistence_notification().is_some());
    assert!(recovered.take_persistence_notification().is_none());
    assert!(recovered.persistence_notice().is_some());
    invalid["schema_version"] = 999.into();
    let future = decode(&invalid.to_string()).unwrap();
    assert!(future.persistence.0.lock().unwrap().read_only);
}

#[test]
fn save_errors_notify_once_per_transition() {
    let metadata = PersistenceMetadata::default();
    metadata.0.lock().unwrap().save_error = Some("Storage full".into());
    assert_eq!(
        metadata.take_notification().as_deref(),
        Some("Storage full")
    );
    assert!(metadata.take_notification().is_none());
    metadata.0.lock().unwrap().save_error = None;
    assert!(metadata.take_notification().is_none());
    metadata.0.lock().unwrap().save_error = Some("Storage full".into());
    assert!(metadata.take_notification().is_some());
}

#[test]
fn legacy_migration_preserves_effective_values_and_unknown_extensions() {
    let mut legacy = Settings::default();
    legacy.key_bindings.a = KeyCode::KeyQ;
    legacy.audio.volume = 0.3;
    legacy.emulation.fast_forward_multiplier = 9;
    legacy.tilt.deadzone = 0.22;
    legacy.ui.settings_window_size = [1200, 800];
    let mut raw = serde_json::to_value(&legacy).unwrap();
    raw["future_plugin_preference"] = serde_json::json!({"enabled": true});
    let migrated = decode(&raw.to_string()).unwrap();
    assert_eq!(migrated, legacy);
    let state = migrated.persistence.0.lock().unwrap();
    let encoded = encode(&migrated, state.template.as_ref()).unwrap();
    let document: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(document["schema_version"], SCHEMA_VERSION);
    assert_eq!(
        document["extensions"]["future_plugin_preference"]["enabled"],
        true
    );
    assert_eq!(decode(&encoded).unwrap(), legacy);
}

#[test]
fn nested_extensions_survive_edits_and_profile_deletion_is_real() {
    let mut settings = Settings::default();
    settings.save_input_profile("Original").unwrap();
    let mut raw: Value = serde_json::from_str(&encode(&settings, None).unwrap()).unwrap();
    raw["global"]["audio"]["future_audio_option"] = 7.into();
    let mut restored = decode(&raw.to_string()).unwrap();
    restored.audio.volume = 0.75;
    restored.delete_input_profile("profile-1");
    let document: Value = serde_json::from_str(&encode(&restored, Some(&raw)).unwrap()).unwrap();
    assert_eq!(document["global"]["audio"]["future_audio_option"], 7);
    assert!(
        document["input_profiles"]["profiles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(decode(&document.to_string()).unwrap().audio.volume, 0.75);
}

#[test]
fn future_schema_is_read_only_even_after_reset() {
    let mut settings = decode(r#"{"schema_version":999,"future":true}"#).unwrap();
    settings.reset_preferences();
    assert!(settings.persistence.0.lock().unwrap().read_only);
    assert!(
        settings
            .persistence_notice()
            .unwrap()
            .contains("newer version")
    );
}

#[test]
fn invalid_primary_recovers_backup_and_keeps_a_visible_notice() {
    let mut previous = Settings::default();
    previous.audio.volume = 0.42;
    let backup = encode(&previous, None).unwrap();
    let recovered = load_from(Ok(Some("{truncated".into())), || Ok(Some(backup)));
    assert_eq!(recovered.audio.volume, 0.42);
    assert!(
        recovered
            .persistence_notice()
            .unwrap()
            .contains("Recovered")
    );
    assert!(recovered.persistence.0.lock().unwrap().preserve_original);
    assert_eq!(previous, recovered);
}

#[test]
fn missing_primary_recovers_backup_but_first_run_is_quiet() {
    let mut previous = Settings::default();
    previous.audio.volume = 0.42;
    let recovered = load_from(Ok(None), || Ok(Some(encode(&previous, None).unwrap())));
    assert_eq!(recovered.audio.volume, 0.42);
    assert!(recovered.persistence_notice().unwrap().contains("missing"));
    let first_run = load_from(Ok(None), || Ok(None));
    assert!(first_run.persistence_notice().is_none());
}

#[test]
fn legacy_recovery_policy_migration_survives_the_new_envelope() {
    let legacy = decode(r#"{"auto_save_state":true}"#).unwrap();
    assert!(legacy.emulation.save_recovery_state);
    assert!(!legacy.emulation.resume_recovery_state);
    assert!(legacy.emulation.recovery_migration_notice_pending);
    let restored = decode(&encode(&legacy, None).unwrap()).unwrap();
    assert_eq!(legacy.emulation, restored.emulation);
}
