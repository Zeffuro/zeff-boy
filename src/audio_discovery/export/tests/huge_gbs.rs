use super::*;
use zeff_audio_discovery::{coverage, huge::gbs, rips};

#[test]
fn huge_gbs_export_validates_before_atomic_publication() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Gb, zeff_audio_discovery::huge::fixture::rom());
    let manifest = input.analyze(Default::default(), &cancel);
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("music.gbs");
    let progress = AtomicU32::new(0);
    SongExportRequest::prepare(
        &input,
        &manifest,
        SongId::Huge(0),
        SongFormat::Gbs,
        RenderOptions::default(),
    )?
    .write_new(&output, &cancel, &progress)?;
    assert_eq!(progress.load(Ordering::Relaxed), 100);
    let bytes = std::fs::read(&output)?;
    assert_eq!(
        bytes,
        gbs::build(&input.bytes, &manifest.scan.huge_songs[0], &cancel)?.bytes
    );
    let rip = rips::inspect(&bytes, rips::RipFormat::Gbs, Default::default(), &cancel)
        .map_err(|stop| anyhow::anyhow!("GBS inspection stopped: {stop:?}"))?
        .context("exported GBS was not recognized")?;
    assert_eq!(rip.song_count, 1);
    assert!(
        manifest
            .scan
            .song(SongId::Huge(0))
            .unwrap()
            .requires_runtime_validation()
    );
    let coverage = coverage::observe(&manifest.scan);
    assert_eq!(coverage.pending_runtime_entries, 1);
    assert_eq!(coverage.render_supported_entries, 0);
    SongExportRequest::prepare(
        &input,
        &manifest,
        SongId::Huge(0),
        SongFormat::Gbs,
        RenderOptions::default(),
    )?
    .write_new(&output, &cancel, &AtomicU32::new(0))
    .unwrap_err();
    assert_eq!(std::fs::read(output)?, bytes);
    let batch_path = directory.path().join("songs.zip");
    let summary = crate::audio_discovery::batch::BatchExportRequest::prepare(
        &Arc::new(input),
        &manifest,
        SongFormat::Gbs,
        |_| Ok(RenderOptions::default()),
    )?
    .write_new(&batch_path, &cancel, &AtomicU32::new(0))?;
    assert_eq!(
        (summary.exported, summary.failed, summary.skipped),
        (1, 0, 0)
    );
    let mut zip = zip::ZipArchive::new(std::fs::File::open(batch_path)?)?;
    let filename = zip
        .file_names()
        .find(|name| name.ends_with(".gbs"))
        .unwrap()
        .to_owned();
    let mut extracted = Vec::new();
    zip.by_name(&filename)?.read_to_end(&mut extracted)?;
    assert_eq!(extracted, bytes);
    Ok(())
}

#[test]
fn huge_gbs_export_rejects_cancelled_and_forged_selections_without_files() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Gb, zeff_audio_discovery::huge::fixture::rom());
    let directory = tempfile::tempdir()?;
    for mutation in 0..5 {
        let mut manifest = input.analyze(Default::default(), &cancel);
        let song = &mut manifest.scan.huge_songs[0];
        match mutation {
            0 => (),
            1 => song.bound.evidence.ram_address += 1,
            2 => song.validation_frames = u32::MAX,
            3 => song.bound.song.descriptor.offset += 1,
            4 => song.source_sha256 = "stale".into(),
            _ => unreachable!(),
        }
        let output = directory.path().join(format!("rejected-{mutation}.gbs"));
        let progress = AtomicU32::new(0);
        let result = SongExportRequest::prepare(
            &input,
            &manifest,
            SongId::Huge(0),
            SongFormat::Gbs,
            RenderOptions::default(),
        )
        .and_then(|request| request.write_new(&output, &AtomicBool::new(mutation == 0), &progress));
        assert!(result.is_err(), "mutation {mutation}");
        assert!(!output.exists());
        assert_eq!(progress.load(Ordering::Relaxed), 0);
    }
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(())
}

#[test]
fn huge_gbs_export_rejects_media_and_manifest_identity_mismatches() -> Result<()> {
    let mut input = input(System::Gb, zeff_audio_discovery::huge::fixture::rom());
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    Arc::make_mut(&mut input.bytes)[0x7000] ^= 1;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("stale.gbs");
    SongExportRequest::prepare(
        &input,
        &manifest,
        SongId::Huge(0),
        SongFormat::Gbs,
        RenderOptions::default(),
    )?
    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
    .unwrap_err();
    assert!(!path.exists());
    Ok(())
}
