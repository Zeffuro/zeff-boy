use std::collections::BTreeMap;

use serde::Serialize;
use zeff_audio_discovery::classification::{AudioClassification, AudioRole};

use super::{
    ScanReport,
    catalog::{SongId, SongRef},
    media::ScanManifest,
};

#[cfg(not(target_arch = "wasm32"))]
mod storage;

pub(crate) type Classifications = BTreeMap<String, EntryClassification>;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EntryClassification {
    pub(crate) selection: SongId,
    pub(crate) detected: AudioClassification,
    pub(crate) current: AudioClassification,
}

pub(crate) fn classify(report: &ScanReport) -> Classifications {
    report
        .song_ids()
        .map(|id| {
            let song = report.song(id).expect("catalog entry");
            let detected = song.classification();
            (
                song.classification_key(),
                EntryClassification {
                    selection: id,
                    current: detected.clone(),
                    detected,
                },
            )
        })
        .collect()
}

impl ScanManifest {
    pub(crate) fn classification(&self, song: SongRef<'_>) -> AudioClassification {
        self.classifications
            .get(&song.classification_key())
            .map_or_else(|| song.classification(), |entry| entry.current.clone())
    }

    fn apply_roles(&mut self, roles: &BTreeMap<String, AudioRole>) {
        for (key, entry) in &mut self.classifications {
            entry.current = roles.get(key).map_or_else(
                || entry.detected.clone(),
                |&role| AudioClassification::assigned(role),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_roles(&mut self) -> anyhow::Result<()> {
        self.classifications_loaded = true;
        if self.scan.media.sha256.is_none() {
            return Ok(());
        }
        let path = storage::path(self)?;
        let saved = storage::read(&path, self)?;
        self.apply_roles(&saved.entries);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn save_role(&mut self, id: SongId, role: Option<AudioRole>) -> anyhow::Result<()> {
        let path = storage::path(self)?;
        storage::save(&path, self, id, role)
    }
}

#[cfg(test)]
mod tests;
