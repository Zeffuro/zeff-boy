use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    AudioSettings, CameraSettings, EmulationSettings, InputDeviceSettings, InputProfileBindings,
    InputProfileCatalog, RecentRomEntry, RewindMode, RewindSettings, Settings, UiSettings,
    VideoSettings,
};
use crate::platform;

const SCHEMA_VERSION: u64 = 1;

/// The persisted document is independent of the runtime Settings adapter.
/// These are global defaults; only explicitly typed domains may gain system or
/// content overrides later. A host device reservation is never a core setting.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct SettingsDocument {
    schema_version: u64,
    global: GlobalPreferences,
    input_profiles: InputProfileCatalog,
    input_devices: InputDeviceSettings,
    extensions: Map<String, Value>,
}

impl Default for SettingsDocument {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct GlobalPreferences {
    emulation: EmulationSettings,
    interface: UiSettings,
    audio: AudioSettings,
    video: VideoSettings,
    rewind: RewindPreferences,
    camera: CameraSettings,
    input: InputProfileBindings,
    recent_roms: Vec<RecentRomEntry>,
}

impl Default for GlobalPreferences {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl GlobalPreferences {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            emulation: settings.emulation.clone(),
            interface: settings.ui.clone(),
            audio: settings.audio.clone(),
            video: settings.video.clone(),
            rewind: RewindPreferences::from(&settings.rewind),
            camera: settings.camera.clone(),
            input: settings.capture_input_profile(),
            recent_roms: settings.recent_roms.clone(),
        }
    }
}

impl SettingsDocument {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            global: GlobalPreferences::from_settings(settings),
            input_profiles: settings.input_profiles.clone(),
            input_devices: settings.input_devices.clone(),
            extensions: Map::new(),
        }
    }

    fn into_settings(self) -> Settings {
        let mut settings = Settings {
            emulation: self.global.emulation,
            ui: self.global.interface,
            audio: self.global.audio,
            video: self.global.video,
            rewind: self.global.rewind.into(),
            camera: self.global.camera,
            recent_roms: self.global.recent_roms,
            input_profiles: self.input_profiles,
            input_devices: self.input_devices,
            ..Settings::default()
        };
        settings.apply_input_profile(&self.global.input);
        settings
    }
}

/// Rewind policy and its host key binding have one owner each in the document.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct RewindPreferences {
    enabled: bool,
    speed: usize,
    seconds: usize,
    mode: RewindMode,
}

impl Default for RewindPreferences {
    fn default() -> Self {
        Self::from(&RewindSettings::default())
    }
}

impl From<&RewindSettings> for RewindPreferences {
    fn from(settings: &RewindSettings) -> Self {
        Self {
            enabled: settings.enabled,
            speed: settings.speed,
            seconds: settings.seconds,
            mode: settings.mode,
        }
    }
}

impl From<RewindPreferences> for RewindSettings {
    fn from(settings: RewindPreferences) -> Self {
        Self {
            enabled: settings.enabled,
            speed: settings.speed,
            seconds: settings.seconds,
            mode: settings.mode,
            ..Self::default()
        }
    }
}

#[derive(Debug, Default)]
struct PersistenceState {
    template: Option<Value>,
    previous_json: Option<String>,
    load_notice: Option<String>,
    save_error: Option<String>,
    read_only: bool,
    preserve_original: bool,
    load_notified: bool,
    notified_save_error: Option<String>,
}

/// Runtime storage diagnostics are shared by edit/undo snapshots, but excluded
/// from value equality: a save outcome is not a preference edit.
#[derive(Debug, Clone, Default)]
pub(super) struct PersistenceMetadata(Arc<Mutex<PersistenceState>>);

impl PartialEq for PersistenceMetadata {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl PersistenceMetadata {
    pub(super) fn take_notification(&self) -> Option<String> {
        let mut state = self.0.lock().ok()?;
        if !state.load_notified {
            state.load_notified = true;
            if state.load_notice.is_some() {
                return state.load_notice.clone();
            }
        }
        if state.notified_save_error != state.save_error {
            state.notified_save_error = state.save_error.clone();
            return state.save_error.clone();
        }
        None
    }

    pub(super) fn notice(&self) -> Option<String> {
        let state = self.0.lock().ok()?;
        match (&state.load_notice, &state.save_error) {
            (Some(load), Some(save)) => Some(format!("{load}\n{save}")),
            (Some(notice), None) | (None, Some(notice)) => Some(notice.clone()),
            (None, None) => None,
        }
    }
}

fn fresh_settings() -> Settings {
    let mut settings = Settings::default();
    settings.ui.ui_scale_needs_auto = true;
    settings
}

fn validate_profiles(settings: &Settings) -> Result<()> {
    let profiles = &settings.input_profiles.profiles;
    if profiles.len() > 64 {
        bail!("settings contain more than 64 input profiles");
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for profile in profiles {
        if profile.id.is_empty()
            || profile.id.len() > 128
            || profile.id.chars().any(char::is_control)
            || profile.name.trim().is_empty()
            || profile.name.chars().count() > 64
            || profile.name.chars().any(char::is_control)
            || !ids.insert(&profile.id)
            || !names.insert(profile.name.trim().to_lowercase())
        {
            bail!("settings contain an invalid or duplicate input profile");
        }
    }
    Ok(())
}

fn decode(json: &str) -> Result<Settings> {
    let raw: Value = serde_json::from_str(json).context("settings are not valid JSON")?;
    let object = raw.as_object().context("settings must be a JSON object")?;
    let version = object.get("schema_version");
    let mut settings;
    let mut template;
    let mut read_only = false;
    let mut load_notice = None;
    if let Some(version) = version {
        let version = version
            .as_u64()
            .context("settings schema version must be an integer")?;
        if version == 0 {
            bail!("unsupported settings schema version 0");
        }
        read_only = version > SCHEMA_VERSION;
        if read_only {
            load_notice = Some(format!(
                "These settings were saved by a newer version (schema {version}). Changes will not be saved. Update Zeff Boy to edit them."
            ));
        }
        match serde_json::from_value::<SettingsDocument>(raw.clone()) {
            Ok(document) => settings = document.into_settings(),
            Err(_) if read_only => settings = fresh_settings(),
            Err(error) => return Err(error).context("invalid settings document"),
        }
        template = raw;
    } else {
        settings = Settings::from_json(json).context("invalid legacy settings")?;
        template = serde_json::to_value(SettingsDocument::from_settings(&settings))?;
        let known = serde_json::to_value(&settings)?;
        let known = known.as_object().expect("Settings serializes as an object");
        let extensions = template["extensions"]
            .as_object_mut()
            .expect("extensions is an object");
        for (key, value) in object {
            if !known.contains_key(key) && key != "auto_save_state" {
                extensions.insert(key.clone(), value.clone());
            }
        }
    }
    if let Err(error) =
        validate_profiles(&settings).and_then(|()| super::validation::validate(&settings))
    {
        if read_only {
            settings = fresh_settings();
        } else {
            return Err(error);
        }
    }
    settings.video.migrate_shader_preset();
    settings.gamepad_bindings.migrate_wonderswan_defaults();
    for profile in &settings.input_profiles.profiles {
        if let Some(index) = profile
            .id
            .strip_prefix("profile-")
            .and_then(|id| id.parse::<u64>().ok())
        {
            settings.input_profiles.next_id =
                settings.input_profiles.next_id.max(index.saturating_add(1));
        }
    }
    settings.persistence = PersistenceMetadata(Arc::new(Mutex::new(PersistenceState {
        template: Some(template),
        previous_json: Some(json.to_owned()),
        load_notice,
        read_only,
        ..PersistenceState::default()
    })));
    Ok(settings)
}

pub(super) fn load() -> Settings {
    load_from(
        platform::load_settings_json(),
        platform::load_settings_backup_json,
    )
}

fn load_from(
    primary: Result<Option<String>>,
    backup: impl FnOnce() -> Result<Option<String>>,
) -> Settings {
    let failure = match primary {
        Ok(Some(json)) => match decode(&json) {
            Ok(settings) => return settings,
            Err(error) => format!("Could not read settings: {error:#}."),
        },
        Ok(None) => match backup().and_then(|json| json.map(|json| decode(&json)).transpose()) {
            Ok(Some(settings)) => {
                if let Ok(mut state) = settings.persistence.0.lock() {
                    let notice = "The main settings file was missing. Recovered the previous saved settings.";
                    state.load_notice = Some(match state.load_notice.take() {
                        Some(existing) => format!("{notice}\n{existing}"),
                        None => notice.to_owned(),
                    });
                }
                return settings;
            }
            Ok(None) => return fresh_settings(),
            Err(error) => {
                let settings = fresh_settings();
                if let Ok(mut state) = settings.persistence.0.lock() {
                    state.load_notice = Some(format!(
                        "Could not read the settings backup: {error:#}. Using defaults."
                    ));
                }
                return settings;
            }
        },
        Err(error) => format!("Could not read settings: {error:#}."),
    };
    let (settings, notice) =
        match backup().and_then(|json| json.map(|json| decode(&json)).transpose()) {
            Ok(Some(settings)) => (
                settings,
                format!("{failure} Recovered the previous saved settings."),
            ),
            _ => (
                fresh_settings(),
                format!("{failure} Using defaults; the original will be preserved before saving."),
            ),
        };
    if let Ok(mut state) = settings.persistence.0.lock() {
        state.load_notice = Some(match state.load_notice.take() {
            Some(existing) => format!("{notice}\n{existing}"),
            None => notice,
        });
        state.preserve_original = true;
    }
    settings
}

fn overlay_known(template: &mut Value, current: Value) {
    if let (Value::Object(old), Value::Object(new)) = (&mut *template, &current) {
        for (key, value) in new {
            if let Some(previous) = old.get_mut(key) {
                // These arrays have fixed port identities, rather than editable
                // list positions. Preserve additive fields on each matching port.
                if matches!(
                    key.as_str(),
                    "players" | "pce_multitap_keyboard" | "pce_multitap"
                ) && let (Some(old_ports), Some(new_ports)) =
                    (previous.as_array_mut(), value.as_array())
                    && old_ports.len() == new_ports.len()
                {
                    for (old_port, new_port) in old_ports.iter_mut().zip(new_ports) {
                        overlay_known(old_port, new_port.clone());
                    }
                    continue;
                }
                overlay_known(previous, value.clone());
            } else {
                old.insert(key.clone(), value.clone());
            }
        }
    } else {
        *template = current;
    }
}

fn encode(settings: &Settings, template: Option<&Value>) -> Result<String> {
    validate_profiles(settings)?;
    super::validation::validate(settings)?;
    let mut current = serde_json::to_value(SettingsDocument::from_settings(settings))?;
    // Preserve extensions only for surviving profile identities. Array position
    // is not identity, and deleted profiles must never reappear during a save.
    if let Some(old_profiles) = template
        .and_then(|value| value.pointer("/input_profiles/profiles"))
        .and_then(Value::as_array)
    {
        for profile in current["input_profiles"]["profiles"]
            .as_array_mut()
            .expect("profile array")
        {
            if let Some(previous) = old_profiles.iter().find(|old| old["id"] == profile["id"]) {
                let mut merged = previous.clone();
                overlay_known(&mut merged, profile.clone());
                *profile = merged;
            }
        }
    }
    let mut value = template.cloned().unwrap_or(Value::Null);
    overlay_known(&mut value, current);
    Ok(serde_json::to_string_pretty(&value)?)
}

pub(super) fn save(settings: &Settings) {
    let Ok(mut state) = settings.persistence.0.lock() else {
        log::error!("settings storage state lock is poisoned");
        return;
    };
    if state.read_only {
        return;
    }
    let result = encode(settings, state.template.as_ref()).and_then(|json| {
        platform::save_settings_json(
            &json,
            state.previous_json.as_deref(),
            state.preserve_original,
        )?;
        state.template = Some(serde_json::from_str(&json)?);
        state.previous_json = Some(json);
        state.preserve_original = false;
        Ok(())
    });
    state.save_error = result.err().map(|error| {
        log::error!("could not save settings: {error:#}");
        format!("Settings could not be saved: {error:#}. Changes may be lost. Retry after correcting the storage problem.")
    });
}

#[cfg(test)]
mod tests {
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
        assert_eq!(document["schema_version"], 1);
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
        let document: Value =
            serde_json::from_str(&encode(&restored, Some(&raw)).unwrap()).unwrap();
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
}
