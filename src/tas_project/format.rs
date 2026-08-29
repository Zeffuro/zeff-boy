use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use std::ffi::OsString;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use zeff_emu_common::replay::{
    decode_replay_event_stream, decode_replay_start_metadata, encode_replay_event_stream,
    encode_replay_start_metadata,
};

#[cfg(not(target_arch = "wasm32"))]
use super::model::TasProjectLoadSource;
use super::model::{
    FORMAT_VERSION, MAX_CAMERA_ASSET_BYTES, MAX_PROJECT_ASSETS, MAX_PROJECT_BRANCHES,
    MAX_START_STATE_BYTES, TasAnnotation, TasBranch, TasBranchOrigin, TasDigest, TasInputSpan,
    TasMarker, TasProject, TasProjectIdentity, TasVerificationProvenance,
};

const MANIFEST_ENTRY: &str = "manifest.json";
const INTEGRITY_ENTRY: &str = "integrity.json";
const START_STATE_ENTRY: &str = "start_state.bin";
const REPLAY_START_ENTRY: &str = "replay_start.bin";
const MAX_PACKAGE_BYTES: u64 = 96 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INTEGRITY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EVENT_STREAM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REPLAY_START_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES: usize = MAX_PROJECT_ASSETS + MAX_PROJECT_BRANCHES + 4;
const MAX_COMPRESSION_RATIO: u64 = 10_000;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_replay_sha256: Option<TasDigest>,
    identity: TasProjectIdentity,
    start_state_size: u64,
    edit_generation: u64,
    rerecord_count: u64,
    active_branch_id: String,
    project_comment: String,
    branches: Vec<ManifestBranch>,
    markers: Vec<TasMarker>,
    annotations: Vec<TasAnnotation>,
    assets: Vec<ManifestAsset>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestBranch {
    id: String,
    name: String,
    comment: String,
    parent: Option<TasBranchOrigin>,
    frame_count: u64,
    input_spans: Vec<TasInputSpan>,
    verification: Option<TasVerificationProvenance>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestAsset {
    sha256: TasDigest,
    size: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Integrity {
    version: u32,
    entries: BTreeMap<String, TasDigest>,
}

impl TasProject {
    pub fn is_project_path(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ztas"))
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let branches = self.branches.iter().collect::<Vec<_>>();

        let manifest = Manifest {
            format: "zeff-tas-project".to_owned(),
            version: FORMAT_VERSION,
            project_id: self.project_id.clone(),
            source_replay_sha256: self.source_replay_sha256,
            identity: self.canonical_identity(),
            start_state_size: self.start_state.len() as u64,
            edit_generation: self.edit_generation,
            rerecord_count: self.rerecord_count,
            active_branch_id: self.active_branch_id.clone(),
            project_comment: self.project_comment.clone(),
            branches: branches
                .iter()
                .map(|branch| ManifestBranch {
                    id: branch.id.clone(),
                    name: branch.name.clone(),
                    comment: branch.comment.clone(),
                    parent: branch.parent.clone(),
                    frame_count: branch.frame_count,
                    input_spans: branch.input_spans.clone(),
                    verification: branch.verification.clone(),
                })
                .collect(),
            markers: self.markers.clone(),
            annotations: self.annotations.clone(),
            assets: self
                .assets
                .iter()
                .map(|(sha256, bytes)| ManifestAsset {
                    sha256: *sha256,
                    size: bytes.len() as u64,
                })
                .collect(),
        };

        let mut entries = BTreeMap::new();
        let mut uncompressed_size = 0u64;
        insert_encoded_entry(
            &mut entries,
            &mut uncompressed_size,
            MANIFEST_ENTRY.to_owned(),
            serde_json::to_vec(&manifest)?,
        )?;
        insert_encoded_entry(
            &mut entries,
            &mut uncompressed_size,
            START_STATE_ENTRY.to_owned(),
            self.start_state.clone(),
        )?;
        insert_encoded_entry(
            &mut entries,
            &mut uncompressed_size,
            REPLAY_START_ENTRY.to_owned(),
            encode_replay_start_metadata(&self.replay_start)?,
        )?;
        for branch in branches {
            insert_encoded_entry(
                &mut entries,
                &mut uncompressed_size,
                event_entry(&branch.id),
                encode_replay_event_stream(&branch.events)?,
            )?;
        }
        for (digest, bytes) in &self.assets {
            insert_encoded_entry(
                &mut entries,
                &mut uncompressed_size,
                asset_entry(*digest),
                bytes.clone(),
            )?;
        }

        let integrity = Integrity {
            version: FORMAT_VERSION,
            entries: entries
                .iter()
                .map(|(name, bytes)| (name.clone(), TasDigest::from_bytes(bytes)))
                .collect(),
        };

        let integrity_bytes = serde_json::to_vec(&integrity)?;
        if integrity_bytes.len() as u64 > MAX_INTEGRITY_BYTES {
            bail!("TAS integrity manifest exceeds its size limit");
        }
        let uncompressed_size = uncompressed_size
            .checked_add(integrity_bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("TAS project size overflow"))?;
        if uncompressed_size > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the uncompressed size limit");
        }

        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(name, options)?;
            writer.write_all(&bytes)?;
        }
        writer.start_file(INTEGRITY_ENTRY, options)?;
        writer.write_all(&integrity_bytes)?;
        let bytes = writer.finish()?.into_inner();
        if bytes.len() as u64 > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the {MAX_PACKAGE_BYTES}-byte package limit");
        }
        let decoded = Self::decode(&bytes).context("encoded TAS project failed verification")?;
        let mut expected = self.clone();
        expected.identity = self.canonical_identity();
        if decoded != expected {
            bail!("encoded TAS project changed project semantics");
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the {MAX_PACKAGE_BYTES}-byte package limit");
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid TAS ZIP")?;
        let names = inspect_archive(&mut archive)?;

        let manifest_bytes = read_entry(&mut archive, MANIFEST_ENTRY, MAX_MANIFEST_BYTES)?;
        let manifest: Manifest =
            serde_json::from_slice(&manifest_bytes).context("invalid TAS project manifest")?;
        if manifest.format != "zeff-tas-project" {
            bail!("invalid TAS project manifest magic");
        }
        if manifest.version != FORMAT_VERSION {
            bail!("unsupported TAS project version: {}", manifest.version);
        }
        if manifest.branches.len() > MAX_PROJECT_BRANCHES
            || manifest.assets.len() > MAX_PROJECT_ASSETS
        {
            bail!("TAS project manifest exceeds collection limits");
        }

        let integrity_bytes = read_entry(&mut archive, INTEGRITY_ENTRY, MAX_INTEGRITY_BYTES)?;
        let integrity: Integrity = serde_json::from_slice(&integrity_bytes)
            .context("invalid TAS project integrity manifest")?;
        if integrity.version != FORMAT_VERSION {
            bail!("unsupported TAS integrity version: {}", integrity.version);
        }

        let expected = expected_entries(&manifest)?;
        if names != expected {
            bail!("TAS project package entries do not match its manifest");
        }
        let expected_integrity = expected
            .iter()
            .filter(|name| name.as_str() != INTEGRITY_ENTRY)
            .cloned()
            .collect::<BTreeSet<_>>();
        if integrity.entries.keys().cloned().collect::<BTreeSet<_>>() != expected_integrity {
            bail!("TAS project integrity entries do not match its manifest");
        }

        let mut verified = BTreeMap::new();
        for (name, expected_digest) in &integrity.entries {
            let limit = entry_limit(name)?;
            let entry = read_entry(&mut archive, name, limit)?;
            if TasDigest::from_bytes(&entry) != *expected_digest {
                bail!("TAS project entry {name:?} failed its SHA-256 check");
            }
            verified.insert(name.clone(), entry);
        }

        let start_state = verified
            .remove(START_STATE_ENTRY)
            .ok_or_else(|| anyhow::anyhow!("TAS project is missing its starting state"))?;
        if start_state.len() as u64 != manifest.start_state_size {
            bail!("TAS starting state size does not match its manifest");
        }
        let replay_start = decode_replay_start_metadata(
            &verified
                .remove(REPLAY_START_ENTRY)
                .ok_or_else(|| anyhow::anyhow!("TAS project is missing replay start metadata"))?,
        )?;

        let mut branches = Vec::with_capacity(manifest.branches.len());
        for branch in manifest.branches {
            let bytes = verified
                .remove(&event_entry(&branch.id))
                .ok_or_else(|| anyhow::anyhow!("TAS branch is missing its event stream"))?;
            branches.push(TasBranch {
                id: branch.id,
                name: branch.name,
                comment: branch.comment,
                parent: branch.parent,
                frame_count: branch.frame_count,
                input_spans: branch.input_spans,
                events: decode_replay_event_stream(&bytes)?,
                verification: branch.verification,
            });
        }

        let mut assets = BTreeMap::new();
        for asset in manifest.assets {
            let name = asset_entry(asset.sha256);
            let bytes = verified
                .remove(&name)
                .ok_or_else(|| anyhow::anyhow!("TAS project is missing asset {name:?}"))?;
            if bytes.len() as u64 != asset.size {
                bail!("TAS asset size does not match its manifest");
            }
            if assets.insert(asset.sha256, bytes).is_some() {
                bail!("duplicate TAS asset identity");
            }
        }

        let project = Self {
            project_id: manifest.project_id,
            source_replay_sha256: manifest.source_replay_sha256,
            identity: manifest.identity,
            start_state,
            replay_start,
            edit_generation: manifest.edit_generation,
            rerecord_count: manifest.rerecord_count,
            active_branch_id: manifest.active_branch_id,
            project_comment: manifest.project_comment,
            branches,
            markers: manifest.markers,
            annotations: manifest.annotations,
            assets,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("failed to inspect TAS project {}", path.display()))?;
        if metadata.len() > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the {MAX_PACKAGE_BYTES}-byte package limit");
        }
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open TAS project {}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the {MAX_PACKAGE_BYTES}-byte package limit");
        }
        Self::decode(&bytes)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        if !Self::is_project_path(path) {
            bail!("TAS projects must use the .ztas extension");
        }
        let bytes = self.encode()?;
        if path.exists() {
            let previous = Self::load(path).with_context(|| {
                format!("refusing to replace invalid TAS project {}", path.display())
            })?;
            let previous_bytes = previous.encode()?;
            publish_snapshot(&backup_path(path)?, &previous_bytes)?;
        }
        publish_snapshot(path, &bytes)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_with_backup(path: &Path) -> Result<(Self, TasProjectLoadSource)> {
        match Self::load(path) {
            Ok(project) => Ok((project, TasProjectLoadSource::Primary)),
            Err(primary_error) => {
                let backup = backup_path(path)?;
                let project = Self::load(&backup).with_context(|| {
                    format!(
                        "primary TAS project {} failed ({primary_error:#}); backup {} also failed",
                        path.display(),
                        backup.display()
                    )
                })?;
                Ok((project, TasProjectLoadSource::Backup))
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn backup_path(path: &Path) -> Result<PathBuf> {
        backup_path(path)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_snapshot(target: &Path, bytes: &[u8]) -> Result<()> {
    crate::platform::write_file_atomically_validated(target, bytes, |temp_file| {
        temp_file.rewind()?;
        let mut temp_bytes = Vec::new();
        temp_file
            .take(MAX_PACKAGE_BYTES + 1)
            .read_to_end(&mut temp_bytes)?;
        TasProject::decode(&temp_bytes)
            .context("temporary TAS project failed validation")
            .map(|_| ())
    })
    .with_context(|| {
        format!(
            "failed to atomically publish TAS project {}",
            target.display()
        )
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn backup_path(target: &Path) -> Result<PathBuf> {
    let file_name = target
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("TAS project target has no file name"))?;
    let mut backup_name = OsString::from(file_name);
    backup_name.push(".bak");
    Ok(target.with_file_name(backup_name))
}

fn inspect_archive<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<BTreeSet<String>> {
    if archive.len() > MAX_ENTRIES {
        bail!("TAS project contains too many ZIP entries");
    }
    let mut names = BTreeSet::new();
    let mut total_size = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if entry.is_dir()
            || entry.is_symlink()
            || entry.enclosed_name().is_none()
            || name.contains('\\')
            || !valid_entry_name(&name)
        {
            bail!("invalid TAS project ZIP entry {name:?}");
        }
        if !names.insert(name.clone()) {
            bail!("duplicate TAS project ZIP entry {name:?}");
        }
        let limit = entry_limit(&name)?;
        if entry.size() > limit {
            bail!("TAS project ZIP entry {name:?} exceeds its size limit");
        }
        total_size = total_size
            .checked_add(entry.size())
            .ok_or_else(|| anyhow::anyhow!("TAS project ZIP size overflow"))?;
        if total_size > MAX_PACKAGE_BYTES {
            bail!("TAS project exceeds the uncompressed size limit");
        }
        if entry.size() != 0
            && (entry.compressed_size() == 0
                || entry.size()
                    > entry
                        .compressed_size()
                        .saturating_mul(MAX_COMPRESSION_RATIO))
        {
            bail!("TAS project ZIP entry {name:?} has an excessive compression ratio");
        }
    }
    if !names.contains(MANIFEST_ENTRY)
        || !names.contains(INTEGRITY_ENTRY)
        || !names.contains(START_STATE_ENTRY)
        || !names.contains(REPLAY_START_ENTRY)
    {
        bail!("TAS project is missing a required ZIP entry");
    }
    Ok(names)
}

fn insert_encoded_entry(
    entries: &mut BTreeMap<String, Vec<u8>>,
    total_size: &mut u64,
    name: String,
    bytes: Vec<u8>,
) -> Result<()> {
    let limit = entry_limit(&name)?;
    if bytes.len() as u64 > limit {
        bail!("TAS project entry {name:?} exceeds its size limit");
    }
    *total_size = total_size
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| anyhow::anyhow!("TAS project size overflow"))?;
    if *total_size > MAX_PACKAGE_BYTES {
        bail!("TAS project exceeds the uncompressed size limit");
    }
    if entries.insert(name.clone(), bytes).is_some() {
        bail!("duplicate TAS project entry {name:?}");
    }
    Ok(())
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("missing TAS project entry {name:?}"))?;
    let mut bytes = Vec::with_capacity(entry.size().min(limit) as usize);
    entry.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("TAS project entry {name:?} exceeds its size limit");
    }
    Ok(bytes)
}

fn expected_entries(manifest: &Manifest) -> Result<BTreeSet<String>> {
    let mut entries = BTreeSet::from([
        MANIFEST_ENTRY.to_owned(),
        INTEGRITY_ENTRY.to_owned(),
        START_STATE_ENTRY.to_owned(),
        REPLAY_START_ENTRY.to_owned(),
    ]);
    for branch in &manifest.branches {
        if !entries.insert(event_entry(&branch.id)) {
            bail!("duplicate TAS branch entry");
        }
    }
    for asset in &manifest.assets {
        if !entries.insert(asset_entry(asset.sha256)) {
            bail!("duplicate TAS asset entry");
        }
    }
    Ok(entries)
}

fn valid_entry_name(name: &str) -> bool {
    matches!(
        name,
        MANIFEST_ENTRY | INTEGRITY_ENTRY | START_STATE_ENTRY | REPLAY_START_ENTRY
    ) || name
        .strip_prefix("branches/")
        .and_then(|name| name.strip_suffix("/events.bin"))
        .is_some_and(valid_path_id)
        || name
            .strip_prefix("assets/")
            .and_then(|name| name.strip_suffix(".bin"))
            .is_some_and(|digest| {
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

fn valid_path_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn entry_limit(name: &str) -> Result<u64> {
    if name == MANIFEST_ENTRY {
        Ok(MAX_MANIFEST_BYTES)
    } else if name == INTEGRITY_ENTRY {
        Ok(MAX_INTEGRITY_BYTES)
    } else if name == START_STATE_ENTRY {
        Ok(MAX_START_STATE_BYTES as u64)
    } else if name == REPLAY_START_ENTRY {
        Ok(MAX_REPLAY_START_BYTES)
    } else if name.starts_with("branches/") {
        Ok(MAX_EVENT_STREAM_BYTES)
    } else if name.starts_with("assets/") {
        Ok(MAX_CAMERA_ASSET_BYTES as u64)
    } else {
        bail!("unknown TAS project entry {name:?}")
    }
}

fn event_entry(branch_id: &str) -> String {
    format!("branches/{branch_id}/events.bin")
}

fn asset_entry(digest: TasDigest) -> String {
    format!("assets/{}.bin", digest.to_hex())
}
