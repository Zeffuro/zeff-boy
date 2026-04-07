#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_storage;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::*;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_storage::{init_storage, read_save_data, save_data_exists, write_save_data};

pub(crate) use time::Instant;

mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use std::time::Instant;

    #[cfg(target_arch = "wasm32")]
    pub(crate) use web_time::Instant;
}
