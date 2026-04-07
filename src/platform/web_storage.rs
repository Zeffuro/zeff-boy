use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use wasm_bindgen::prelude::*;

thread_local! {
    static SAVE_CACHE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
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
")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn idb_get_all_entries() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn idb_put(key: &str, value: &[u8]) -> Result<JsValue, JsValue>;
}

pub(crate) async fn init_storage() {
    match idb_get_all_entries().await {
        Ok(entries) => {
            let arr = js_sys::Array::from(&entries);
            let count = arr.length();
            SAVE_CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                for i in 0..count {
                    let pair = js_sys::Array::from(&arr.get(i));
                    let key = pair.get(0).as_string().unwrap_or_default();
                    let value = js_sys::Uint8Array::new(&pair.get(1));
                    cache.insert(key, value.to_vec());
                }
            });
            log::info!("Loaded {count} entries from IndexedDB");
        }
        Err(e) => {
            log::warn!("Failed to load from IndexedDB: {e:?}");
        }
    }
    migrate_local_storage();
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
            if let Some(hex) = storage.get_item(&key).ok().flatten() {
                if let Ok(bytes) = const_hex::decode(&hex) {
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
