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

const SCHEMA_VERSION: u64 = 3;

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct SettingsDocument {
    schema_version: u64,
    global: GlobalPreferences,
    input_profiles: InputProfileCatalog,
    input_devices: InputDeviceSettings,
    input_overrides: super::InputOverrides,
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
        let mut input = settings.capture_input_profile();
        input.keyboard_unbound.clear();
        input.binding_sets.clear();
        input.autofire.clear();
        Self {
            emulation: settings.emulation.clone(),
            interface: settings.ui.clone(),
            audio: settings.audio.clone(),
            video: settings.video.clone(),
            rewind: RewindPreferences::from(&settings.rewind),
            camera: settings.camera.clone(),
            input,
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
            input_overrides: settings.input_overrides.clone(),
            extensions: Map::new(),
        }
    }

    fn into_settings(self) -> Settings {
        let global_keyboard_unbound = self.input_overrides.global_keyboard_unbound.clone();
        let global_binding_sets = self.input_overrides.global_binding_sets.clone();
        let global_autofire = self.input_overrides.global_autofire.clone();
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
            input_overrides: self.input_overrides,
            ..Settings::default()
        };
        settings.apply_input_profile(&self.global.input);
        settings.input_overrides.global_keyboard_unbound = global_keyboard_unbound;
        settings.input_overrides.global_binding_sets = global_binding_sets;
        settings.input_overrides.global_autofire = global_autofire;
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

#[derive(Debug, Clone, Default)]
struct PersistenceState {
    template: Option<Value>,
    previous_json: Option<String>,
    load_notice: Option<String>,
    save_error: Option<String>,
    read_only: bool,
    storage_write_protected: bool,
    preserve_original: bool,
    load_notified: bool,
    notified_save_error: Option<String>,
    #[cfg(any(target_arch = "wasm32", test))]
    browser_generation: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    browser_pending_sequence: Option<u64>,
    #[cfg(any(target_arch = "wasm32", test))]
    browser_persisted_sequence: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    browser_pending_count: usize,
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
    pub(super) fn can_retry_save(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let Ok(state) = self.0.lock() else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        {
            refresh_browser_status(&mut state);
            if state.browser_pending_count > 0 {
                return false;
            }
        }
        state.save_error.is_some() && !state.read_only && !state.storage_write_protected
    }
    pub(super) fn take_notification(&self) -> Option<String> {
        let mut state = self.0.lock().ok()?;
        #[cfg(target_arch = "wasm32")]
        refresh_browser_status(&mut state);
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
        #[cfg(not(target_arch = "wasm32"))]
        let state = self.0.lock().ok()?;
        #[cfg(target_arch = "wasm32")]
        let mut state = self.0.lock().ok()?;
        #[cfg(target_arch = "wasm32")]
        {
            refresh_browser_status(&mut state);
            if state.browser_pending_count > 0
                && state.save_error.is_none()
                && state.load_notice.is_none()
            {
                return Some(
                    "Saving settings to browser storage… Keep this tab open until saving finishes."
                        .to_owned(),
                );
            }
        }
        match (&state.load_notice, &state.save_error) {
            (Some(load), Some(save)) => Some(format!("{load}\n{save}")),
            (Some(notice), None) | (None, Some(notice)) => Some(notice.clone()),
            (None, None) => None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn refresh_browser_status(state: &mut PersistenceState) {
    if state.browser_generation == platform::settings_storage_generation() {
        return;
    }
    let status = platform::settings_storage_status();
    apply_browser_status(state, status);
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_browser_status(state: &mut PersistenceState, status: platform::SettingsStorageStatus) {
    state.browser_generation = status.generation;
    state.browser_pending_count = status.pending;
    state.storage_write_protected |= status.write_protected;
    if status.persisted_sequence > state.browser_persisted_sequence {
        state.browser_persisted_sequence = status.persisted_sequence;
        if let Some(json) = status.persisted_json {
            state.template = serde_json::from_str(&json).ok();
            state.previous_json = Some(json);
            state.preserve_original = false;
        }
    }
    if state.storage_write_protected {
        state.save_error = Some("Browser settings use a newer storage format. Export your current changes and update Zeff Boy before saving.".to_owned());
    } else if let Some(error) = status.error {
        state.save_error = Some(format!(
            "Settings could not be saved: {error}. Export your changes before reloading."
        ));
    } else if state
        .browser_pending_sequence
        .is_some_and(|sequence| status.persisted_sequence >= sequence)
    {
        state.save_error = None;
        state.browser_pending_sequence = None;
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
    let discarded_calibrations = settings.input_devices.discard_invalid_calibrations();
    if discarded_calibrations > 0 {
        let notice = format!(
            "Ignored {discarded_calibrations} invalid controller calibration record(s). Normalized device input will be used until recalibrated."
        );
        load_notice = Some(match load_notice.take() {
            Some(existing) => format!("{existing}\n{notice}"),
            None => notice,
        });
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

fn calibration_identity(value: &Value) -> Option<(&str, &str)> {
    Some((
        value.pointer("/fingerprint/name")?.as_str()?,
        value.pointer("/fingerprint/uuid")?.as_str()?,
    ))
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
                if let Some(old_bindings) =
                    merged.get_mut("bindings").and_then(Value::as_object_mut)
                {
                    old_bindings.remove("binding_sets");
                    old_bindings.remove("autofire");
                }
                overlay_known(&mut merged, profile.clone());
                *profile = merged;
            }
        }
    }
    // Calibration records are an editable model list. Preserve extensions by
    // the known fingerprint identity so reordering cannot transfer fields and
    // deleting a model cannot restore its old record.
    if let Some(old_calibrations) = template
        .and_then(|value| value.pointer("/input_devices/calibrations"))
        .and_then(Value::as_array)
    {
        for calibration in current["input_devices"]["calibrations"]
            .as_array_mut()
            .expect("calibration array")
        {
            let Some(identity) = calibration_identity(calibration) else {
                continue;
            };
            if let Some(previous) = old_calibrations
                .iter()
                .find(|old| calibration_identity(old) == Some(identity))
            {
                let mut merged = previous.clone();
                overlay_known(&mut merged, calibration.clone());
                *calibration = merged;
            }
        }
    }
    let mut value = template.cloned().unwrap_or(Value::Null);
    // Sparse-map deletions must not be restored by the additive settings overlay.
    if let Some(old) = value.get_mut("input_overrides") {
        let mut overrides = current["input_overrides"].clone();
        for collection in ["systems", "games"] {
            if let Some(scopes) = overrides[collection].as_object_mut() {
                for (key, patch) in scopes {
                    if let Some(previous) = old[collection].get(key) {
                        for field in ["bindings", "transforms"] {
                            if let Some(entries) = patch[field].as_object_mut() {
                                for (name, entry) in entries {
                                    if let Some(previous_entry) = previous[field].get(name) {
                                        let mut merged = previous_entry.clone();
                                        if field == "bindings"
                                            && entry["state"] == "unbound"
                                            && let Some(record) = merged.as_object_mut()
                                        {
                                            record.remove("value");
                                        }
                                        overlay_known(&mut merged, entry.clone());
                                        *entry = merged;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        *old = overrides.clone();
        current["input_overrides"] = overrides;
    }
    if let Some(input) = value
        .pointer_mut("/global/input")
        .and_then(Value::as_object_mut)
    {
        input.remove("binding_sets");
        input.remove("autofire");
    }
    overlay_known(&mut value, current);
    Ok(serde_json::to_string_pretty(&value)?)
}

pub(super) fn save(settings: &Settings) {
    let _ = try_save(settings);
}

fn try_save(settings: &Settings) -> Result<()> {
    let Ok(mut state) = settings.persistence.0.lock() else {
        bail!("Settings storage state is unavailable");
    };
    #[cfg(target_arch = "wasm32")]
    refresh_browser_status(&mut state);
    if state.read_only || state.storage_write_protected {
        bail!("Settings are protected because they require a newer version");
    }
    let result = encode(settings, state.template.as_ref()).and_then(|json| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            platform::save_settings_json(
                &json,
                state.previous_json.as_deref(),
                state.preserve_original,
            )?;
            state.template = Some(serde_json::from_str(&json)?);
            state.previous_json = Some(json);
            state.preserve_original = false;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let sequence = platform::save_settings_json(
                &json,
                state.previous_json.as_deref(),
                state.preserve_original,
            )?;
            state.browser_pending_sequence = Some(sequence);
            state.browser_generation = 0;
        }
        Ok(())
    });
    if let Err(error) = &result {
        log::error!("could not save settings: {error:#}");
        state.save_error = Some(format!(
            "Settings could not be saved: {error:#}. Changes may be lost. Retry after correcting the storage problem."
        ));
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            state.save_error = None;
        }
    }
    result
}

pub(super) fn export(settings: &Settings) -> Result<String> {
    #[cfg(not(target_arch = "wasm32"))]
    let state = settings
        .persistence
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("Settings storage state is unavailable"))?;
    #[cfg(target_arch = "wasm32")]
    let mut state = settings
        .persistence
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("Settings storage state is unavailable"))?;
    #[cfg(target_arch = "wasm32")]
    refresh_browser_status(&mut state);
    if state.read_only {
        return state
            .previous_json
            .clone()
            .context("The original read-only settings document is unavailable");
    }
    encode(settings, state.template.as_ref())
}

pub(super) fn import(settings: &mut Settings, json: &str) -> Result<()> {
    let imported = prepare_import(settings, json)?;
    try_save(&imported)?;
    *settings = imported;
    Ok(())
}

fn prepare_import(settings: &Settings, json: &str) -> Result<Settings> {
    if json.len() > 4 * 1024 * 1024 {
        bail!("The settings file exceeds the 4 MiB limit");
    }
    #[cfg(target_arch = "wasm32")]
    if platform::settings_storage_status().pending > 0 {
        bail!("Wait for pending settings saves before importing");
    }
    let mut imported = decode(json)?;
    let template = {
        let state = imported
            .persistence
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Settings storage state is unavailable"))?;
        if state.read_only {
            bail!("This file requires a newer version of Zeff Boy");
        }
        state.template.clone()
    };
    let mut metadata = {
        #[cfg(not(target_arch = "wasm32"))]
        let state = settings
            .persistence
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Settings storage state is unavailable"))?;
        #[cfg(target_arch = "wasm32")]
        let mut state = settings
            .persistence
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Settings storage state is unavailable"))?;
        #[cfg(target_arch = "wasm32")]
        refresh_browser_status(&mut state);
        if state.read_only || state.storage_write_protected {
            bail!("The current settings are protected because they require a newer version");
        }
        state.clone()
    };
    metadata.template = template;
    imported.persistence = PersistenceMetadata(Arc::new(Mutex::new(metadata)));
    Ok(imported)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn accept_browser_settings(settings: &mut Settings) -> Result<()> {
    let status = platform::settings_storage_status();
    let json = status
        .conflicting_json
        .context("No saved settings conflict is available")?;
    let candidate = decode(&json)?;
    let accepted = platform::accept_latest_settings_after_conflict()?
        .context("No saved settings conflict is available")?;
    if accepted != json {
        bail!("Saved settings changed during recovery; try again");
    }
    *settings = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
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
