use super::*;
use crate::audio_discovery::{
    ScanLimits,
    catalog::{SongId, SongRef},
    export::SongExportRequest,
    test_support::nes_music::{fixture as synthetic_fixture, synthetic_song},
};

#[test]
fn nes_catalog_uses_unique_selector_offsets_and_export_verifies_exact_rom_profile() -> Result<()> {
    let bytes = synthetic_fixture();
    let first = synthetic_song(&bytes, 1);
    let alias = synthetic_song(&bytes, 4);
    assert_eq!(first.header, alias.header);
    assert_ne!(first.table_entry, alias.table_entry);
    let mut input = ScanInput {
        cdda: None,
        system: Some(System::Nes),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "nes-test",
        display_name: None,
    };
    let mut manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert!(manifest.scan.nes_songs.is_empty());
    manifest.scan.nes_songs.extend([first.clone(), alias]);
    assert_eq!(
        manifest.scan.song_at_offset(first.table_entry.offset)?,
        SongId::Nes(0)
    );
    assert_eq!(
        manifest
            .scan
            .song_at_offset(manifest.scan.nes_songs[1].table_entry.offset)?,
        SongId::Nes(1)
    );
    for format in [SongFormat::Midi, SongFormat::MappedAssets] {
        assert!(SongRef::Nes(&first).supports(format));
        let request = SongExportRequest::prepare(
            &input,
            &manifest,
            SongId::Nes(0),
            format,
            RenderOptions::default(),
        )?;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("must-not-exist");
        assert!(
            request
                .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                .is_err()
        );
        assert!(!path.exists());
    }
    assert!(!SongRef::Nes(&first).supports(SongFormat::SoundFont));
    input.system = Some(System::Gb);
    assert!(
        SongExportRequest::prepare(
            &input,
            &manifest,
            SongId::Nes(0),
            SongFormat::MappedAssets,
            RenderOptions::default()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn nes_runtime_and_sweep_gates_leave_mapped_assets_available() {
    let mut bytes = synthetic_fixture();
    bytes[0x230..0x233].copy_from_slice(&[0, 0x18, 0x9a]);
    for index in [0, 6] {
        let song = synthetic_song(&bytes, index);
        assert!(!song.midi_exportable);
        assert!(!SongRef::Nes(&song).supports(SongFormat::Midi));
        assert!(SongRef::Nes(&song).supports(SongFormat::MappedAssets));
    }
}
