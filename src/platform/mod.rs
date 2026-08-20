#[cfg(any(target_arch = "wasm32", test))]
mod firmware_store;
#[cfg(not(target_arch = "wasm32"))]
mod managed_firmware;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_storage;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use managed_firmware::{
    NativeFirmwareImport, import_firmware_file, managed_firmware_dir, remove_managed_firmware,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::*;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_storage::{
    firmware_inventory_snapshot, firmware_storage_key, import_firmware, init_storage,
    read_save_data, remove_firmware, save_data_exists, write_save_data,
};

pub(crate) use time::Instant;

mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use std::time::Instant;

    #[cfg(target_arch = "wasm32")]
    pub(crate) use web_time::Instant;
}
