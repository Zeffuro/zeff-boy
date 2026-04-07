use super::RomMetadata;
use crate::libretro_common::LibretroPlatform;

use super::MetadataRefreshStats;

pub(crate) fn refresh_cache_from_libretro() -> anyhow::Result<MetadataRefreshStats> {
    anyhow::bail!("metadata refresh not available on web")
}

pub(crate) fn lookup_cached(_crc32: u32, _platform: LibretroPlatform) -> Option<RomMetadata> {
    None
}

pub(crate) fn build_cheat_search_hints(
    _rom_title: &str,
    _meta: Option<&RomMetadata>,
) -> Vec<String> {
    Vec::new()
}
