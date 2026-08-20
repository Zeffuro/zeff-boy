use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use wasm_bindgen::prelude::*;

use super::firmware_store::{
    FirmwareRecord, decode_record, encode_record, firmware_key_matches_bytes, is_firmware_key,
    is_valid_firmware_key, storage_key,
};

thread_local! {
    static SAVE_CACHE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    static FIRMWARE_CACHE: RefCell<HashMap<String, FirmwareRecord>> = RefCell::new(HashMap::new());
}

#[wasm_bindgen(inline_js = "
const DB_NAME = 'zeff-boy-saves';
const DB_VERSION = 1;
const STORE_NAME = 'data';

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
        const tx = db.transaction(STORE_NAME, 'readonly');
        const store = tx.objectStore(STORE_NAME);
        const keysReq = store.getAllKeys();
        const valsReq = store.getAll();
        tx.oncomplete = () => {
            db.close();
            const entries = [];
            for (let i = 0; i < keysReq.result.length; i++) {
                entries.push([keysReq.result[i], new Uint8Array(valsReq.result[i])]);
            }
            resolve(entries);
        };
        tx.onerror = () => { db.close(); reject(tx.error); };
    });
}

export async function idb_put(key, value) {
    const db = await openDb();
    const copy = new Uint8Array(value);
    return new Promise((resolve, reject) => {
        const tx = db.transaction(STORE_NAME, 'readwrite');
        tx.objectStore(STORE_NAME).put(copy.buffer, key);
        tx.oncomplete = () => { db.close(); resolve(); };
        tx.onerror = () => { db.close(); reject(tx.error); };
    });
}

export async function idb_delete(key) {
    const db = await openDb();
    return new Promise((resolve, reject) => {
        const tx = db.transaction(STORE_NAME, 'readwrite');
        tx.objectStore(STORE_NAME).delete(key);
        tx.oncomplete = () => { db.close(); resolve(); };
        tx.onerror = () => { db.close(); reject(tx.error); };
    });
}
")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn idb_get_all_entries() -> Result<JsValue, JsValue>;

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
    migrate_local_storage();
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

fn migrate_local_storage() {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return;
    };
    let len = storage.length().unwrap_or(0);
    let mut migrated = 0u32;
    let mut keys_to_remove = Vec::new();
    SAVE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        for i in 0..len {
            let Some(key) = storage.key(i).ok().flatten() else {
                continue;
            };
            if !key.starts_with("zeff-state-") {
                continue;
            }
            if cache.contains_key(&key) {
                keys_to_remove.push(key);
                continue;
            }
            if let Some(hex) = storage.get_item(&key).ok().flatten()
                && let Ok(bytes) = const_hex::decode(&hex)
            {
                let k = key.clone();
                let b = bytes.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = idb_put(&k, &b).await;
                });
                cache.insert(key.clone(), bytes);
                keys_to_remove.push(key);
                migrated += 1;
            }
        }
    });
    for key in &keys_to_remove {
        let _ = storage.remove_item(key);
    }
    if migrated > 0 {
        log::info!("Migrated {migrated} save entries from localStorage to IndexedDB");
    }
}

pub(crate) fn write_save_data(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let key = format!("zeff-state-{}", path.display());
    let data = bytes.to_vec();
    SAVE_CACHE.with(|cache| {
        cache.borrow_mut().insert(key.clone(), data.clone());
    });
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = idb_put(&key, &data).await {
            log::error!("IndexedDB write failed: {e:?}");
        }
    });
    Ok(())
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
