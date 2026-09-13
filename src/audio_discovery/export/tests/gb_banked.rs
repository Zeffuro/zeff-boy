use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::song::PcmSong, preview::PreviewRequest};

#[test]
fn banked_gb_routes_audio_to_native_and_preserves_midi_and_source_assets() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(
        System::Gb,
        zeff_audio_discovery::gb_music::native::fixture_rom(),
    );
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::Gb(1);
    let song = &manifest.scan.gb_songs[1];
    let selected = manifest.scan.song(id).unwrap();
    assert!(PcmSong::can_play(selected));
    assert!(PcmSong::is_native(selected));
    assert!(PreviewRequest::can_preview(&manifest, id));
    assert!(!PreviewRequest::can_preview(&manifest, SongId::Gb(0)));
    let directory = tempfile::tempdir()?;
    let midi_options = RenderOptions {
        loops: 2,
        max_seconds: 2,
        ..Default::default()
    };
    let expected = zeff_audio_discovery::gb_music::midi(&input.bytes, song, 2, 2, &cancel)?.bytes;
    let midi_path = directory.path().join("song.mid");
    let request =
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Midi, midi_options)?;
    assert!(matches!(&request, SongExportRequest::Gb(_)));
    request.write_new(&midi_path, &cancel, &AtomicU32::new(0))?;
    assert_eq!(std::fs::read(midi_path)?, expected);
    let asset_path = directory.path().join("source.zip");
    let request = SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::MappedAssets,
        midi_options,
    )?;
    assert!(matches!(&request, SongExportRequest::Gb(_)));
    request.write_new(&asset_path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(asset_path)?)?;
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["schema"], "zeff-gb-music-export/1");
    assert_eq!(metadata["song"], serde_json::to_value(song)?);
    for span in &song.mapped_spans {
        let mut data = Vec::new();
        archive
            .by_name(&format!(
                "source/{:08x}-{:08x}.bin",
                span.offset, span.byte_len
            ))?
            .read_to_end(&mut data)?;
        assert_eq!(
            data,
            input.bytes[span.offset as usize..(span.offset + span.byte_len) as usize]
        );
    }
    let options = RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    };
    let mut session =
        PcmSong::from_ref(selected)
            .unwrap()
            .session(&input.bytes, options, &cancel)?;
    let mut pcm = vec![0; session.duration_frames() * 2];
    assert_eq!(session.read(&mut pcm, &cancel)?, pcm.len());
    assert!(pcm.iter().any(|&sample| sample != 0));
    for format in [AudioFormat::Wav, AudioFormat::Flac] {
        assert!(selected.supports(SongFormat::Audio(format)));
        assert!(
            !manifest
                .scan
                .song(SongId::Gb(0))
                .unwrap()
                .supports(SongFormat::Audio(format))
        );
        let path = directory
            .path()
            .join(format!("song.{}", format.extension()));
        let request =
            SongExportRequest::prepare(&input, &manifest, id, SongFormat::Audio(format), options)?;
        assert!(matches!(&request, SongExportRequest::Pcm(_)));
        request.write_new(&path, &cancel, &AtomicU32::new(0))?;
        let decoded = if format == AudioFormat::Wav {
            hound::WavReader::open(path)?
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            claxon::FlacReader::open(path)?
                .samples()
                .map(|v| v.map(|v| v as i16))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        assert_eq!(decoded, pcm);
    }
    Ok(())
}
