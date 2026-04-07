use crate::libretro_common::LibretroPlatform;

#[derive(Clone, Debug)]
pub(crate) struct RomMetadata {
    pub(crate) crc32: u32,
    pub(crate) title: String,
    pub(crate) rom_name: String,
    pub(crate) platform: LibretroPlatform,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MetadataRefreshStats {
    pub(crate) total_entries: usize,
    pub(crate) gb_entries: usize,
    pub(crate) gbc_entries: usize,
    pub(crate) nes_entries: usize,
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{build_cheat_search_hints, lookup_cached, refresh_cache_from_libretro};

#[cfg(test)]
use native::{build_index, deserialize_entries, parse_dat_entries, serialize_entries};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::*;

#[cfg(test)]
mod tests;
