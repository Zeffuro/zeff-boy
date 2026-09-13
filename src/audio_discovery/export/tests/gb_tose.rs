use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::song::PcmSong, relations::GraphStatus};

#[test]
fn tose_graph_assets_and_recordings_preserve_the_revalidated_selector() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Gb, zeff_audio_discovery::gb_tose::synthetic_rom());
    let mut manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::GbTose(0);
    let selected = manifest.scan.song(id).unwrap();
    assert!(PcmSong::can_play(selected) && PcmSong::is_native(selected));
    assert_eq!(selected.span().unwrap().canonical_cpu_address, None);
    assert!(!selected.supports(SongFormat::Midi));
    assert!(!selected.supports(SongFormat::Gbs));
    assert_eq!(
        manifest
            .scan
            .asset_relations(id, Default::default(), &cancel)
            .status,
        GraphStatus::Complete
    );
    let directory = tempfile::tempdir()?;
    let options = RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    };
    let path = directory.path().join("assets.zip");
    SongExportRequest::prepare(&input, &manifest, id, SongFormat::MappedAssets, options)?
        .write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    for span in &manifest.scan.gb_tose_songs[0].mapped_spans {
        let name = format!(
            "source/{:08x}-{:08x}.bin",
            span.effective_offset, span.byte_len
        );
        let mut bytes = Vec::new();
        archive.by_name(&name)?.read_to_end(&mut bytes)?;
        assert_eq!(
            bytes,
            input.bytes
                [span.effective_offset as usize..(span.effective_offset + span.byte_len) as usize]
        );
    }
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["selection"]["engine"], "gb_tose");
    assert_eq!(
        metadata["selection"]["song"],
        serde_json::to_value(&manifest.scan.gb_tose_songs[0])?
    );
    let mut session =
        PcmSong::from_ref(selected)
            .unwrap()
            .session(&input.bytes, options, &cancel)?;
    let mut expected = vec![0; session.duration_frames() * 2];
    assert_eq!(session.read(&mut expected, &cancel)?, expected.len());
    for format in [AudioFormat::Wav, AudioFormat::Flac] {
        let path = directory
            .path()
            .join(format!("song.{}", format.extension()));
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Audio(format), options)?
            .write_new(&path, &cancel, &AtomicU32::new(0))?;
        let actual = if format == AudioFormat::Wav {
            hound::WavReader::open(path)?
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            claxon::FlacReader::open(path)?
                .samples()
                .map(|v| v.map(|v| v as i16))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        assert_eq!(actual, expected);
    }
    manifest.scan.gb_tose_songs[0].mapped_spans[0].effective_offset += 1;
    let path = directory.path().join("forged.zip");
    assert!(
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::MappedAssets, options)?
            .write_new(&path, &cancel, &AtomicU32::new(0))
            .is_err()
    );
    assert!(!path.exists());
    Ok(())
}
