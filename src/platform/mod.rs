#[cfg(not(target_arch = "wasm32"))]
mod atomic_file;
#[cfg(any(target_arch = "wasm32", test))]
mod firmware_store;
#[cfg(not(target_arch = "wasm32"))]
mod managed_firmware;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod stable_directory;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_arch = "wasm32", test))]
mod web_persistence;
#[cfg(target_arch = "wasm32")]
mod web_storage;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use atomic_file::{
    write_file_atomically, write_file_atomically_validated, write_new_file_atomically_validated,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use managed_firmware::{
    NativeFirmwareImport, import_firmware_file, managed_firmware_dir, remove_managed_firmware,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use stable_directory::{StableDirectory, metadata_is_redirect};
#[cfg(target_arch = "wasm32")]
pub(crate) use web::*;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_persistence::{DirtyEpoch, SaveWrite};
#[cfg(target_arch = "wasm32")]
pub(crate) use web_storage::{
    SaveBatchCompletion, capture_save_writes, commit_save_writes, firmware_inventory_snapshot,
    firmware_storage_key, import_firmware, init_storage, read_save_data, read_sram_data,
    remove_firmware, save_data_exists, save_writes_are_committed, write_save_data, write_sram_data,
};
#[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
pub(crate) use web_storage::{
    clear_browser_storage_for_test, fresh_browser_storage_entries_for_test,
};

pub(crate) use time::Instant;

mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use std::time::Instant;

    #[cfg(target_arch = "wasm32")]
    pub(crate) use web_time::Instant;
}
