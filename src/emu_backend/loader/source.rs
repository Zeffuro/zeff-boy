use super::*;

pub(crate) struct LoadedBackend {
    pub(crate) backend: EmuBackend,
    pub(crate) original_crc32: u32,
}

pub(super) struct RomSource<'a> {
    pub(super) system: ActiveSystem,
    pub(super) source_path: &'a Path,
    pub(super) rom_path: &'a Path,
    pub(super) preloaded_data: Option<Vec<u8>>,
    pub(super) loaded_from_source_path: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) authenticated_gba_zip: bool,
}

pub(super) fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}
