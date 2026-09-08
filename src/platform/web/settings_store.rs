use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::platform::web_settings_state::{SettingsStorageStatus, SettingsStorageTracker};

const LEGACY_PRIMARY_KEY: &str = "zeff-boy-settings";
const LEGACY_BACKUP_KEY: &str = "zeff-boy-settings-backup";
const MAX_SETTINGS_BYTES: usize = 4 * 1024 * 1024;

thread_local! {
    static PRIMARY: RefCell<Option<Result<Option<String>, String>>> = const { RefCell::new(None) };
    static BACKUP: RefCell<Option<Result<Option<String>, String>>> = const { RefCell::new(None) };
    static TRACKER: RefCell<SettingsStorageTracker> = RefCell::new(SettingsStorageTracker::default());
    static INITIALIZED: RefCell<bool> = const { RefCell::new(false) };
    static WRITE_PROTECTED: RefCell<bool> = const { RefCell::new(false) };
}

#[wasm_bindgen(inline_js = r#"
const SETTINGS_DB_NAME = 'zeff-boy-settings';
const SETTINGS_DB_VERSION = 1;
const SETTINGS_STORE_NAME = 'documents';
const SETTINGS_FORMAT_VERSION = 1;
const SETTINGS_PRIMARY_KEY = 'settings';
const SETTINGS_BACKUP_KEY = 'settings-backup';
const SETTINGS_MIGRATION_KEY = 'legacy-local-storage-import-v1';
const SETTINGS_MAX_BYTES = 4 * 1024 * 1024;

let settingsWriteChain = Promise.resolve();
let settingsTabRecord = null;
let settingsTabWriteProtected = false;
let settingsLastConflict = null;
let settingsInitialized = false;
let settingsOriginalPreserved = false;

function openSettingsDb() {
    return new Promise((resolve, reject) => {
        let settled = false;
        const request = indexedDB.open(SETTINGS_DB_NAME, SETTINGS_DB_VERSION);
        request.onupgradeneeded = () => {
            const db = request.result;
            if (!db.objectStoreNames.contains(SETTINGS_STORE_NAME)) {
                db.createObjectStore(SETTINGS_STORE_NAME);
            }
        };
        request.onsuccess = () => {
            const db = request.result;
            if (settled) {
                db.close();
                return;
            }
            settled = true;
            db.onversionchange = () => db.close();
            resolve(db);
        };
        request.onerror = () => {
            if (settled) return;
            settled = true;
            reject(request.error || new Error('IndexedDB open failed'));
        };
        request.onblocked = () => {
            if (settled) return;
            settled = true;
            reject(new Error('IndexedDB open was blocked by another tab'));
        };
    });
}

function classifySettingsRecord(value) {
    if (value === undefined) return { kind: 'missing' };
    if (!value || typeof value !== 'object') return { kind: 'invalid' };
    if (!Number.isSafeInteger(value.storageVersion) || value.storageVersion < 1) {
        return { kind: 'invalid' };
    }
    if (value.storageVersion > SETTINGS_FORMAT_VERSION) {
        return { kind: 'future', value };
    }
    if (!Number.isSafeInteger(value.revision) || value.revision < 1
        || typeof value.json !== 'string'
        || new TextEncoder().encode(value.json).byteLength > SETTINGS_MAX_BYTES) {
        return { kind: 'invalid' };
    }
    return { kind: 'valid', value };
}

function newSettingsRecord(revision, json) {
    return { storageVersion: SETTINGS_FORMAT_VERSION, revision, json };
}

function settingsRecoveryKey() {
    const suffix = globalThis.crypto && typeof globalThis.crypto.randomUUID === 'function'
        ? globalThis.crypto.randomUUID()
        : `${Date.now()}-${Math.random()}`;
    return `recovered:${suffix}`;
}

function classifyMigrationMarker(marker) {
    if (marker === undefined) return { kind: 'missing' };
    if (!marker || typeof marker !== 'object'
        || !Number.isSafeInteger(marker.storageVersion)
        || marker.storageVersion < 1) return { kind: 'invalid' };
    if (marker.storageVersion > SETTINGS_FORMAT_VERSION) return { kind: 'future' };
    return marker.state === 'complete' ? { kind: 'valid' } : { kind: 'invalid' };
}

export async function zeffBoySettingsStoreInit(legacyPrimary, legacyBackup, legacyAvailable) {
    const db = await openSettingsDb();
    return new Promise((resolve, reject) => {
        let settled = false;
        let outcome = null;
        const finish = (callback) => {
            if (settled) return;
            settled = true;
            db.close();
            callback();
        };
        try {
            const tx = db.transaction(SETTINGS_STORE_NAME, 'readwrite');
            const store = tx.objectStore(SETTINGS_STORE_NAME);
            const values = {};
            let remaining = 3;
            const receive = (name, request) => {
                request.onsuccess = () => {
                    values[name] = request.result;
                    remaining--;
                    if (remaining === 0) prepare();
                };
            };
            const prepare = () => {
                const primary = classifySettingsRecord(values.primary);
                const backup = classifySettingsRecord(values.backup);
                const marker = classifyMigrationMarker(values.marker);
                let writeProtected = marker.kind === 'future';
                let primaryError = null;
                let backupError = null;

                if (primary.kind === 'future' || backup.kind === 'future') writeProtected = true;
                if (primary.kind === 'invalid') primaryError = 'The IndexedDB settings record is invalid';
                if (backup.kind === 'invalid') backupError = 'The IndexedDB settings backup is invalid';
                if (backup.kind === 'future') backupError = 'The IndexedDB settings backup uses a newer storage format';

                let activePrimary = primary.kind === 'valid' ? primary.value : null;
                let activeBackup = backup.kind === 'valid' ? backup.value : null;
                const migrationNeeded = marker.kind === 'missing' || marker.kind === 'invalid';
                const canFinishMigration = activePrimary || legacyAvailable;
                if (!writeProtected && migrationNeeded && canFinishMigration) {
                    if (marker.kind === 'invalid') {
                        store.put(values.marker, settingsRecoveryKey());
                    }
                    if (!activePrimary && typeof legacyPrimary === 'string') {
                        if (new TextEncoder().encode(legacyPrimary).byteLength <= SETTINGS_MAX_BYTES) {
                            if (primary.kind === 'invalid') {
                                store.put(values.primary, settingsRecoveryKey());
                            }
                            activePrimary = newSettingsRecord(1, legacyPrimary);
                            store.put(activePrimary, SETTINGS_PRIMARY_KEY);
                            primaryError = null;
                        } else {
                            primaryError = 'Legacy browser settings exceed the storage size limit';
                        }
                    }
                    if (!activeBackup && typeof legacyBackup === 'string') {
                        if (new TextEncoder().encode(legacyBackup).byteLength <= SETTINGS_MAX_BYTES) {
                            if (backup.kind === 'invalid') {
                                store.put(values.backup, settingsRecoveryKey());
                            }
                            activeBackup = newSettingsRecord(1, legacyBackup);
                            store.put(activeBackup, SETTINGS_BACKUP_KEY);
                            backupError = null;
                        } else {
                            backupError = 'Legacy browser settings backup exceeds the storage size limit';
                        }
                    }
                    if (!primaryError && !backupError) {
                        store.put(
                            { storageVersion: SETTINGS_FORMAT_VERSION, state: 'complete' },
                            SETTINGS_MIGRATION_KEY,
                        );
                    }
                }

                outcome = {
                    primary: activePrimary ? activePrimary.json
                        : primary.kind === 'future' && typeof primary.value.json === 'string'
                            && new TextEncoder().encode(primary.value.json).byteLength <= SETTINGS_MAX_BYTES
                            ? primary.value.json : null,
                    backup: activeBackup ? activeBackup.json : null,
                    primaryError,
                    backupError,
                    writeProtected,
                    revision: activePrimary ? activePrimary.revision : 0,
                    malformedPrimary: primary.kind === 'invalid',
                };
            };

            receive('primary', store.get(SETTINGS_PRIMARY_KEY));
            receive('backup', store.get(SETTINGS_BACKUP_KEY));
            receive('marker', store.get(SETTINGS_MIGRATION_KEY));
            tx.oncomplete = () => finish(() => {
                settingsTabRecord = outcome && outcome.revision > 0
                    ? newSettingsRecord(outcome.revision, outcome.primary)
                    : null;
                settingsTabWriteProtected = !!(outcome && outcome.writeProtected);
                settingsLastConflict = null;
                settingsInitialized = true;
                settingsOriginalPreserved = false;
                resolve(outcome);
            });
            tx.onerror = () => finish(() => reject(tx.error || new Error('IndexedDB settings initialization failed')));
            tx.onabort = () => finish(() => reject(tx.error || new Error('IndexedDB settings initialization was aborted')));
        } catch (error) {
            finish(() => reject(error));
        }
    });
}

function commitSettings(json, preserveOriginal) {
    if (!settingsInitialized) {
        return Promise.resolve({ kind: 'error', message: 'Browser settings storage is not initialized' });
    }
    if (settingsTabWriteProtected) {
        return Promise.resolve({ kind: 'future', message: 'Browser settings use a newer storage format' });
    }
    const shouldPreserveOriginal = preserveOriginal && !settingsOriginalPreserved;
    return openSettingsDb().then((db) => new Promise((resolve, reject) => {
        let settled = false;
        let outcome = null;
        const finish = (callback) => {
            if (settled) return;
            settled = true;
            db.close();
            callback();
        };
        try {
            const tx = db.transaction(SETTINGS_STORE_NAME, 'readwrite');
            const store = tx.objectStore(SETTINGS_STORE_NAME);
            const values = {};
            let remaining = 3;
            const receive = (name, request) => {
                request.onsuccess = () => {
                    values[name] = request.result;
                    remaining--;
                    if (remaining === 0) prepare();
                };
            };
            const prepare = () => {
                const raw = values.primary;
                const current = classifySettingsRecord(raw);
                const backup = classifySettingsRecord(values.backup);
                const marker = classifyMigrationMarker(values.marker);
                if (current.kind === 'future' || backup.kind === 'future' || marker.kind === 'future') {
                    outcome = { kind: 'future', message: 'Browser settings use a newer storage format' };
                    return;
                }
                if (current.kind === 'invalid' && !shouldPreserveOriginal) {
                    outcome = { kind: 'error', message: 'The IndexedDB settings record is invalid and was left unchanged' };
                    return;
                }

                const expected = settingsTabRecord;
                const actual = current.kind === 'valid' ? current.value : null;
                const sameRevision = (!expected && !actual)
                    || (expected && actual && expected.revision === actual.revision
                        && expected.json === actual.json);
                if (!sameRevision && current.kind !== 'invalid') {
                    outcome = { kind: 'conflict', current: actual };
                    return;
                }

                if (actual && actual.json === json) {
                    outcome = { kind: 'persisted', current: actual, preservedOriginal: false };
                    return;
                }

                const revision = actual ? actual.revision + 1 : 1;
                if (!Number.isSafeInteger(revision)) {
                    outcome = { kind: 'error', message: 'The browser settings revision limit was reached' };
                    return;
                }
                if (shouldPreserveOriginal && raw !== undefined) {
                    store.put(raw, settingsRecoveryKey());
                }
                if (actual) {
                    if (backup.kind === 'invalid') store.put(values.backup, settingsRecoveryKey());
                    store.put(actual, SETTINGS_BACKUP_KEY);
                }
                const next = newSettingsRecord(revision, json);
                store.put(next, SETTINGS_PRIMARY_KEY);
                outcome = { kind: 'persisted', current: next, preservedOriginal: shouldPreserveOriginal };
            };
            receive('primary', store.get(SETTINGS_PRIMARY_KEY));
            receive('backup', store.get(SETTINGS_BACKUP_KEY));
            receive('marker', store.get(SETTINGS_MIGRATION_KEY));
            tx.oncomplete = () => finish(() => resolve(outcome || {
                kind: 'error', message: 'IndexedDB settings transaction completed without a result',
            }));
            tx.onerror = () => finish(() => reject(tx.error || new Error('IndexedDB settings write failed')));
            tx.onabort = () => finish(() => reject(tx.error || new Error('IndexedDB settings write was aborted')));
        } catch (error) {
            finish(() => reject(error));
        }
    }));
}

export function zeffBoySettingsStoreSave(json, preserveOriginal) {
    const operation = settingsWriteChain.then(() => commitSettings(json, preserveOriginal));
    settingsWriteChain = operation.then((outcome) => {
        if (outcome.kind === 'persisted') {
            settingsTabRecord = outcome.current;
            if (outcome.preservedOriginal) settingsOriginalPreserved = true;
            settingsLastConflict = null;
        } else if (outcome.kind === 'conflict' && outcome.current) {
            settingsLastConflict = outcome.current;
        } else if (outcome.kind === 'future') {
            settingsTabWriteProtected = true;
        }
    }, () => undefined);
    return operation;
}

export function zeffBoySettingsStoreAcceptConflict(revision, json) {
    if (!settingsLastConflict || settingsLastConflict.revision !== revision
        || settingsLastConflict.json !== json) return false;
    settingsTabRecord = settingsLastConflict;
    settingsLastConflict = null;
    return true;
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = zeffBoySettingsStoreInit)]
    async fn js_init(
        legacy_primary: &JsValue,
        legacy_backup: &JsValue,
        legacy_available: bool,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = zeffBoySettingsStoreSave)]
    async fn js_save(json: &str, preserve_original: bool) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = zeffBoySettingsStoreAcceptConflict)]
    fn js_accept_conflict(revision: f64, json: &str) -> bool;
}

pub(crate) async fn init() {
    let (legacy_primary, legacy_backup, legacy_available, legacy_error) = legacy_settings();
    let result = js_init(
        &legacy_primary
            .as_deref()
            .map_or(JsValue::NULL, JsValue::from_str),
        &legacy_backup
            .as_deref()
            .map_or(JsValue::NULL, JsValue::from_str),
        legacy_available,
    )
    .await;

    match result {
        Ok(value) => {
            let primary = read_optional_string(&value, "primary");
            let backup = read_optional_string(&value, "backup");
            let primary_error = read_optional_string(&value, "primaryError").or_else(|| {
                if primary.is_none() {
                    legacy_error.clone()
                } else {
                    None
                }
            });
            let backup_error = read_optional_string(&value, "backupError");
            let write_protected = read_bool(&value, "writeProtected").unwrap_or(false);
            PRIMARY.with(|cached| {
                *cached.borrow_mut() = Some(primary_error.map_or(Ok(primary.clone()), Err));
            });
            BACKUP.with(|cached| {
                *cached.borrow_mut() = Some(backup_error.map_or(Ok(backup), Err));
            });
            TRACKER.with(|tracker| {
                tracker.borrow_mut().initialize(primary, write_protected);
            });
            WRITE_PROTECTED.with(|protected| *protected.borrow_mut() = write_protected);
            INITIALIZED.with(|initialized| *initialized.borrow_mut() = true);
        }
        Err(error) => {
            let message = format!(
                "Browser settings storage could not be initialized: {}",
                js_error_message(&error)
            );
            PRIMARY.with(|cached| {
                *cached.borrow_mut() = Some(match legacy_primary.clone() {
                    Some(json) => Ok(Some(json)),
                    None => Err(message.clone()),
                });
            });
            BACKUP.with(|cached| {
                *cached.borrow_mut() = Some(Ok(legacy_backup));
            });
            TRACKER.with(|tracker| {
                let mut tracker = tracker.borrow_mut();
                tracker.initialize(legacy_primary, false);
                tracker.initialization_failed(message);
            });
            INITIALIZED.with(|initialized| *initialized.borrow_mut() = false);
        }
    }
}

pub(crate) fn load_primary() -> anyhow::Result<Option<String>> {
    cached_read(
        &PRIMARY,
        "Browser settings storage was read before initialization",
    )
}

pub(crate) fn load_backup() -> anyhow::Result<Option<String>> {
    cached_read(
        &BACKUP,
        "Browser settings backup was read before initialization",
    )
}

pub(crate) fn save(json: &str, preserve_original: bool) -> anyhow::Result<u64> {
    anyhow::ensure!(
        INITIALIZED.with(|initialized| *initialized.borrow()),
        "browser settings storage is unavailable"
    );
    anyhow::ensure!(
        !WRITE_PROTECTED.with(|protected| *protected.borrow()),
        "browser settings use a newer storage format and cannot be changed"
    );
    anyhow::ensure!(
        json.len() <= MAX_SETTINGS_BYTES,
        "browser settings exceed the 4 MiB storage limit"
    );
    let sequence = TRACKER.with(|tracker| {
        tracker
            .borrow_mut()
            .schedule()
            .ok_or_else(|| anyhow::anyhow!("browser settings save sequence is exhausted"))
    })?;
    let json = json.to_owned();
    wasm_bindgen_futures::spawn_local(async move {
        let completion = match js_save(&json, preserve_original).await {
            Ok(value) => parse_save_outcome(&value),
            Err(error) => Err((
                format!(
                    "IndexedDB could not save settings (storage may be full or blocked): {}",
                    js_error_message(&error)
                ),
                None,
            )),
        };
        TRACKER.with(|tracker| match completion {
            Ok(()) => tracker.borrow_mut().persisted(sequence, json),
            Err((message, conflict)) => {
                tracker.borrow_mut().failed(sequence, message, conflict);
            }
        });
    });
    Ok(sequence)
}

pub(crate) fn status() -> SettingsStorageStatus {
    TRACKER.with(|tracker| tracker.borrow().snapshot())
}

pub(crate) fn generation() -> u64 {
    TRACKER.with(|tracker| tracker.borrow().generation())
}

pub(crate) fn accept_latest_after_conflict() -> anyhow::Result<Option<String>> {
    let snapshot = status();
    anyhow::ensure!(
        snapshot.pending == 0,
        "browser settings writes are still pending"
    );
    let (Some(revision), Some(json)) = (snapshot.conflict_revision, snapshot.conflicting_json)
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        js_accept_conflict(revision as f64, &json),
        "the saved settings changed again; load them again before replacing local edits"
    );
    TRACKER.with(|tracker| {
        tracker.borrow_mut().accept_conflict();
    });
    PRIMARY.with(|cached| *cached.borrow_mut() = Some(Ok(Some(json.clone()))));
    Ok(Some(json))
}

fn cached_read(
    cache: &'static std::thread::LocalKey<RefCell<Option<Result<Option<String>, String>>>>,
    uninitialized: &str,
) -> anyhow::Result<Option<String>> {
    cache.with(|cached| match cached.borrow().as_ref() {
        Some(Ok(value)) => Ok(value.clone()),
        Some(Err(error)) => Err(anyhow::anyhow!(error.clone())),
        None => Err(anyhow::anyhow!("{uninitialized}")),
    })
}

fn legacy_settings() -> (Option<String>, Option<String>, bool, Option<String>) {
    let Some(window) = web_sys::window() else {
        return (
            None,
            None,
            false,
            Some("Browser window is unavailable".to_string()),
        );
    };
    let storage = match window.local_storage() {
        Ok(Some(storage)) => storage,
        Ok(None) => {
            return (
                None,
                None,
                false,
                Some("Legacy browser settings storage is disabled".to_string()),
            );
        }
        Err(error) => {
            return (
                None,
                None,
                false,
                Some(format!(
                    "Legacy browser settings could not be read: {}",
                    js_error_message(&error)
                )),
            );
        }
    };
    let primary = match storage.get_item(LEGACY_PRIMARY_KEY) {
        Ok(value) => value,
        Err(error) => {
            return (
                None,
                None,
                false,
                Some(format!(
                    "Legacy browser settings could not be read: {}",
                    js_error_message(&error)
                )),
            );
        }
    };
    let backup = match storage.get_item(LEGACY_BACKUP_KEY) {
        Ok(value) => value,
        Err(error) => {
            return (
                primary,
                None,
                false,
                Some(format!(
                    "Legacy browser settings backup could not be read: {}",
                    js_error_message(&error)
                )),
            );
        }
    };
    (primary, backup, true, None)
}

fn parse_save_outcome(value: &JsValue) -> Result<(), (String, Option<(u64, String)>)> {
    match read_string(value, "kind").as_deref() {
        Some("persisted") => Ok(()),
        Some("conflict") => {
            let current = js_sys::Reflect::get(value, &JsValue::from_str("current"))
                .unwrap_or(JsValue::UNDEFINED);
            let revision = read_number(&current, "revision")
                .filter(|revision| revision.is_finite() && *revision >= 1.0)
                .map(|revision| revision as u64);
            let json = read_string(&current, "json");
            let conflict = revision.zip(json);
            Err((
                "Settings changed in another tab. Export this tab's unsaved settings or load the saved settings before editing again."
                    .to_string(),
                conflict,
            ))
        }
        Some("future") => {
            WRITE_PROTECTED.with(|protected| *protected.borrow_mut() = true);
            TRACKER.with(|tracker| tracker.borrow_mut().mark_write_protected());
            Err((
                "Browser settings were saved by a newer storage format. Update Zeff Boy to edit them."
                    .to_string(),
                None,
            ))
        }
        _ => Err((
            read_string(value, "message")
                .unwrap_or_else(|| "IndexedDB settings transaction failed".to_string()),
            None,
        )),
    }
}

fn read_optional_string(value: &JsValue, property: &str) -> Option<String> {
    read_string(value, property)
}

fn read_string(value: &JsValue, property: &str) -> Option<String> {
    js_sys::Reflect::get(value, &JsValue::from_str(property))
        .ok()?
        .as_string()
}

fn read_number(value: &JsValue, property: &str) -> Option<f64> {
    js_sys::Reflect::get(value, &JsValue::from_str(property))
        .ok()?
        .as_f64()
}

fn read_bool(value: &JsValue, property: &str) -> Option<bool> {
    js_sys::Reflect::get(value, &JsValue::from_str(property))
        .ok()?
        .as_bool()
}

fn js_error_message(error: &JsValue) -> String {
    if let Some(message) = error.as_string() {
        return message;
    }
    let name = read_string(error, "name").unwrap_or_else(|| "browser error".to_string());
    let message = read_string(error, "message").unwrap_or_default();
    if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    }
}
