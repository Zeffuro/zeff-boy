use super::*;
use crate::audio_discovery::{ScanLimits, vgm_export::tests::fixture};
use std::io::Write;

#[test]
fn qualified_vgm_cli_renders_raw_gzip_and_selected_zip_audio() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-vgm-playback")?;
    let mut raw = fixture(false);
    raw.truncate(u32::from_le_bytes(raw[4..8].try_into()?) as usize + 4);
    raw[8..12].copy_from_slice(&0x171u32.to_le_bytes());
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&raw)?;
    let gzip = gzip.finish()?;
    let mut expected = None;
    for (index, bytes) in [&raw, &gzip].into_iter().enumerate() {
        let source = directory.path().join(format!("source-{index}.vgm"));
        std::fs::write(&source, bytes)?;
        let archive = directory.path().join(format!("source-{index}.zip"));
        crate::test_support::write_zip(
            &archive,
            &[("capture.vgm", bytes), ("other.vgm", b"invalid")],
        )?;
        for zipped in [false, true] {
            let output = directory
                .path()
                .join(format!("output-{index}-{zipped}.wav"));
            let request = AudioDiscoveryRequest {
                output_path: directory.path().join(format!("scan-{index}-{zipped}.json")),
                input_path: if zipped {
                    archive.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| "capture.vgm".into()),
                max_work: None,
                max_candidates: None,
                driver_evidence: None,
                relations: None,
                export: Some(OfflineExport {
                    format: SongFormat::Audio(crate::audio_discovery::formats::AudioFormat::Wav),
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions {
                        sample_rate: 44_100,
                        max_seconds: 1,
                        ..RenderOptions::default()
                    },
                    explicit: ExplicitExportSettings {
                        sample_rate: true,
                        max_seconds: true,
                        ..ExplicitExportSettings::default()
                    },
                }),
            };
            assert!(run_request(&request)?);
            let mut wav = hound::WavReader::open(&output)?;
            assert_eq!(wav.duration(), 735);
            assert_eq!(wav.spec().sample_rate, 44_100);
            let pcm = wav
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert!(pcm.iter().any(|sample| *sample != 0));
            if let Some(expected) = &expected {
                assert_eq!(&pcm, expected);
            } else {
                expected = Some(pcm);
            }
            let mut export = request.export.unwrap();
            export.explicit.loops = true;
            assert!(export.options_for(SongId::Vgm(0)).is_err());
            export.explicit.loops = false;
            export.format = SongFormat::Vgm;
            assert!(export.options_for(SongId::Vgm(0)).is_err());
        }
    }
    Ok(())
}

#[test]
fn vgm_and_vgz_direct_and_selected_zip_inputs_keep_source_identity() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-vgm")?;
    for (extension, gzip) in [("vgm", false), ("vgz", true), ("vgm", true), ("vgz", false)] {
        let bytes = fixture(gzip);
        let label = format!("{extension}-{gzip}");
        let source = directory.path().join(format!("{label}.{extension}"));
        let archive = directory.path().join(format!("{label}.zip"));
        let member = format!("logs/{label}.{extension}");
        std::fs::write(&source, &bytes)?;
        crate::test_support::write_zip(
            &archive,
            &[(member.as_str(), &bytes), ("unselected.vgm", b"invalid")],
        )?;
        assert!(normalize_archive_member(&member).is_ok());
        for zipped in [false, true] {
            let output = directory.path().join(format!("{label}-{zipped}.vgm"));
            let request = AudioDiscoveryRequest {
                output_path: directory.path().join(format!("{label}-{zipped}.json")),
                input_path: if zipped {
                    archive.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| member.clone()),
                max_work: None,
                max_candidates: None,
                driver_evidence: None,
                relations: None,
                export: Some(OfflineExport {
                    format: SongFormat::Vgm,
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions::default(),
                    explicit: ExplicitExportSettings::default(),
                }),
            };
            let input = load_input(&request)?;
            assert_eq!(input.bytes.as_ref(), bytes);
            let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
            assert_eq!(manifest.scan.song_count(), 1);
            assert_eq!(
                manifest.scan.media.sha256.as_deref(),
                Some(sha256_hex(&bytes).as_str())
            );
            assert_eq!(
                manifest.source.as_ref().unwrap().selected_member.is_some(),
                zipped
            );
            assert!(run_request(&request)?);
            assert_eq!(std::fs::read(output)?, fixture(false));
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&request.output_path)?)?;
            assert_eq!(report["scan"]["media"]["system"], "standalone_vgm");
            assert_eq!(
                report["scan"]["vgm_logs"][0]["logical"]["address_space"],
                if gzip {
                    "decompressed_vgm"
                } else {
                    "source_file"
                }
            );
            if zipped {
                assert_eq!(
                    report["source"]["container"]["sha256"],
                    sha256_hex(&std::fs::read(&archive)?)
                );
                assert_eq!(report["source"]["selected_member"]["name"], member);
            }
        }
    }
    Ok(())
}
