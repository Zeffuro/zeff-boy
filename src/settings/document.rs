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
mod svp_tests;

#[cfg(test)]
mod tests;
