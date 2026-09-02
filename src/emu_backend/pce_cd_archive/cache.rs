use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct CachedArchive {
    version: u32,
    source_size: u64,
    source_modified_secs: u64,
    source_modified_nanos: u32,
    members: Vec<CachedMember>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedMember {
    name: String,
    size: u64,
    sha256: [u8; 32],
}

pub(super) struct CacheEntry {
    pub(super) path: PathBuf,
    manifest: CachedArchive,
    extracted_bytes: u64,
}

#[derive(Clone, Copy)]
pub(super) struct SourceFingerprint {
    size: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Clone, Copy)]
pub(super) struct CacheIdentity<'a> {
    pub(super) source: SourceFingerprint,
    pub(super) key: &'a str,
}

struct StagingCache {
    cache_root: PathBuf,
    path: PathBuf,
    published: bool,
}

impl Drop for StagingCache {
    fn drop(&mut self) {
        if !self.published {
            remove_cache_entry(&self.cache_root, &self.path);
        }
    }
}

pub(super) enum CachedDiscError {
    Corrupt,
    Load(PceCdLoadError),
}

#[cfg(test)]
pub(super) fn pce_cd_cache_root() -> PathBuf {
    std::env::temp_dir().join(format!("zeff-pce-cd-cache-tests-{}", std::process::id()))
}

#[cfg(not(test))]
pub(super) fn pce_cd_cache_root() -> PathBuf {
    crate::platform::cache_dir().join("pce-cd-7z-v1")
}

pub(super) fn validate_cacheable_manifest(
    manifest: &ArchiveManifest,
) -> Result<(), PceCdLoadError> {
    if manifest.entries.len() > CACHE_ENTRY_MEMBER_LIMIT {
        return Err(PceCdLoadError::TooManyArchiveEntries(
            manifest.entries.len(),
        ));
    }
    for member in &manifest.entries {
        if member.is_link {
            return Err(PceCdLoadError::ArchiveLinkUnsupported(member.name.clone()));
        }
        if member.has_stream {
            validate_regular_member(member)?;
        }
    }
    Ok(())
}

pub(super) fn source_fingerprint(path: &Path) -> Result<SourceFingerprint, PceCdLoadError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let modified = metadata
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(SourceFingerprint {
        size: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

pub(super) fn cache_key(path: &Path, source: SourceFingerprint) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-boy:pce-cd-7z-cache:v1\0");
    update_path_hash(&mut hasher, &canonical);
    hasher.update(source.size.to_le_bytes());
    hasher.update(source.modified_secs.to_le_bytes());
    hasher.update(source.modified_nanos.to_le_bytes());
    const_hex::encode(hasher.finalize())
}

#[cfg(windows)]
fn update_path_hash(hasher: &mut Sha256, path: &Path) {
    use std::os::windows::ffi::OsStrExt;

    for unit in path.as_os_str().encode_wide() {
        hasher.update(unit.to_le_bytes());
    }
}

#[cfg(not(windows))]
fn update_path_hash(hasher: &mut Sha256, path: &Path) {
    use std::os::unix::ffi::OsStrExt;

    hasher.update(path.as_os_str().as_bytes());
}

pub(super) fn prepare_cache(
    reader: &mut ArchiveReader<File>,
    archive: &ArchiveManifest,
    identity: CacheIdentity<'_>,
    cache_root: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<CacheEntry, PceCdLoadError> {
    std::fs::create_dir_all(cache_root).map_err(cache_io_error)?;
    cleanup_stale_staging(cache_root);
    let path = cache_root.join(identity.key);
    if let Some(manifest) = read_cache_manifest(&path, archive, identity.source) {
        return Ok(CacheEntry {
            path,
            manifest,
            extracted_bytes: 0,
        });
    }
    remove_cache_entry(cache_root, &path);
    extract_cache(reader, archive, identity, cache_root, cancel, progress)
}

pub(super) fn extract_cache(
    reader: &mut ArchiveReader<File>,
    archive: &ArchiveManifest,
    identity: CacheIdentity<'_>,
    cache_root: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<CacheEntry, PceCdLoadError> {
    static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

    check_cancelled(cancel)?;
    let decoded_total = archive
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    if decoded_total > CACHE_ENTRY_BYTES_LIMIT || archive.entries.len() > CACHE_ENTRY_MEMBER_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    progress.set_total_bytes(decoded_total);
    progress.set_completed_bytes(0);

    let nonce = STAGING_NONCE.fetch_add(1, Ordering::Relaxed);
    let staging_path = cache_root.join(format!(
        ".{}.tmp-{}-{nonce}",
        identity.key,
        std::process::id()
    ));
    std::fs::create_dir(&staging_path).map_err(cache_io_error)?;
    let mut staging = StagingCache {
        cache_root: cache_root.to_path_buf(),
        path: staging_path,
        published: false,
    };
    let files_root = staging.path.join(CACHE_FILES_DIR);
    std::fs::create_dir(&files_root).map_err(cache_io_error)?;

    let mut cached = Vec::with_capacity(archive.entries.len());
    let mut decoded = 0_u64;
    let mut failure = None;
    let result = reader.for_each_entries(|entry, input| {
        if failure.is_some() {
            return Err(cancel_error());
        }
        if cancel.load(Ordering::Acquire) {
            failure = Some(PceCdLoadError::ArchiveCancelled);
            return Err(cancel_error());
        }
        let name = match normalize_portable_path(entry.name()) {
            Ok(name) => name,
            Err(()) => {
                failure = Some(PceCdLoadError::UnsafeArchiveEntry(entry.name().to_owned()));
                return Err(cancel_error());
            }
        };
        let Some(expected) = archive.entries.iter().find(|member| member.name == name) else {
            failure = Some(PceCdLoadError::ArchiveChanged);
            return Err(cancel_error());
        };
        if !expected.has_stream {
            return Ok(true);
        }

        let output_path = cache_member_path(&files_root, &name);
        let Some(parent) = output_path.parent() else {
            failure = Some(PceCdLoadError::UnsafeArchiveEntry(name));
            return Err(cancel_error());
        };
        if let Err(error) = std::fs::create_dir_all(parent) {
            failure = Some(cache_io_error(error));
            return Err(cancel_error());
        }
        let mut output = match File::create(&output_path) {
            Ok(output) => output,
            Err(error) => {
                failure = Some(cache_io_error(error));
                return Err(cancel_error());
            }
        };
        let mut hasher = Sha256::new();
        let mut local = 0_u64;
        let mut buffer = [0; STREAM_BUFFER_BYTES];
        loop {
            if cancel.load(Ordering::Acquire) {
                failure = Some(PceCdLoadError::ArchiveCancelled);
                return Err(cancel_error());
            }
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            if let Err(error) = output.write_all(&buffer[..count]) {
                failure = Some(cache_io_error(error));
                return Err(cancel_error());
            }
            hasher.update(&buffer[..count]);
            local = match local.checked_add(count as u64) {
                Some(value) => value,
                None => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            decoded = match decoded.checked_add(count as u64) {
                Some(value) if value <= CACHE_ENTRY_BYTES_LIMIT => value,
                _ => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            progress.update_decode_progress(decoded, cancel);
        }
        if local != expected.size {
            failure = Some(PceCdLoadError::ArchiveMemberSizeMismatch(name));
            return Err(cancel_error());
        }
        cached.push(CachedMember {
            name,
            size: local,
            sha256: hasher.finalize().into(),
        });
        Ok(true)
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(map_sevenz_error)?;
    check_cancelled(cancel)?;
    let stream_count = archive
        .entries
        .iter()
        .filter(|member| member.has_stream)
        .count();
    if decoded != decoded_total || cached.len() != stream_count {
        return Err(PceCdLoadError::ArchiveChanged);
    }

    let manifest = CachedArchive {
        version: CACHE_FORMAT_VERSION,
        source_size: identity.source.size,
        source_modified_secs: identity.source.modified_secs,
        source_modified_nanos: identity.source.modified_nanos,
        members: cached,
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(cache_io_error)?;
    if manifest_bytes.len() as u64 > CACHE_MANIFEST_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    std::fs::write(staging.path.join(CACHE_COMPLETE_FILE), manifest_bytes)
        .map_err(cache_io_error)?;
    check_cancelled(cancel)?;

    let final_path = cache_root.join(identity.key);
    if let Some(manifest) = read_cache_manifest(&final_path, archive, identity.source) {
        return Ok(CacheEntry {
            path: final_path,
            manifest,
            extracted_bytes: decoded_total,
        });
    }
    if final_path.exists() {
        remove_cache_entry(cache_root, &final_path);
    }
    if let Err(error) = std::fs::rename(&staging.path, &final_path) {
        if let Some(manifest) = read_cache_manifest(&final_path, archive, identity.source) {
            return Ok(CacheEntry {
                path: final_path,
                manifest,
                extracted_bytes: decoded_total,
            });
        }
        return Err(cache_io_error(error));
    }
    staging.published = true;
    prune_cache(cache_root, Some(&final_path));
    Ok(CacheEntry {
        path: final_path,
        manifest,
        extracted_bytes: decoded_total,
    })
}

fn read_cache_manifest(
    path: &Path,
    archive: &ArchiveManifest,
    source: SourceFingerprint,
) -> Option<CachedArchive> {
    let root_metadata = std::fs::symlink_metadata(path).ok()?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return None;
    }
    let marker = path.join(CACHE_COMPLETE_FILE);
    let marker_metadata = std::fs::symlink_metadata(&marker).ok()?;
    if !marker_metadata.is_file()
        || marker_metadata.file_type().is_symlink()
        || marker_metadata.len() > CACHE_MANIFEST_BYTES_LIMIT
    {
        return None;
    }
    let bytes = std::fs::read(marker).ok()?;
    let cached: CachedArchive = serde_json::from_slice(&bytes).ok()?;
    if cached.version != CACHE_FORMAT_VERSION
        || cached.source_size != source.size
        || cached.source_modified_secs != source.modified_secs
        || cached.source_modified_nanos != source.modified_nanos
        || cached.members.len() > CACHE_ENTRY_MEMBER_LIMIT
    {
        return None;
    }
    let expected = archive
        .entries
        .iter()
        .filter(|member| member.has_stream)
        .collect::<Vec<_>>();
    if cached.members.len() != expected.len() {
        return None;
    }
    for (member, expected) in cached.members.iter().zip(expected) {
        if member.name != expected.name || member.size != expected.size {
            return None;
        }
        let file = cache_member_path(&path.join(CACHE_FILES_DIR), &member.name);
        let metadata = std::fs::symlink_metadata(file).ok()?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != member.size
            || metadata_is_reparse_point(&metadata)
        {
            return None;
        }
    }
    Some(cached)
}

pub(super) fn load_cached_disc(
    cache: &CacheEntry,
    archive: &ArchiveManifest,
    cue_name: &str,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    apply_mods: bool,
) -> Result<LoadedPceCd, CachedDiscError> {
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    let cue = cache
        .manifest
        .members
        .iter()
        .find(|member| member.name == cue_name)
        .ok_or(CachedDiscError::Corrupt)?;
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    progress.set_completed_bytes(cache.extracted_bytes);
    progress.set_total_bytes(cache.extracted_bytes.saturating_add(cue.size));
    let cue_bytes = read_cached_member(cache, cue, cancel, progress, cache.extracted_bytes)?;
    let sheet = parse_cue_bytes(&cue_bytes).map_err(CachedDiscError::Load)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut data_total = 0_u64;
    for file in &sheet.files {
        let name =
            resolve_reference(archive, cue_name, &file.reference).map_err(CachedDiscError::Load)?;
        let member = archive
            .entries
            .iter()
            .find(|member| member.name == name)
            .ok_or_else(|| {
                CachedDiscError::Load(PceCdLoadError::ArchiveMemberMissing(name.clone()))
            })?;
        validate_data_member(member).map_err(CachedDiscError::Load)?;
        data_total = data_total
            .checked_add(member.size)
            .ok_or(CachedDiscError::Load(PceCdLoadError::DataTooLarge(
                u64::MAX,
            )))?;
        if data_total > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(CachedDiscError::Load(PceCdLoadError::DataTooLarge(
                data_total,
            )));
        }
        resolved.push(name);
    }

    let base = cache.extracted_bytes.saturating_add(cue.size);
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    progress.set_total_bytes(base.saturating_add(data_total));
    let mut sources = Vec::with_capacity(resolved.len());
    for name in &resolved {
        let member = cache
            .manifest
            .members
            .iter()
            .find(|member| member.name == name.as_str())
            .ok_or(CachedDiscError::Corrupt)?;
        sources.push(cached_file_source(cache, member)?);
    }
    let overlay_sources = sources.clone();
    let mut verified = 0_u64;
    let mut loaded = load_cached_cue_file_backed(&cue_bytes, &sheet, sources, |count| {
        check_cancelled(cancel)?;
        verified = verified.saturating_add(count);
        progress.update_decode_progress(base.saturating_add(verified), cancel);
        check_cancelled(cancel)
    })
    .map_err(file_backed_cache_error)?;
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    if !apply_mods {
        return Ok(loaded);
    }
    let (dir, mods, selected_crc32) = pce_cd_mod_config(
        crc32fast::hash(&loaded.source_disc_sha256),
        loaded.content_crc32,
    );
    if !mods.iter().any(|entry| entry.enabled) {
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    if let Some(disc) = try_load_cached_cue_ppf_overlay(&sheet, overlay_sources, &dir, &mods)
        .map_err(file_backed_cache_error)?
    {
        loaded.disc = disc;
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    drop(loaded);

    progress.set_total_bytes(base.saturating_add(data_total.saturating_mul(2)));
    let mut completed = base.saturating_add(data_total);
    let mut files = Vec::with_capacity(resolved.len());
    for name in resolved {
        let member = cache
            .manifest
            .members
            .iter()
            .find(|member| member.name == name)
            .ok_or(CachedDiscError::Corrupt)?;
        let bytes = read_cached_member(cache, member, cancel, progress, completed)?;
        completed = completed.saturating_add(member.size);
        files.push(bytes);
    }
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    build_disc_with_mods(cue_bytes, &sheet, files, true).map_err(CachedDiscError::Load)
}

fn cached_file_source(
    cache: &CacheEntry,
    member: &CachedMember,
) -> Result<CueFileSource, CachedDiscError> {
    let path = cache_member_path(&cache.path.join(CACHE_FILES_DIR), &member.name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| CachedDiscError::Corrupt)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || metadata.len() != member.size
    {
        return Err(CachedDiscError::Corrupt);
    }
    Ok(CueFileSource {
        path,
        bytes: member.size,
        sha256: member.sha256,
    })
}

fn file_backed_cache_error(error: PceCdLoadError) -> CachedDiscError {
    match error {
        PceCdLoadError::ArchiveChanged | PceCdLoadError::BinUnreadable(_) => {
            CachedDiscError::Corrupt
        }
        error => CachedDiscError::Load(error),
    }
}

fn read_cached_member(
    cache: &CacheEntry,
    member: &CachedMember,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    progress_base: u64,
) -> Result<Vec<u8>, CachedDiscError> {
    let path = cache_member_path(&cache.path.join(CACHE_FILES_DIR), &member.name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| CachedDiscError::Corrupt)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || metadata.len() != member.size
    {
        return Err(CachedDiscError::Corrupt);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(member.size as usize)
        .map_err(|_| CachedDiscError::Load(PceCdLoadError::ArchiveAllocationFailed))?;
    let mut file = File::open(path).map_err(|_| CachedDiscError::Corrupt)?;
    let mut hasher = Sha256::new();
    let mut local = 0_u64;
    let mut buffer = [0; STREAM_BUFFER_BYTES];
    loop {
        check_cancelled(cancel).map_err(CachedDiscError::Load)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| CachedDiscError::Corrupt)?;
        if count == 0 {
            break;
        }
        local = local
            .checked_add(count as u64)
            .ok_or(CachedDiscError::Corrupt)?;
        if local > member.size {
            return Err(CachedDiscError::Corrupt);
        }
        bytes.extend_from_slice(&buffer[..count]);
        hasher.update(&buffer[..count]);
        progress.update_decode_progress(progress_base.saturating_add(local), cancel);
    }
    if local != member.size || <[u8; 32]>::from(hasher.finalize()) != member.sha256 {
        return Err(CachedDiscError::Corrupt);
    }
    Ok(bytes)
}

fn cache_member_path(root: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(root.to_path_buf(), |path, component| path.join(component))
}

pub(super) fn touch_cache_entry(path: &Path) {
    let marker = path.join(CACHE_COMPLETE_FILE);
    let Ok(bytes) = std::fs::read(&marker) else {
        return;
    };
    let _ = std::fs::write(marker, bytes);
}

pub(super) fn prune_cache(cache_root: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return;
    };
    let mut complete = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            let marker = path.join(CACHE_COMPLETE_FILE);
            let modified = std::fs::metadata(marker).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    complete.sort_by_key(|(modified, _)| *modified);
    while complete.len() > CACHE_MAX_COMPLETE_ENTRIES {
        let index = complete
            .iter()
            .position(|(_, path)| keep != Some(path.as_path()));
        let Some(index) = index else {
            break;
        };
        let (_, path) = complete.remove(index);
        remove_cache_entry(cache_root, &path);
    }
}

fn cleanup_stale_staging(cache_root: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().contains(".tmp-") {
            continue;
        }
        let path = entry.path();
        let stale = std::fs::symlink_metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() >= 24 * 60 * 60);
        if stale {
            remove_cache_entry(cache_root, &path);
        }
    }
}

pub(super) fn remove_cache_entry(cache_root: &Path, path: &Path) {
    if path.parent() != Some(cache_root) {
        return;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        let _ = std::fs::remove_file(path);
    } else if metadata.is_dir() {
        let Ok(resolved_root) = std::fs::canonicalize(cache_root) else {
            return;
        };
        let Ok(resolved_target) = std::fs::canonicalize(path) else {
            return;
        };
        if resolved_target != resolved_root && resolved_target.parent() == Some(&resolved_root) {
            let _ = std::fs::remove_dir_all(resolved_target);
        }
    }
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & WINDOWS_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn cache_io_error(error: impl std::fmt::Display) -> PceCdLoadError {
    PceCdLoadError::Archive(format!("CD cache: {error}"))
}
