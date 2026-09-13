use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::song::PcmSong};

#[test]
fn queue_nes_keeps_sequence_exports_and_adds_matching_lossless_recordings() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(
        System::Nes,
        zeff_audio_discovery::nes_music::native::fixture_rom(),
    );
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::Nes(8);
    let song = &manifest.scan.nes_songs[8];
    let options = RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    };
    let directory = tempfile::tempdir()?;
    let midi = SongExportRequest::prepare(&input, &manifest, id, SongFormat::Midi, options)?;
    assert!(matches!(&midi, SongExportRequest::Nes(_)));
    let path = directory.path().join("song.mid");
    midi.write_new(&path, &cancel, &AtomicU32::new(0))?;
    assert_eq!(
        std::fs::read(path)?,
        zeff_audio_discovery::nes_music::midi(&input.bytes, song, options.loops, 1, &cancel)?.bytes
    );
    let assets =
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::MappedAssets, options)?;
    assert!(matches!(assets, SongExportRequest::Nes(_)));
    let path = directory.path().join("source.zip");
    assets.write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["schema"], "zeff-nes-music-export/1");
    assert_eq!(metadata["song"], serde_json::to_value(song)?);
    let mut session = PcmSong::from_ref(manifest.scan.song(id).unwrap())
        .unwrap()
        .session(&input.bytes, options, &cancel)?;
    let mut expected = vec![0; session.duration_frames() * 2];
    assert_eq!(session.read(&mut expected, &cancel)?, expected.len());
    for format in [AudioFormat::Wav, AudioFormat::Flac] {
        let path = directory
            .path()
            .join(format!("song.{}", format.extension()));
        let request =
            SongExportRequest::prepare(&input, &manifest, id, SongFormat::Audio(format), options)?;
        assert!(matches!(&request, SongExportRequest::Pcm(_)));
        request.write_new(&path, &cancel, &AtomicU32::new(0))?;
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
    Ok(())
}
