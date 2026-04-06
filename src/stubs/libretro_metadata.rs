use crate::libretro_common::LibretroPlatform;

#[derive(Clone, Debug)]
pub(crate) struct RomMetadata {
    pub(crate) crc32: u32,
    pub(crate) title: String,
    pub(crate) rom_name: String,
    pub(crate) platform: LibretroPlatform,
}

#[derive(Default)]
pub(crate) struct MetadataRefreshStats {
    pub(crate) total_entries: usize,
    pub(crate) gb_entries: usize,
    pub(crate) gbc_entries: usize,
    pub(crate) nes_entries: usize,
}

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

