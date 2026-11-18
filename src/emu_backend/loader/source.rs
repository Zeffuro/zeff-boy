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

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn zip_tas_source_media(
    source_path: &Path,
    rom_path: &Path,
    extension: &str,
    max_member_bytes: u64,
    raw_sha256: [u8; 32],
    sync: impl FnOnce(&str) -> [u8; 32],
) -> Option<([u8; 32], usize, [u8; 32])> {
    has_extension(source_path, "zip")
        .then(|| {
            crate::rom_archive::extract_bounded_zip_member(
                source_path,
                Some(rom_path),
                extension,
                128 * 1024 * 1024,
                max_member_bytes,
            )
        })?
        .ok()
        .filter(|selected| zeff_firmware::sha256_bytes(&selected.bytes) == raw_sha256)
        .map(|selected| {
            let sync = sync(&selected.member_name);
            (selected.archive_sha256, selected.archive_len, sync)
        })
}
