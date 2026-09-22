use super::*;
use crate::audio_discovery::{
    batch::BatchExportRequest, export::SongExportRequest, render::RenderOptions,
};
use std::io::Read;

#[test]
fn nsfe_single_and_bulk_exports_match_and_preserve_source_identity() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let progress = AtomicU32::new(0);
    let mut input = Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Nes),
        standalone_audio: None,
        bytes: zeff_audio_discovery::nes_native::fixture_rom().into(),
        provenance: None,
        analysis_profile: "nsfe-export-test",
        display_name: None,
    });
    let manifest = input.analyze(Default::default(), &cancel);
    let directory = tempfile::tempdir()?;
    let single = directory.path().join("selected.nsfe");
    let id = SongId::NesNative(0);
    let request = SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::Nsfe,
        RenderOptions::default(),
    )?;
    request.write_new(&single, &cancel, &progress)?;
    let selected = std::fs::read(&single)?;
    assert!(selected.starts_with(b"NSFE"));
    let request = || {
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::Nsfe,
            RenderOptions::default(),
        )
    };
    assert!(request()?.write_new(&single, &cancel, &progress).is_err());
    assert_eq!(std::fs::read(&single)?, selected);
    let cancelled = directory.path().join("cancelled.nsfe");
    assert!(
        request()?
            .write_new(&cancelled, &AtomicBool::new(true), &progress)
            .is_err()
    );
    assert!(!cancelled.exists());

    let bulk = directory.path().join("all.zip");
    let summary = BatchExportRequest::prepare(&input, &manifest, SongFormat::Nsfe, |_| {
        Ok(RenderOptions::default())
    })?
    .write_new(&bulk, &cancel, &progress)?;
    assert_eq!(summary.exported, manifest.scan.nes_native_songs.len());
    assert_eq!(summary.failed, 0);
    let expected: Vec<_> = manifest
        .scan
        .nes_native_songs
        .iter()
        .map(|song| {
            native_rips::encode_as(
                &input.bytes,
                SongRef::NesNative(song),
                native_rips::NativeRipFormat::Nsfe,
                &cancel,
            )
            .map(|rip| rip.bytes)
        })
        .collect::<Result<_>>()?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(bulk)?)?;
    let mut actual = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.name().ends_with(".nsfe") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            actual.push(bytes);
        }
    }
    assert_eq!(actual.len(), expected.len());
    assert!(actual.contains(&selected));
    for bytes in expected {
        assert!(actual.contains(&bytes));
    }
    Arc::make_mut(&mut Arc::get_mut(&mut input).unwrap().bytes)[16 + 0x6c4c] ^= 1;
    let stale = directory.path().join("stale.nsfe");
    assert!(
        SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::Nsfe,
            RenderOptions::default()
        )?
        .write_new(&stale, &cancel, &progress)
        .is_err()
    );
    assert!(!stale.exists());
    Ok(())
}
