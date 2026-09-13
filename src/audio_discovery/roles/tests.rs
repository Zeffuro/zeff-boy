use std::sync::{Arc, atomic::AtomicBool};

use super::*;
use crate::audio_discovery::{ScanLimits, media::ScanInput};
use zeff_emu_common::system::System;

fn manifest() -> ScanManifest {
    let input = ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: Arc::from(crate::audio_discovery::test_support::gba_fixture()),
        provenance: None,
        analysis_profile: "test",
        display_name: None,
    };
    input.analyze(ScanLimits::default(), &AtomicBool::new(false))
}

#[test]
fn manual_roles_survive_catalog_reordering_and_do_not_change_capabilities() {
    let mut manifest = manifest();
    let id = manifest.scan.song_ids().next().unwrap();
    let song = manifest.scan.song(id).unwrap();
    let key = song.classification_key();
    #[cfg(not(target_arch = "wasm32"))]
    let before = song.supports(crate::audio_discovery::formats::SongFormat::Midi);
    assert_eq!(song.classification().role, AudioRole::Unknown);
    manifest.apply_roles(&BTreeMap::from([(key.clone(), AudioRole::Fanfare)]));
    let song = manifest.scan.song(id).unwrap();
    assert_eq!(manifest.classification(song).role, AudioRole::Fanfare);
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(
        song.supports(crate::audio_discovery::formats::SongFormat::Midi),
        before
    );
    manifest
        .scan
        .candidates
        .insert(0, manifest.scan.candidates[0].clone());
    manifest.scan.candidates[0].header.effective_offset += 1;
    manifest.classifications = classify(&manifest.scan);
    manifest.apply_roles(&BTreeMap::from([(key, AudioRole::Fanfare)]));
    assert_eq!(
        manifest
            .classification(manifest.scan.song(SongId::Mp2k(1)).unwrap())
            .role,
        AudioRole::Fanfare
    );
    assert_eq!(
        manifest
            .classification(manifest.scan.song(SongId::Mp2k(0)).unwrap())
            .role,
        AudioRole::Unknown
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persisted_roles_are_identity_bound_merge_partial_scans_and_preserve_invalid_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("roles.json");
    let mut manifest = manifest();
    let id = manifest.scan.song_ids().next().unwrap();
    storage::save(&path, &mut manifest, id, Some(AudioRole::SoundEffect)).unwrap();
    let saved = storage::read(&path, &manifest).unwrap();
    assert_eq!(saved.entries.len(), 1);
    let mut other = manifest.clone();
    other.scan.media.system = "gb";
    assert!(storage::read(&path, &other).is_err());
    other.scan.media = manifest.scan.media.clone();
    other.scan.media.sha256 = Some("ff".repeat(32));
    assert!(storage::read(&path, &other).is_err());
    let key = manifest.scan.song(id).unwrap().classification_key();
    manifest.scan.candidates[0].header.effective_offset += 1;
    manifest.classifications = classify(&manifest.scan);
    storage::save(&path, &mut manifest, id, Some(AudioRole::Fanfare)).unwrap();
    assert!(
        storage::read(&path, &manifest)
            .unwrap()
            .entries
            .contains_key(&key)
    );
    storage::save(&path, &mut manifest, id, None).unwrap();
    assert_eq!(storage::read(&path, &manifest).unwrap().entries.len(), 1);
    std::fs::write(&path, b"invalid").unwrap();
    assert!(storage::save(&path, &mut manifest, id, Some(AudioRole::Music)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
}
