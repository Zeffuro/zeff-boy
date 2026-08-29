use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use wasm_bindgen::prelude::*;

use super::firmware_store::{
    FirmwareRecord, decode_record, encode_record, firmware_key_matches_bytes, is_firmware_key,
    is_valid_firmware_key, storage_key,
};
use super::web_persistence::{
    SaveWrite, coalesce_write, migration_allowed, publish_committed, sram_key,
};

thread_local! {
    static SAVE_CACHE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    static FIRMWARE_CACHE: RefCell<HashMap<String, FirmwareRecord>> = RefCell::new(HashMap::new());
    static CAPTURED_WRITES: RefCell<Option<Vec<SaveWrite>>> = const { RefCell::new(None) };
    static LEGACY_SRAM_MIGRATIONS: RefCell<usize> = const { RefCell::new(0) };
    static PENDING_SAVE_KEYS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
}

const MAX_STORED_ENTRIES: usize = 4_096;
const MAX_STORED_VALUE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STORED_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_LEGACY_MIGRATIONS: usize = 64;

pub(crate) type SaveBatchCompletion = std::rc::Rc<std::cell::RefCell<Option<Result<(), String>>>>;

#[wasm_bindgen(inline_js = "
const DB_NAME = 'zeff-boy-saves';
const DB_VERSION = 1;
const STORE_NAME = 'data';
const MAX_ENTRIES = 4096;
const MAX_VALUE_BYTES = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES = 64 * 1024 * 1024;
let writeChain = Promise.resolve();

function openDb() {
    return new Promise((resolve, reject) => {
        const req = indexedDB.open(DB_NAME, DB_VERSION);
        req.onupgradeneeded = () => {
            const db = req.result;
            if (!db.objectStoreNames.contains(STORE_NAME)) {
                db.createObjectStore(STORE_NAME);
            }
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
    });
}

export async function idb_get_all_entries() {
    const db = await openDb();
    return new Promise((resolve, reject) => {
        let settled = false;
        const finish = (callback) => {
            if (settled) return;
            settled = true;
            db.close();
            callback();
        };
        try {
            const tx = db.transaction(STORE_NAME, 'readonly');
            const entries = [];
            let totalBytes = 0;
            const request = tx.objectStore(STORE_NAME).openCursor();
            request.onsuccess = () => {
                const cursor = request.result;
                if (!cursor || entries.length >= MAX_ENTRIES) return;
                if (typeof cursor.key !== 'string' || cursor.key.length > 512) {
                    cursor.continue();
                    return;
                }
                const value = new Uint8Array(cursor.value);
                if (value.byteLength > MAX_VALUE_BYTES || totalBytes + value.byteLength > MAX_TOTAL_BYTES) {
                    return;
                }
                totalBytes += value.byteLength;
                entries.push([cursor.key, value]);
                cursor.continue();
            };
            tx.oncomplete = () => finish(() => resolve(entries));
            tx.onerror = () => finish(() => reject(tx.error || new Error('IndexedDB read failed')));
            tx.onabort = () => finish(() => reject(tx.error || new Error('IndexedDB read aborted')));
        } catch (error) {
            finish(() => reject(error));
        }
    });
}

function applyBatch(keys, values, deleteKeys) {
    return openDb().then((db) => new Promise((resolve, reject) => {
        let settled = false;
        const finish = (callback) => {
            if (settled) return;
            settled = true;
            db.close();
            callback();
        };
        try {
            if (keys.length !== values.length) throw new Error('IndexedDB batch length mismatch');
            const tx = db.transaction(STORE_NAME, 'readwrite');
            const store = tx.objectStore(STORE_NAME);
            for (let i = 0; i < keys.length; i++) {
                const copy = new Uint8Array(values[i]);
                store.put(copy.buffer, keys[i]);
            }
            for (let i = 0; i < deleteKeys.length; i++) store.delete(deleteKeys[i]);
            tx.oncomplete = () => finish(resolve);
            tx.onerror = () => finish(() => reject(tx.error || new Error('IndexedDB write failed')));
            tx.onabort = () => finish(() => reject(tx.error || new Error('IndexedDB write aborted')));
        } catch (error) {
            finish(() => reject(error));
        }
    }));
}

export function idb_apply_batch(keys, values, deleteKeys) {
    const operation = writeChain.then(() => applyBatch(keys, values, deleteKeys));
    writeChain = operation.catch(() => undefined);
    return operation;
}

export function idb_put(key, value) {
    return idb_apply_batch([key], [value], []);
}

export function idb_delete(key) {
    return idb_apply_batch([], [], [key]);
}
")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn idb_get_all_entries() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn idb_apply_batch(
        keys: &js_sys::Array,
        values: &js_sys::Array,
        delete_keys: &js_sys::Array,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn idb_put(key: &str, value: &[u8]) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn idb_delete(key: &str) -> Result<JsValue, JsValue>;
}

pub(crate) async fn init_storage() {
    match idb_get_all_entries().await {
        Ok(entries) => {
            let arr = js_sys::Array::from(&entries);
            let count = arr.length();
            for i in 0..count {
                let pair = js_sys::Array::from(&arr.get(i));
                let key = pair.get(0).as_string().unwrap_or_default();
                let value = js_sys::Uint8Array::new(&pair.get(1)).to_vec();
                if is_firmware_key(&key) {
                    if !is_valid_firmware_key(&key) {
                        log::warn!("Ignoring invalid firmware storage key");
                        continue;
                    }
                    match decode_record(&value) {
                        Ok(record)
                            if is_known_good(&record)
                                && firmware_key_matches_bytes(&key, &record.bytes) =>
                        {
                            FIRMWARE_CACHE.with(|cache| {
                                cache.borrow_mut().insert(key, record);
                            });
                        }
                        Ok(_) => log::warn!(
                            "Ignoring unrecognized or incorrectly keyed stored firmware record"
                        ),
                        Err(error) => {
                            log::warn!("Ignoring invalid stored firmware record: {error}")
                        }
                    }
                } else {
                    if !is_valid_save_key(&key) || value.len() > MAX_STORED_VALUE_BYTES {
                        log::warn!("Ignoring invalid browser save entry");
                        continue;
                    }
                    SAVE_CACHE.with(|cache| {
                        cache.borrow_mut().insert(key, value);
                    });
                }
            }
            log::info!("Loaded {count} entries from IndexedDB");
        }
        Err(e) => {
            log::warn!("Failed to load from IndexedDB: {e:?}");
        }
    }
    migrate_local_storage().await;
}

fn entry_for_record(record: &FirmwareRecord) -> zeff_firmware::FirmwareInventoryEntry {
    zeff_firmware::FirmwareInventoryEntry::from_bytes(
        record.bytes.clone(),
        Some(record.original_filename.clone()),
        zeff_firmware::catalog_specs(),
    )
}

fn is_known_good(record: &FirmwareRecord) -> bool {
    matches!(
        entry_for_record(record).validation,
        zeff_firmware::ValidationStatus::KnownGood { .. }
    )
}

pub(crate) struct ImportedFirmware {
    pub(crate) spec_id: String,
    pub(crate) variant_id: String,
}

pub(crate) fn import_firmware(
    original_filename: String,
    bytes: Vec<u8>,
    completion: std::rc::Rc<std::cell::RefCell<Option<Result<(), String>>>>,
) -> anyhow::Result<ImportedFirmware> {
    let record = FirmwareRecord {
        original_filename,
        bytes,
    };
    let entry = entry_for_record(&record);
    let (spec_id, variant_id) = match &entry.validation {
        zeff_firmware::ValidationStatus::KnownGood {
            spec_id,
            variant_id,
        } => (spec_id.clone(), variant_id.clone()),
        zeff_firmware::ValidationStatus::UnknownHash { .. } => {
            anyhow::bail!(
                "Firmware filename and size are plausible, but its hash is not recognized"
            )
        }
        zeff_firmware::ValidationStatus::WrongSize { actual, .. } => {
            anyhow::bail!("Firmware has an unexpected size ({actual} bytes)")
        }
        zeff_firmware::ValidationStatus::NoMatchingSpec => {
            anyhow::bail!("File is not recognized as supported firmware")
        }
    };
    let key = storage_key(&record.bytes);
    let encoded = encode_record(&record)?;
    wasm_bindgen_futures::spawn_local(async move {
        let result = match idb_put(&key, &encoded).await {
            Ok(_) => {
                FIRMWARE_CACHE.with(|cache| {
                    cache.borrow_mut().insert(key, record);
                });
                Ok(())
            }
            Err(error) => Err(format!("IndexedDB firmware write failed: {error:?}")),
        };
        *completion.borrow_mut() = Some(result);
    });
    Ok(ImportedFirmware {
        spec_id,
        variant_id,
    })
}

pub(crate) fn firmware_inventory_snapshot() -> Arc<zeff_firmware::FirmwareInventory> {
    let mut inventory = zeff_firmware::FirmwareInventory::new();
    FIRMWARE_CACHE.with(|cache| {
        let cache = cache.borrow();
        let mut records = cache.iter().collect::<Vec<_>>();
        records.sort_by_key(|(key, _)| *key);
        for (_, record) in records {
            inventory.add(entry_for_record(record));
        }
    });
    Arc::new(inventory)
}

pub(crate) fn firmware_storage_key(bytes: &[u8]) -> String {
    storage_key(bytes)
}

pub(crate) fn remove_firmware(
    key: &str,
    completion: std::rc::Rc<std::cell::RefCell<Option<Result<(), String>>>>,
) -> anyhow::Result<()> {
    if !is_valid_firmware_key(key) {
        anyhow::bail!("invalid browser firmware storage key");
    }
    let exists = FIRMWARE_CACHE.with(|cache| cache.borrow().contains_key(key));
    if !exists {
        anyhow::bail!("browser firmware entry was not found");
    }
    let key = key.to_owned();
    wasm_bindgen_futures::spawn_local(async move {
        let result = match idb_delete(&key).await {
            Ok(_) => {
                FIRMWARE_CACHE.with(|cache| {
                    cache.borrow_mut().remove(&key);
                });
                Ok(())
            }
            Err(error) => Err(format!("IndexedDB firmware removal failed: {error:?}")),
        };
        *completion.borrow_mut() = Some(result);
    });
    Ok(())
}

async fn migrate_local_storage() {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return;
    };
    let len = storage
        .length()
        .unwrap_or(0)
        .min(MAX_LEGACY_MIGRATIONS as u32);
    let mut writes = Vec::new();
    let mut total_bytes = 0usize;
    for i in 0..len {
        let Some(key) = storage.key(i).ok().flatten() else {
            continue;
        };
        if !key.starts_with("zeff-state-") || key.len() > 512 || save_cache_contains_key(&key) {
            continue;
        }
        let Some(hex) = storage.get_item(&key).ok().flatten() else {
            continue;
        };
        if hex.len() > MAX_STORED_VALUE_BYTES.saturating_mul(2) {
            continue;
        }
        let Ok(bytes) = const_hex::decode(&hex) else {
            continue;
        };
        if total_bytes.saturating_add(bytes.len()) > MAX_STORED_TOTAL_BYTES {
            break;
        }
        total_bytes += bytes.len();
        writes.push(SaveWrite { key, data: bytes });
    }
    if writes.is_empty() {
        return;
    }
    let keys = writes
        .iter()
        .map(|write| write.key.clone())
        .collect::<Vec<_>>();
    match apply_save_batch(&writes, &[]).await {
        Ok(()) => {
            commit_cache_batch(&writes, &[]);
            for key in &keys {
                let _ = storage.remove_item(key);
            }
            log::info!(
                "Migrated {} save entries from localStorage to IndexedDB",
                writes.len()
            );
        }
        Err(error) => log::warn!("Failed to migrate browser save data: {error}"),
    }
}

pub(crate) fn write_save_data(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let key = format!("zeff-state-{}", path.display());
    capture_write(key, bytes)
}

pub(crate) fn save_data_exists(path: &Path) -> bool {
    let key = format!("zeff-state-{}", path.display());
    SAVE_CACHE.with(|cache| cache.borrow().contains_key(&key))
}

pub(crate) fn read_save_data(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let key = format!("zeff-state-{}", path.display());
    let result = SAVE_CACHE.with(|cache| cache.borrow().get(&key).cloned());
    Ok(result)
}

pub(crate) fn capture_save_writes<T>(
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<(T, Vec<SaveWrite>)> {
    CAPTURED_WRITES.with(|captured| -> anyhow::Result<()> {
        anyhow::ensure!(
            captured.borrow().is_none(),
            "browser save capture is already active"
        );
        *captured.borrow_mut() = Some(Vec::new());
        Ok(())
    })?;
    let result = operation();
    let writes = CAPTURED_WRITES.with(|captured| captured.borrow_mut().take().unwrap_or_default());
    result.map(|value| (value, writes))
}

pub(crate) fn commit_save_writes(writes: Vec<SaveWrite>, completion: SaveBatchCompletion) {
    PENDING_SAVE_KEYS.with(|pending| {
        let mut pending = pending.borrow_mut();
        for write in &writes {
            *pending.entry(write.key.clone()).or_default() += 1;
        }
    });
    wasm_bindgen_futures::spawn_local(async move {
        let result = apply_save_batch(&writes, &[]).await;
        if result.is_ok() {
            commit_cache_batch(&writes, &[]);
        }
        PENDING_SAVE_KEYS.with(|pending| {
            let mut pending = pending.borrow_mut();
            for write in &writes {
                if let Some(count) = pending.get_mut(&write.key) {
                    *count -= 1;
                    if *count == 0 {
                        pending.remove(&write.key);
                    }
                }
            }
        });
        *completion.borrow_mut() = Some(result);
    });
}

pub(crate) fn save_writes_are_committed(writes: &[SaveWrite]) -> bool {
    SAVE_CACHE.with(|cache| {
        let cache = cache.borrow();
        writes
            .iter()
            .all(|write| cache.get(&write.key) == Some(&write.data))
    })
}

pub(crate) fn write_sram_data(
    system: &str,
    media_identity: [u8; 32],
    component: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    capture_write(sram_key(system, media_identity, component), bytes)
}

pub(crate) fn read_sram_data(
    legacy_path: &Path,
    system: &str,
    media_identity: [u8; 32],
    component: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let key = sram_key(system, media_identity, component);
    if let Some(bytes) = SAVE_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Ok(Some(bytes));
    }

    let legacy_key = format!("zeff-state-{}", legacy_path.display());
    let Some(bytes) = SAVE_CACHE.with(|cache| cache.borrow().get(&legacy_key).cloned()) else {
        return Ok(None);
    };
    schedule_legacy_sram_migration(key, bytes.clone(), legacy_key);
    Ok(Some(bytes))
}

fn capture_write(key: String, bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        key.len() <= 512 && bytes.len() <= MAX_STORED_VALUE_BYTES,
        "browser save entry exceeds storage limits"
    );
    CAPTURED_WRITES.with(|captured| {
        let mut captured = captured.borrow_mut();
        let writes = captured
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("browser save requires an asynchronous commit"))?;
        coalesce_write(writes, key, bytes.to_vec());
        Ok(())
    })
}

fn schedule_legacy_sram_migration(key: String, data: Vec<u8>, legacy_key: String) {
    let migration_count = LEGACY_SRAM_MIGRATIONS.with(|count| *count.borrow());
    let can_migrate = SAVE_CACHE.with(|cache| {
        PENDING_SAVE_KEYS.with(|pending| {
            let pending_keys = pending.borrow().keys().cloned().collect::<HashSet<_>>();
            migration_allowed(
                &key,
                &cache.borrow(),
                &pending_keys,
                migration_count,
                MAX_LEGACY_MIGRATIONS,
            )
        })
    });
    if !can_migrate {
        return;
    }
    LEGACY_SRAM_MIGRATIONS.with(|count| *count.borrow_mut() += 1);
    PENDING_SAVE_KEYS.with(|pending| {
        *pending.borrow_mut().entry(key.clone()).or_default() += 1;
    });
    let marker_key = format!("zeff-sram-migrated-v2:{key}");
    let writes = vec![
        SaveWrite {
            key: key.clone(),
            data,
        },
        SaveWrite {
            key: marker_key,
            data: Vec::new(),
        },
    ];
    let deletes = vec![legacy_key];
    wasm_bindgen_futures::spawn_local(async move {
        match apply_save_batch(&writes, &deletes).await {
            Ok(()) => commit_cache_batch(&writes, &deletes),
            Err(error) => log::warn!("Failed to migrate legacy SRAM: {error}"),
        }
        PENDING_SAVE_KEYS.with(|pending| {
            let mut pending = pending.borrow_mut();
            if let Some(count) = pending.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    pending.remove(&key);
                }
            }
        });
    });
}

async fn apply_save_batch(writes: &[SaveWrite], deletes: &[String]) -> Result<(), String> {
    if writes.len().saturating_add(deletes.len()) > MAX_STORED_ENTRIES {
        return Err("browser save batch has too many entries".to_string());
    }
    let total_bytes = writes
        .iter()
        .try_fold(0usize, |total, write| total.checked_add(write.data.len()))
        .ok_or_else(|| "browser save batch size overflow".to_string())?;
    if total_bytes > MAX_STORED_TOTAL_BYTES
        || writes
            .iter()
            .any(|write| write.key.len() > 512 || write.data.len() > MAX_STORED_VALUE_BYTES)
    {
        return Err("browser save batch exceeds storage limits".to_string());
    }

    let keys = js_sys::Array::new();
    let values = js_sys::Array::new();
    for write in writes {
        keys.push(&JsValue::from_str(&write.key));
        values.push(&js_sys::Uint8Array::from(write.data.as_slice()));
    }
    let delete_keys = js_sys::Array::new();
    for key in deletes {
        delete_keys.push(&JsValue::from_str(key));
    }
    idb_apply_batch(&keys, &values, &delete_keys)
        .await
        .map(|_| ())
        .map_err(|error| format!("IndexedDB transaction failed: {error:?}"))
}

fn commit_cache_batch(writes: &[SaveWrite], deletes: &[String]) {
    SAVE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        publish_committed(&mut cache, writes);
        for key in deletes {
            cache.remove(key);
        }
    });
}

fn save_cache_contains_key(key: &str) -> bool {
    SAVE_CACHE.with(|cache| cache.borrow().contains_key(key))
}

fn is_valid_save_key(key: &str) -> bool {
    key.len() <= 512
        && (key.starts_with("zeff-state-")
            || key.starts_with("zeff-sram-v2:")
            || key.starts_with("zeff-sram-migrated-v2:")
            || is_firmware_key(key))
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
fn decode_indexed_db_entries(value: JsValue) -> Result<Vec<(String, Vec<u8>)>, String> {
    let array = js_sys::Array::from(&value);
    let mut entries = Vec::with_capacity(array.length() as usize);
    let mut total_bytes = 0usize;
    for index in 0..array.length() {
        let pair = js_sys::Array::from(&array.get(index));
        if pair.length() != 2 {
            return Err("IndexedDB test entry is not a key/value pair".to_string());
        }
        let key = pair
            .get(0)
            .as_string()
            .ok_or_else(|| "IndexedDB test entry key is not a string".to_string())?;
        let data = js_sys::Uint8Array::new(&pair.get(1)).to_vec();
        total_bytes = total_bytes
            .checked_add(data.len())
            .ok_or_else(|| "IndexedDB test entry size overflow".to_string())?;
        if key.len() > 512
            || data.len() > MAX_STORED_VALUE_BYTES
            || total_bytes > MAX_STORED_TOTAL_BYTES
        {
            return Err("IndexedDB test entry exceeds storage bounds".to_string());
        }
        entries.push((key, data));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
async fn raw_indexed_db_entries_for_test() -> Result<Vec<(String, Vec<u8>)>, String> {
    let value = idb_get_all_entries()
        .await
        .map_err(|error| format!("IndexedDB test read failed: {error:?}"))?;
    decode_indexed_db_entries(value)
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
fn clear_browser_storage_caches_for_test() {
    SAVE_CACHE.with(|cache| cache.borrow_mut().clear());
    FIRMWARE_CACHE.with(|cache| cache.borrow_mut().clear());
    CAPTURED_WRITES.with(|captured| {
        assert!(captured.borrow().is_none(), "save capture remains active");
    });
    LEGACY_SRAM_MIGRATIONS.with(|count| *count.borrow_mut() = 0);
    PENDING_SAVE_KEYS.with(|pending| {
        assert!(pending.borrow().is_empty(), "save writes remain pending");
    });
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
pub(crate) async fn clear_browser_storage_for_test() -> Result<(), String> {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        storage
            .clear()
            .map_err(|error| format!("localStorage test clear failed: {error:?}"))?;
    }
    let entries = raw_indexed_db_entries_for_test().await?;
    let deletes = entries.into_iter().map(|(key, _)| key).collect::<Vec<_>>();
    apply_save_batch(&[], &deletes).await?;
    clear_browser_storage_caches_for_test();
    if raw_indexed_db_entries_for_test().await?.is_empty() {
        Ok(())
    } else {
        Err("IndexedDB test clear left stored entries".to_string())
    }
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
pub(crate) async fn fresh_browser_storage_entries_for_test()
-> Result<Vec<(String, Vec<u8>)>, String> {
    clear_browser_storage_caches_for_test();
    init_storage().await;
    let mut cached = SAVE_CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .map(|(key, data)| (key.clone(), data.clone()))
            .collect::<Vec<_>>()
    });
    cached.sort_by(|left, right| left.0.cmp(&right.0));
    let raw = raw_indexed_db_entries_for_test().await?;
    if cached != raw {
        return Err("production IndexedDB reload differs from stored entries".to_string());
    }
    Ok(cached)
}
