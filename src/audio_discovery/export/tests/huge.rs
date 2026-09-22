use super::*;
use crate::audio_discovery::formats::AudioFormat;

#[test]
fn huge_audio_exports_share_verified_pcm_and_reject_stale_selections() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Gb, zeff_audio_discovery::huge::fixture::rom());
    let mut manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::Huge(0);
    let options = RenderOptions {
        max_seconds: 1,
        ..Default::default()
    };
    let directory = tempfile::tempdir()?;
    let mut expected = Vec::new();
    for format in AudioFormat::ALL {
        if format == AudioFormat::Ogg && !cfg!(feature = "audio-recording") {
            continue;
        }
        let path = directory
            .path()
            .join(format!("song.{}", format.extension()));
        SongExportRequest::prepare(&input, &manifest, id, SongFormat::Audio(format), options)?
            .write_new(&path, &cancel, &AtomicU32::new(0))?;
        match format {
            AudioFormat::Wav => {
                expected = hound::WavReader::open(&path)?
                    .samples::<i16>()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                assert_eq!(expected.len(), 96_000);
                let bytes = std::fs::read(&path)?;
                let offset = bytes.windows(4).position(|v| v == b"ICMT").unwrap();
                let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
                let metadata: Value =
                    serde_json::from_slice(&bytes[offset + 8..offset + 8 + size - 1])?;
                assert_eq!(
                    metadata["runtime_validation"]["original"]["recurrence"]["passed"],
                    true
                );
                assert_eq!(metadata["runtime_validation"]["sample_rate"], 48_000);
                assert_eq!(metadata["frames"], 48_000);
            }
            AudioFormat::Flac => {
                let pcm = claxon::FlacReader::open(&path)?
                    .samples()
                    .map(|v| v.map(|v| v as i16))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                assert_eq!(pcm, expected);
            }
            AudioFormat::Ogg => {
                let mut reader =
                    lewton::inside_ogg::OggStreamReader::new(std::fs::File::open(&path)?)?;
                assert_eq!(reader.ident_hdr.audio_sample_rate, 48_000);
                let mut samples = Vec::new();
                while let Some(packet) = reader.read_dec_packet_itl()? {
                    samples.extend(packet);
                }
                assert_eq!(samples.len(), expected.len());
                assert!(samples.iter().any(|v| *v != 0));
                assert!(
                    reader
                        .comment_hdr
                        .comment_list
                        .iter()
                        .any(|(key, value)| key == "ZEFF_METADATA"
                            && value.contains("runtime_validation"))
                );
            }
        }
    }
    let existing = directory.path().join("existing.wav");
    std::fs::write(&existing, b"preserve")?;
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::Audio(AudioFormat::Wav),
        options,
    )?
    .write_new(&existing, &cancel, &AtomicU32::new(0))
    .unwrap_err();
    assert_eq!(std::fs::read(&existing)?, b"preserve");
    assert!(
        manifest
            .scan
            .song(id)
            .unwrap()
            .requires_runtime_validation()
    );
    for format in [SongFormat::Midi, SongFormat::MappedAssets] {
        assert!(SongExportRequest::prepare(&input, &manifest, id, format, options).is_err());
    }
    let path = directory.path().join("rejected.wav");
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::Audio(AudioFormat::Wav),
        options,
    )?
    .write_new(&path, &AtomicBool::new(true), &AtomicU32::new(0))
    .unwrap_err();
    assert!(!path.exists());
    manifest.scan.huge_songs[0].bound.evidence.ram_address += 1;
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::Audio(AudioFormat::Wav),
        options,
    )?
    .write_new(&path, &cancel, &AtomicU32::new(0))
    .unwrap_err();
    assert!(!path.exists());
    Ok(())
}
