use super::*;
use serde_json::Value;

fn input(bytes: Vec<u8>) -> Arc<ScanInput> {
    Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "batch-test",
        display_name: Some("Batch fixture".into()),
    })
}

fn options(_: SongId) -> Result<RenderOptions> {
    Ok(RenderOptions {
        max_seconds: 1,
        fade_seconds: 0,
        ..Default::default()
    })
}

fn read_entry(pack: &mut zip::ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pack.by_name(name)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn batch_preserves_single_exports_reports_failures_and_skips_and_never_replaces() -> Result<()> {
    let module = super::super::test_support::tracker::mod_fixture();
    let mut bytes = vec![0xfe; 37];
    bytes.extend_from_slice(&module);
    bytes.extend_from_slice(&[0xfe; 37]);
    bytes.extend_from_slice(&super::super::test_support::tracker::xm_fixture());
    let input = input(bytes);
    let cancel = AtomicBool::new(false);
    let progress = AtomicU32::new(0);
    let mut manifest = input.analyze(Default::default(), &cancel);
    assert_eq!(manifest.scan.tracker_modules.len(), 2);
    let good_id = manifest.scan.song_at_offset(37)?;
    let mut broken = manifest.scan.tracker_modules[0].clone();
    broken.span.offset = u32::MAX;
    manifest.scan.tracker_modules.push(broken);
    let directory = tempfile::tempdir()?;
    let single = directory.path().join("single.mod");
    SongExportRequest::prepare(
        &input,
        &manifest,
        good_id,
        SongFormat::Mod,
        options(good_id)?,
    )?
    .write_new(&single, &cancel, &progress)?;
    let path = directory.path().join("all.zip");
    let request = || BatchExportRequest::prepare(&input, &manifest, SongFormat::Mod, options);
    let summary = request()?.write_new(&path, &cancel, &progress)?;
    assert_eq!(
        (summary.exported, summary.skipped, summary.failed),
        (1, 1, 1)
    );
    assert_eq!(progress.load(Ordering::Relaxed), 100);
    let original = std::fs::read(&path)?;
    let mut pack = zip::ZipArchive::new(File::open(&path)?)?;
    let report: Value = serde_json::from_slice(&read_entry(&mut pack, "batch-report.json")?)?;
    let catalog = read_entry(&mut pack, "scan-report.json")?;
    assert_eq!(
        serde_json::from_slice::<Value>(&catalog)?,
        serde_json::to_value(&manifest)?
    );
    assert_eq!(
        report["scan_report"]["sha256"],
        zeff_firmware::sha256_hex(&catalog)
    );
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["status"] == "exported")
        .unwrap();
    let name = entry["files"][0]["path"].as_str().unwrap();
    let bytes = read_entry(&mut pack, name)?;
    assert_eq!(bytes, std::fs::read(single)?);
    assert_eq!(
        entry["files"][0]["sha256"],
        zeff_firmware::sha256_hex(&bytes)
    );
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["status"] != "exported")
            .all(|entry| entry["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()))
    );
    assert!(request()?.write_new(&path, &cancel, &progress).is_err());
    assert_eq!(std::fs::read(&path)?, original);
    let cancelled = directory.path().join("cancelled.zip");
    assert!(
        request()?
            .write_new(&cancelled, &AtomicBool::new(true), &progress)
            .is_err()
    );
    assert!(!cancelled.exists());
    manifest.scan.media.sha256 = Some("changed".into());
    let stale = directory.path().join("stale.zip");
    assert!(
        BatchExportRequest::prepare(&input, &manifest, SongFormat::Mod, options)?
            .write_new(&stale, &cancel, &progress)
            .is_err()
    );
    assert!(!stale.exists());
    Ok(())
}

#[test]
fn mini_batch_keeps_library_bindings_deduplicates_and_distinguishes_banks() -> Result<()> {
    check_native_mini_batch(zeff_audio_discovery::gbass::fixture_rom_banked(), 4)
}

#[test]
fn mini_batch_keeps_library_bindings_and_distinguishes_modules() -> Result<()> {
    check_native_mini_batch(zeff_audio_discovery::gbass::fixture_rom_module(), 4)
}

#[test]
fn mini_batch_preserves_separate_pcm_instrument_layout() -> Result<()> {
    check_native_mini_batch(zeff_audio_discovery::gbass::fixture_rom_separate(), 2)
}

fn check_native_mini_batch(mut bytes: Vec<u8>, expected_songs: usize) -> Result<()> {
    bytes[0xB2] = 0x96;
    let input = input(bytes);
    let cancel = AtomicBool::new(false);
    let progress = AtomicU32::new(0);
    let manifest = input.analyze(Default::default(), &cancel);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("all.zip");
    let summary = BatchExportRequest::prepare(&input, &manifest, SongFormat::MiniGsfPack, options)?
        .write_new(&path, &cancel, &progress)?;
    assert_eq!((summary.exported, summary.failed), (expected_songs, 0));
    let mut batch = zip::ZipArchive::new(File::open(path)?)?;
    let report: Value = serde_json::from_slice(&read_entry(&mut batch, "batch-report.json")?)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut libraries = 0;
    for (ordinal, entry) in report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["status"] == "exported")
        .enumerate()
    {
        let id: SongId = serde_json::from_value(entry["song"].clone())?;
        let single = directory.path().join(format!("single-{ordinal}.zip"));
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::MiniGsfPack, options(id)?)?
            .write_new(&single, &cancel, &progress)?;
        let mut original = zip::ZipArchive::new(File::open(single)?)?;
        for file in entry["files"].as_array().unwrap() {
            let name = file["path"].as_str().unwrap();
            let original_name = file["original_entry"].as_str().unwrap();
            assert_eq!(
                read_entry(&mut batch, name)?,
                read_entry(&mut original, original_name)?
            );
            if name.ends_with(".gsflib") {
                libraries += 1;
                assert_eq!(name.rsplit('/').next().unwrap(), original_name);
            } else {
                assert!(
                    seen.insert(name.to_owned()),
                    "duplicate song or manifest name"
                );
            }
        }
    }
    let unique_libraries = batch
        .file_names()
        .filter(|name| name.ends_with(".gsflib"))
        .count();
    assert!(unique_libraries < libraries);
    assert_eq!(
        batch
            .file_names()
            .filter(|name| name.ends_with(".minigsf"))
            .count(),
        expected_songs
    );
    Ok(())
}

#[test]
fn cancellation_during_streaming_discards_the_entire_archive() -> Result<()> {
    struct CancellingReader<'a> {
        inner: std::io::Cursor<Vec<u8>>,
        cancel: &'a AtomicBool,
    }
    impl Read for CancellingReader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(bytes)?;
            self.cancel.store(true, Ordering::Relaxed);
            Ok(count)
        }
    }
    impl Seek for CancellingReader<'_> {
        fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cancel.zip");
    let cancel = AtomicBool::new(false);
    let result = crate::platform::write_new_file_atomically_streamed(
        &path,
        |file| {
            let mut archive = Archive::new(file);
            archive.add_bytes("complete.txt", b"finished first song", &cancel)?;
            let mut reader = CancellingReader {
                inner: std::io::Cursor::new(vec![0; 128 * 1024]),
                cancel: &cancel,
            };
            archive.add_reader("cancelled.wav", &mut reader, 128 * 1024, &cancel)?;
            archive.finish()
        },
        || check_cancel(&cancel),
    );
    assert!(result.is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(())
}
