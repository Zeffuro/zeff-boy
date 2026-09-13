use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use zeff_audio_discovery::classification::AudioRole;

use super::{ScanManifest, SongId};

const SCHEMA: &str = "zeff-audio-classifications/1";
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedRoles {
    schema: String,
    system: String,
    sha256: String,
    pub(super) entries: BTreeMap<String, AudioRole>,
}

pub(super) fn path(manifest: &ScanManifest) -> Result<PathBuf> {
    let sha = manifest
        .scan
        .media
        .sha256
        .as_deref()
        .context("scan has no media identity")?;
    ensure!(
        sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid media identity"
    );
    let system = manifest.scan.media.system;
    ensure!(
        system
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "invalid media system"
    );
    Ok(crate::platform::settings_dir()
        .join("audio-classifications")
        .join(format!("{system}-{sha}.json")))
}

pub(super) fn read(path: &Path, manifest: &ScanManifest) -> Result<SavedRoles> {
    let sha = manifest
        .scan
        .media
        .sha256
        .as_deref()
        .context("scan has no media identity")?;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SavedRoles {
                schema: SCHEMA.into(),
                system: manifest.scan.media.system.into(),
                sha256: sha.into(),
                entries: BTreeMap::new(),
            });
        }
        Err(error) => return Err(error).context("could not read saved audio classifications"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "audio classification file is too large"
    );
    let saved: SavedRoles =
        serde_json::from_slice(&bytes).context("invalid saved audio classifications")?;
    ensure!(
        saved.schema == SCHEMA && saved.system == manifest.scan.media.system && saved.sha256 == sha,
        "saved audio classifications do not match this source"
    );
    validate(&saved)?;
    Ok(saved)
}

fn validate(saved: &SavedRoles) -> Result<()> {
    ensure!(
        saved.entries.len() <= 16384 && saved.entries.keys().all(|key| key.len() <= 256),
        "too many or invalid saved classifications"
    );
    Ok(())
}

pub(super) fn save(
    path: &Path,
    manifest: &mut ScanManifest,
    id: SongId,
    role: Option<AudioRole>,
) -> Result<()> {
    let song = manifest
        .scan
        .song(id)
        .context("selected entry is absent from this scan")?;
    let key = song.classification_key();
    // Merge with disk so a partial rescan cannot erase assignments for omitted entries.
    let mut saved = read(path, manifest)?;
    if let Some(role) = role {
        saved.entries.insert(key, role);
    } else {
        saved.entries.remove(&key);
    }
    validate(&saved)?;
    let bytes = serde_json::to_vec_pretty(&saved)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "audio classifications are too large"
    );
    std::fs::create_dir_all(path.parent().context("classification path has no parent")?)?;
    crate::platform::write_file_atomically_validated(path, &bytes, |file| {
        file.rewind()?;
        let _: SavedRoles = serde_json::from_reader(file)?;
        Ok(())
    })?;
    manifest.apply_roles(&saved.entries);
    Ok(())
}
