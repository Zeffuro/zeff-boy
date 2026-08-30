use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use sha2::{Digest as _, Sha256};

use super::model::{TasDigest, TasSeekCacheIdentity};

pub const MAX_SEEK_CACHE_ENTRIES: usize = 512;
pub const MAX_SEEK_CACHE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_SEEK_CACHE_STATE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SEEK_CACHE_STATE_FORMAT_ID_BYTES: usize = 128;
pub const MAX_SEEK_CACHE_FILE_BYTES: usize = MAX_SEEK_CACHE_STATE_BYTES + 512;
pub const MAX_SEEK_CACHE_DIRECTORY_ENTRIES: usize = MAX_SEEK_CACHE_ENTRIES * 2;
pub(crate) const MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD: usize = 8;

const CACHE_MAGIC: &[u8; 8] = b"ZSCACHE1";
const CACHE_FILE_VERSION: u32 = 1;
const CACHE_KEY_DOMAIN: &[u8] = b"ZEFF-TAS-SEEK-CACHE-KEY-1\0";
const CACHE_FILE_EXTENSION: &str = "zsc";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TasSeekCacheLimits {
    pub max_entries: usize,
    pub max_bytes: u64,
    pub max_state_bytes: usize,
}

impl Default for TasSeekCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_SEEK_CACHE_ENTRIES,
            max_bytes: MAX_SEEK_CACHE_BYTES,
            max_state_bytes: MAX_SEEK_CACHE_STATE_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TasSeekStateCache {
    directory: crate::platform::StableDirectory,
    limits: TasSeekCacheLimits,
}

impl TasSeekStateCache {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(root, TasSeekCacheLimits::default())
    }

    pub fn open_with_limits(root: impl AsRef<Path>, limits: TasSeekCacheLimits) -> Result<Self> {
        validate_limits(limits)?;
        let requested_root = root.as_ref();
        reject_project_container(requested_root)?;
        let directory = crate::platform::StableDirectory::open_or_create(
            requested_root,
            "TAS seek cache root",
        )?;
        reject_project_container(directory.path())?;
        Ok(Self { directory, limits })
    }

    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    pub fn limits(&self) -> TasSeekCacheLimits {
        self.limits
    }

    pub fn path_for(&self, identity: &TasSeekCacheIdentity) -> Result<PathBuf> {
        let key = cache_key(identity)?;
        Ok(self
            .directory
            .path()
            .join(format!("{}.{}", key.to_hex(), CACHE_FILE_EXTENSION)))
    }

    pub fn load(&self, identity: &TasSeekCacheIdentity) -> Result<Option<Vec<u8>>> {
        self.validate_root()?;
        let path = self.path_for(identity)?;
        self.reject_case_equivalent_target(&path)?;
        let bytes = match self.read_entry_file(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(_) => {
                self.remove_entry_best_effort(&path);
                return Ok(None);
            }
        };

        match decode_entry(&bytes, self.limits.max_state_bytes) {
            Ok((stored_identity, state))
                if stored_identity == *identity
                    && TasDigest::from_bytes(&state.bytes) == state.sha256 =>
            {
                Ok(Some(state.bytes))
            }
            Ok(_) | Err(_) => {
                self.remove_entry_best_effort(&path);
                Ok(None)
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn load_newest_matching(
        &self,
        target_cursor: u64,
        mut expected_identity_at: impl FnMut(u64) -> Result<TasSeekCacheIdentity>,
    ) -> Result<Option<(u64, Vec<u8>)>> {
        self.validate_root()?;
        let mut candidates = Vec::new();

        for entry in self.collect_entries()? {
            let bytes = match self.read_entry_file(&entry.path) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(_) => {
                    self.remove_entry_best_effort(&entry.path);
                    continue;
                }
            };
            let (stored_identity, stored_state) =
                match decode_entry(&bytes, self.limits.max_state_bytes) {
                    Ok(decoded) => decoded,
                    Err(_) => {
                        self.remove_entry_best_effort(&entry.path);
                        continue;
                    }
                };
            if self.path_for(&stored_identity)? != entry.path
                || TasDigest::from_bytes(&stored_state.bytes) != stored_state.sha256
            {
                self.remove_entry_best_effort(&entry.path);
                continue;
            }
            if stored_identity.cursor > target_cursor {
                continue;
            }
            candidates.push((stored_identity, stored_state.bytes));
            candidates.sort_unstable_by_key(|(identity, _)| std::cmp::Reverse(identity.cursor));
            candidates.truncate(MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD);
        }

        for (stored_identity, state) in candidates {
            if stored_identity == expected_identity_at(stored_identity.cursor)? {
                return Ok(Some((stored_identity.cursor, state)));
            }
        }
        Ok(None)
    }

    pub fn store(&self, identity: &TasSeekCacheIdentity, state: &[u8]) -> Result<()> {
        self.validate_root()?;
        let bytes = encode_entry(identity, state, self.limits.max_state_bytes)?;
        let path = self.path_for(identity)?;
        self.prune_for_write(&path, bytes.len() as u64)?;
        self.validate_root()?;

        let expected_identity = identity.clone();
        crate::platform::write_file_atomically_validated(&path, &bytes, |temp_file| {
            temp_file.rewind()?;
            let mut written = Vec::with_capacity(bytes.len());
            temp_file
                .take((MAX_SEEK_CACHE_FILE_BYTES as u64) + 1)
                .read_to_end(&mut written)?;
            let (stored_identity, stored_state) =
                decode_entry(&written, self.limits.max_state_bytes)?;
            if stored_identity != expected_identity {
                bail!("temporary TAS seek cache identity did not round-trip")
            }
            if TasDigest::from_bytes(&stored_state.bytes) != stored_state.sha256 {
                bail!("temporary TAS seek cache state SHA-256 did not round-trip")
            }
            Ok(())
        })
        .with_context(|| {
            format!(
                "failed to atomically write TAS seek state {}",
                path.display()
            )
        })?;

        self.prune()?;
        Ok(())
    }

    pub fn prune(&self) -> Result<()> {
        self.validate_root()?;
        self.prune_to_budget(None, 0, false)
    }

    fn read_entry_file(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        self.validate_root()?;
        if !self.is_direct_cache_entry(path) {
            return Ok(None);
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.len() > MAX_SEEK_CACHE_FILE_BYTES as u64 {
            bail!("TAS seek cache entry exceeds its file limit")
        }

        let file = File::open(path)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take((MAX_SEEK_CACHE_FILE_BYTES as u64) + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_SEEK_CACHE_FILE_BYTES {
            bail!("TAS seek cache entry exceeds its file limit")
        }
        Ok(Some(bytes))
    }

    fn prune_for_write(&self, target: &Path, incoming_bytes: u64) -> Result<()> {
        self.validate_root()?;
        // Reserve space for the sibling temp file as well as the current entry.
        let replacing_existing_entry = matches!(
            fs::symlink_metadata(target),
            Ok(metadata) if metadata.file_type().is_file()
        );
        self.prune_to_budget(Some(target), incoming_bytes, !replacing_existing_entry)
    }

    fn prune_to_budget(
        &self,
        protected: Option<&Path>,
        reserve_bytes: u64,
        reserve_entry: bool,
    ) -> Result<()> {
        self.validate_root()?;
        let mut entries = self.collect_entries()?;
        let mut total_bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
        if reserve_bytes > self.limits.max_bytes {
            bail!("TAS seek cache write exceeds the total cache-byte limit")
        }

        entries.sort_by(|left, right| {
            left.modified
                .cmp(&right.modified)
                .then_with(|| left.name.cmp(&right.name))
        });

        while entries.len().saturating_add(usize::from(reserve_entry)) > self.limits.max_entries
            || total_bytes.saturating_add(reserve_bytes) > self.limits.max_bytes
        {
            let Some(index) = entries
                .iter()
                .position(|entry| Some(entry.path.as_path()) != protected)
            else {
                bail!("TAS seek cache cannot make room without removing its protected entry")
            };
            let entry = entries.remove(index);
            self.remove_regular_entry(&entry.path)?;
            total_bytes = total_bytes.saturating_sub(entry.bytes);
        }
        Ok(())
    }

    fn collect_entries(&self) -> Result<Vec<CacheEntry>> {
        self.validate_root()?;
        let mut entries = Vec::new();
        let root = self.directory.path();
        for (index, entry) in fs::read_dir(root)?.enumerate() {
            if index >= MAX_SEEK_CACHE_DIRECTORY_ENTRIES {
                bail!(
                    "TAS seek cache directory exceeds the {MAX_SEEK_CACHE_DIRECTORY_ENTRIES}-entry scan limit"
                )
            }
            let entry = entry?;
            let path = entry.path();
            if path.parent() != Some(root) {
                continue;
            }
            let name = entry.file_name();
            if is_case_equivalent_cache_entry_name(&name) {
                bail!("TAS seek cache contains a case-equivalent cache filename")
            }
            if !is_cache_entry_name(&name) {
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_file() => metadata,
                Ok(_) => continue,
                Err(_) => continue,
            };
            if metadata.len() > MAX_SEEK_CACHE_FILE_BYTES as u64 {
                self.remove_regular_entry(&path)?;
                continue;
            }
            entries.push(CacheEntry {
                name: entry.file_name(),
                path,
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            });
        }
        Ok(entries)
    }

    fn is_direct_cache_entry(&self, path: &Path) -> bool {
        path.parent() == Some(self.directory.path())
            && path.file_name().is_some_and(is_cache_entry_name)
    }

    fn remove_entry_best_effort(&self, path: &Path) {
        let _ = self.remove_regular_entry(path);
    }

    fn remove_regular_entry(&self, path: &Path) -> Result<()> {
        self.validate_root()?;
        if !self.is_direct_cache_entry(path) {
            return Ok(());
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        fs::remove_file(path)
            .with_context(|| format!("failed to prune TAS seek cache entry {}", path.display()))
    }

    fn validate_root(&self) -> Result<()> {
        reject_project_container(self.directory.path())?;
        self.directory.revalidate()?;
        reject_project_container(self.directory.path())?;
        Ok(())
    }

    fn reject_case_equivalent_target(&self, target: &Path) -> Result<()> {
        #[cfg(windows)]
        {
            let target_name = target
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("TAS seek cache target has no filename"))?;
            for (index, entry) in fs::read_dir(self.directory.path())?.enumerate() {
                if index >= MAX_SEEK_CACHE_DIRECTORY_ENTRIES {
                    bail!(
                        "TAS seek cache directory exceeds the {MAX_SEEK_CACHE_DIRECTORY_ENTRIES}-entry scan limit"
                    )
                }
                let entry = entry?;
                let name = entry.file_name();
                if name != target_name
                    && name
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&target_name.to_string_lossy())
                {
                    bail!("TAS seek cache target conflicts with a case-equivalent filename")
                }
            }
        }
        #[cfg(not(windows))]
        let _ = target;
        Ok(())
    }
}

#[derive(Debug)]
struct CacheEntry {
    name: std::ffi::OsString,
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

#[derive(Debug)]
struct StoredState {
    sha256: TasDigest,
    bytes: Vec<u8>,
}

fn validate_limits(limits: TasSeekCacheLimits) -> Result<()> {
    if limits.max_entries == 0 || limits.max_entries > MAX_SEEK_CACHE_ENTRIES {
        bail!("TAS seek cache entry limit must be 1..={MAX_SEEK_CACHE_ENTRIES}")
    }
    if limits.max_bytes == 0 || limits.max_bytes > MAX_SEEK_CACHE_BYTES {
        bail!("TAS seek cache byte limit must be 1..={MAX_SEEK_CACHE_BYTES}")
    }
    if limits.max_state_bytes == 0 || limits.max_state_bytes > MAX_SEEK_CACHE_STATE_BYTES {
        bail!("TAS seek cache state limit must be 1..={MAX_SEEK_CACHE_STATE_BYTES} bytes")
    }
    Ok(())
}

fn reject_project_container(path: &Path) -> Result<()> {
    if path.ancestors().any(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ztas"))
    }) {
        bail!("TAS seek cache root must remain outside a .ztas project package")
    }
    Ok(())
}

fn cache_key(identity: &TasSeekCacheIdentity) -> Result<TasDigest> {
    let identity_bytes = encode_identity(identity)?;
    let mut hash = Sha256::new();
    hash.update(CACHE_KEY_DOMAIN);
    hash.update((identity_bytes.len() as u64).to_le_bytes());
    hash.update(identity_bytes);
    Ok(TasDigest(hash.finalize().into()))
}

fn encode_entry(
    identity: &TasSeekCacheIdentity,
    state: &[u8],
    max_state_bytes: usize,
) -> Result<Vec<u8>> {
    if state.len() > max_state_bytes {
        bail!("TAS seek state exceeds the configured state-byte limit")
    }
    let identity_bytes = encode_identity(identity)?;
    let state_sha256 = TasDigest::from_bytes(state);
    let capacity = CACHE_MAGIC.len()
        + std::mem::size_of::<u32>()
        + identity_bytes.len()
        + std::mem::size_of::<u32>()
        + state_sha256.0.len()
        + state.len();
    if capacity > MAX_SEEK_CACHE_FILE_BYTES {
        bail!("TAS seek cache entry exceeds its file-byte limit")
    }

    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(CACHE_MAGIC);
    bytes.extend_from_slice(&CACHE_FILE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&identity_bytes);
    bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&state_sha256.0);
    bytes.extend_from_slice(state);
    Ok(bytes)
}

fn decode_entry(
    bytes: &[u8],
    max_state_bytes: usize,
) -> Result<(TasSeekCacheIdentity, StoredState)> {
    if bytes.len() > MAX_SEEK_CACHE_FILE_BYTES {
        bail!("TAS seek cache entry exceeds its file-byte limit")
    }
    let mut cursor = 0;
    if take(bytes, &mut cursor, CACHE_MAGIC.len())? != CACHE_MAGIC {
        bail!("invalid TAS seek cache magic")
    }
    if read_u32(bytes, &mut cursor)? != CACHE_FILE_VERSION {
        bail!("unsupported TAS seek cache file version")
    }
    let identity = decode_identity(bytes, &mut cursor)?;
    let state_len = usize::try_from(read_u32(bytes, &mut cursor)?)?;
    if state_len > max_state_bytes {
        bail!("TAS seek cache state exceeds the configured state-byte limit")
    }
    let sha256 = TasDigest(read_array(bytes, &mut cursor)?);
    let state = take(bytes, &mut cursor, state_len)?.to_vec();
    if cursor != bytes.len() {
        bail!("trailing bytes in TAS seek cache entry")
    }
    Ok((
        identity,
        StoredState {
            sha256,
            bytes: state,
        },
    ))
}

fn encode_identity(identity: &TasSeekCacheIdentity) -> Result<Vec<u8>> {
    let state_format = identity.state_format_compatibility_id.as_bytes();
    if state_format.is_empty()
        || state_format.len() > MAX_SEEK_CACHE_STATE_FORMAT_ID_BYTES
        || !state_format.iter().copied().all(is_identity_byte)
    {
        bail!("invalid TAS seek-cache state format compatibility ID")
    }

    let mut bytes = Vec::with_capacity(4 + 2 + state_format.len() + 32 + 32 + 8);
    bytes.extend_from_slice(&identity.cache_format_version.to_le_bytes());
    bytes.extend_from_slice(&(state_format.len() as u16).to_le_bytes());
    bytes.extend_from_slice(state_format);
    bytes.extend_from_slice(&identity.sync_identity_sha256.0);
    bytes.extend_from_slice(&identity.branch_prefix_sha256.0);
    bytes.extend_from_slice(&identity.cursor.to_le_bytes());
    Ok(bytes)
}

fn decode_identity(bytes: &[u8], cursor: &mut usize) -> Result<TasSeekCacheIdentity> {
    let cache_format_version = read_u32(bytes, cursor)?;
    let state_format_len = usize::from(read_u16(bytes, cursor)?);
    if state_format_len == 0 || state_format_len > MAX_SEEK_CACHE_STATE_FORMAT_ID_BYTES {
        bail!("invalid TAS seek-cache state format compatibility ID length")
    }
    let state_format = take(bytes, cursor, state_format_len)?;
    if !state_format.iter().copied().all(is_identity_byte) {
        bail!("invalid TAS seek-cache state format compatibility ID")
    }
    let state_format_compatibility_id = std::str::from_utf8(state_format)?.to_owned();
    let sync_identity_sha256 = TasDigest(read_array(bytes, cursor)?);
    let branch_prefix_sha256 = TasDigest(read_array(bytes, cursor)?);
    let cursor = read_u64(bytes, cursor)?;
    Ok(TasSeekCacheIdentity {
        cache_format_version,
        state_format_compatibility_id,
        sync_identity_sha256,
        branch_prefix_sha256,
        cursor,
    })
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("TAS seek cache offset overflow"))?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| anyhow::anyhow!("truncated TAS seek cache entry"))?;
    *cursor = end;
    Ok(slice)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16> {
    Ok(u16::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N]> {
    take(bytes, cursor, N)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid TAS seek cache field length"))
}

fn is_identity_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
}

fn is_cache_entry_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some((key, extension)) = name.rsplit_once('.') else {
        return false;
    };
    extension == CACHE_FILE_EXTENSION
        && key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(windows)]
fn is_case_equivalent_cache_entry_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower != name && is_cache_entry_name(OsStr::new(&lower))
}

#[cfg(not(windows))]
fn is_case_equivalent_cache_entry_name(_name: &OsStr) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_directory;

    fn identity(prefix: u8, cursor: u64) -> TasSeekCacheIdentity {
        TasSeekCacheIdentity {
            cache_format_version: 1,
            state_format_compatibility_id: "nes-native-v11".to_owned(),
            sync_identity_sha256: TasDigest([0xA1; 32]),
            branch_prefix_sha256: TasDigest([prefix; 32]),
            cursor,
        }
    }

    #[test]
    fn stores_at_a_deterministic_content_keyed_path_and_verifies_state_sha() {
        let root = test_directory("tas-seek-cache").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        let identity = identity(0x10, 6);
        let path = cache.path_for(&identity).unwrap();

        cache.store(&identity, b"synthetic state").unwrap();
        assert_eq!(cache.path_for(&identity).unwrap(), path);
        assert_eq!(
            cache.load(&identity).unwrap(),
            Some(b"synthetic state".to_vec())
        );

        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        fs::write(&path, bytes).unwrap();
        assert_eq!(cache.load(&identity).unwrap(), None);
        assert!(!path.exists());
    }

    #[test]
    fn identity_mismatch_is_disposable() {
        let root = test_directory("tas-seek-cache-mismatch").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        let stored = identity(0x20, 4);
        let requested = identity(0x20, 5);
        let requested_path = cache.path_for(&requested).unwrap();
        let bytes = encode_entry(&stored, b"state", MAX_SEEK_CACHE_STATE_BYTES).unwrap();
        fs::write(&requested_path, bytes).unwrap();

        assert_eq!(cache.load(&requested).unwrap(), None);
        assert!(!requested_path.exists());
    }

    #[test]
    fn shared_prefix_reuses_a_state_but_an_edit_at_n_rejects_later_cursor() {
        let root = test_directory("tas-seek-cache-prefix").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        let before_edit = identity(0x31, 12);
        let shared_branch = identity(0x31, 12);
        cache.store(&before_edit, b"cursor before frame N").unwrap();
        assert_eq!(
            cache.load(&shared_branch).unwrap(),
            Some(b"cursor before frame N".to_vec())
        );

        let after_edit_before_n = identity(0x31, 12);
        let before_edit_after_n = identity(0x41, 13);
        let after_edit_after_n = identity(0x42, 13);
        cache
            .store(&before_edit_after_n, b"old post-edit-prefix state")
            .unwrap();
        assert_eq!(
            cache.load(&after_edit_before_n).unwrap(),
            Some(b"cursor before frame N".to_vec())
        );
        assert_eq!(cache.load(&after_edit_after_n).unwrap(), None);
    }

    #[test]
    fn newest_matching_scan_is_bounded_by_entries_and_skips_foreign_or_corrupt_states() {
        let root = test_directory("tas-seek-cache-newest").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        cache.store(&identity(3, 3), b"cursor three").unwrap();
        cache.store(&identity(7, 7), b"cursor seven").unwrap();
        cache
            .store(&identity(9, 9), b"foreign cursor nine")
            .unwrap();

        let newest = cache
            .load_newest_matching(8, |cursor| Ok(identity(cursor as u8, cursor)))
            .unwrap();
        assert_eq!(newest, Some((7, b"cursor seven".to_vec())));

        let corrupt_path = cache.path_for(&identity(7, 7)).unwrap();
        let mut corrupt = fs::read(&corrupt_path).unwrap();
        *corrupt.last_mut().unwrap() ^= 1;
        fs::write(&corrupt_path, corrupt).unwrap();
        let newest = cache
            .load_newest_matching(8, |cursor| Ok(identity(cursor as u8, cursor)))
            .unwrap();
        assert_eq!(newest, Some((3, b"cursor three".to_vec())));
        assert!(!corrupt_path.exists());
        assert!(cache.path_for(&identity(9, 9)).unwrap().exists());
    }

    #[test]
    fn newest_matching_bounds_project_prefix_identity_work() {
        let root = test_directory("tas-seek-cache-identity-budget").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        for cursor in 1..=12 {
            cache
                .store(&identity(cursor as u8, cursor), &[cursor as u8])
                .unwrap();
        }

        let checks = std::cell::Cell::new(0usize);
        let newest = cache
            .load_newest_matching(12, |cursor| {
                checks.set(checks.get() + 1);
                Ok(identity(0xFF, cursor))
            })
            .unwrap();

        assert_eq!(newest, None);
        assert_eq!(checks.get(), MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD);
    }

    #[test]
    fn pruning_is_bounded_and_never_removes_a_sibling_outside_the_exact_root() {
        let parent = test_directory("tas-seek-cache-prune").unwrap();
        let root = parent.path().join("cache");
        let outside = parent.path().join("outside.zsc");
        fs::write(&outside, b"outside").unwrap();
        let cache = TasSeekStateCache::open_with_limits(
            &root,
            TasSeekCacheLimits {
                max_entries: 2,
                max_bytes: 1024,
                max_state_bytes: 128,
            },
        )
        .unwrap();

        cache.store(&identity(1, 1), &[1; 32]).unwrap();
        cache.store(&identity(2, 2), &[2; 32]).unwrap();
        cache.store(&identity(3, 3), &[3; 32]).unwrap();

        assert!(outside.exists());
        assert!(cache.collect_entries().unwrap().len() <= 2);
        assert!(
            cache
                .collect_entries()
                .unwrap()
                .iter()
                .map(|entry| entry.bytes)
                .sum::<u64>()
                <= 1024
        );
    }

    #[test]
    fn cache_root_cannot_be_inside_a_project_package() {
        let root = test_directory("tas-seek-cache-ztas").unwrap();
        let project = root.path().join("project.ztas");
        assert!(TasSeekStateCache::open(project.join("cache")).is_err());
    }

    #[test]
    fn directory_scan_is_bounded() {
        let root = test_directory("tas-seek-cache-scan-limit").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        for index in 0..=MAX_SEEK_CACHE_DIRECTORY_ENTRIES {
            fs::write(root.path().join(format!("unrelated-{index}")), b"").unwrap();
        }

        assert!(cache.prune().is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn replaced_root_is_rejected_before_read_write_or_prune() {
        let parent = test_directory("tas-seek-cache-root-replacement").unwrap();
        let root = parent.path().join("cache");
        let cache = TasSeekStateCache::open(&root).unwrap();
        let displaced = parent.path().join("displaced-cache");
        fs::rename(&root, &displaced).unwrap();
        fs::create_dir(&root).unwrap();

        let identity = identity(0x71, 9);
        assert!(cache.load(&identity).is_err());
        assert!(
            cache
                .store(&identity, b"must not reach replacement")
                .is_err()
        );
        assert!(cache.prune().is_err());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn stable_directory_guard_allows_normal_cache_operations() {
        let root = test_directory("tas-seek-cache-windows-root-guard").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        let identity = identity(0x73, 11);
        cache.store(&identity, b"guarded state").unwrap();
        assert_eq!(
            cache.load(&identity).unwrap(),
            Some(b"guarded state".to_vec())
        );
        cache.prune().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn case_equivalent_cache_filename_is_rejected_before_accounting() {
        let root = test_directory("tas-seek-cache-case-equivalent").unwrap();
        let cache = TasSeekStateCache::open(root.path()).unwrap();
        let identity = identity(0x72, 10);
        let canonical = cache.path_for(&identity).unwrap();
        let upper = canonical
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_uppercase();
        fs::write(root.path().join(upper), b"not a valid cache entry").unwrap();

        assert!(cache.load(&identity).is_err());
        assert!(cache.prune().is_err());
        assert!(
            cache
                .store(&identity, b"must not overwrite case-equivalent")
                .is_err()
        );
    }
}
