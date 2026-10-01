use std::io::Read;
use std::sync::{Arc, atomic::AtomicBool};

use super::*;
use crate::audio_discovery::{
    ScanLimits, catalog::SongId, formats::AudioFormat, preview::PreviewRequest,
    test_support::rips::nsf2_fixture,
};

#[test]
fn nsf2_direct_and_selected_zip_exports_preserve_features_and_chunks() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-nsf2")?;
    let bytes = nsf2_fixture();
    let source = directory.path().join("source.nsf");
    let archive_path = directory.path().join("source.zip");
    std::fs::write(&source, &bytes)?;
    crate::test_support::write_zip(
        &archive_path,
        &[("music/source.nsf", &bytes), ("other.bin", b"unselected")],
    )?;
    for zipped in [false, true] {
        let label = if zipped { "zip" } else { "direct" };
        for format in [SongFormat::Nsf, SongFormat::MappedAssets] {
            let output = directory
                .path()
                .join(format!("{label}-{}.out", format.info().id));
            let request = AudioDiscoveryRequest {
                output_path: directory
                    .path()
                    .join(format!("{label}-{}.json", format.info().id)),
                input_path: if zipped {
                    archive_path.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| "music/source.nsf".to_owned()),
                max_work: None,
                max_candidates: None,
                driver_evidence: None,
                relations: None,
                export: Some(OfflineExport {
                    format,
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions::default(),
                    explicit: ExplicitExportSettings::default(),
                }),
            };
            let input = Arc::new(load_input(&request)?);
            let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
            assert_eq!(manifest.scan.song_count(), 1);
            assert_eq!(manifest.scan.music_rips[0].version, Some(2));
            assert!(
                manifest.scan.music_rips[0]
                    .init
                    .unwrap()
                    .initial_source_offset
                    .is_none()
            );
            assert!(
                manifest.scan.music_rips[0]
                    .play
                    .unwrap()
                    .initial_source_offset
                    .is_none()
            );
            assert!(!PreviewRequest::can_preview(&manifest, SongId::Rip(0)));
            assert!(
                PreviewRequest::prepare_song(
                    &input,
                    &manifest,
                    SongId::Rip(0),
                    RenderOptions::default()
                )
                .is_err()
            );
            assert!(run_request(&request)?);
            let exported = std::fs::read(&output)?;
            if format == SongFormat::Nsf {
                assert_eq!(exported, bytes);
            } else {
                let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&exported))?;
                let mut original = Vec::new();
                zip.by_name("source.nsf")?.read_to_end(&mut original)?;
                assert_eq!(original, bytes);
                let metadata: serde_json::Value =
                    serde_json::from_reader(zip.by_name("manifest.json")?)?;
                assert_eq!(metadata["rip"]["version"], 2);
                assert_eq!(metadata["rip"]["details"]["kind"], "nsf2");
                assert_eq!(metadata["rip"]["details"]["raw_flags"], bytes[0x7c]);
                let mut rebuilt = Vec::new();
                for asset in metadata["assets"].as_array().unwrap() {
                    assert_eq!(
                        asset["span"]["offset"].as_u64().unwrap() as usize,
                        rebuilt.len()
                    );
                    let mut data = Vec::new();
                    zip.by_name(asset["path"].as_str().unwrap())?
                        .read_to_end(&mut data)?;
                    assert_eq!(
                        asset["span"]["byte_len"].as_u64().unwrap() as usize,
                        data.len()
                    );
                    assert_eq!(asset["sha256"], sha256_hex(&data));
                    rebuilt.extend(data);
                }
                assert_eq!(rebuilt, bytes);
                assert!(zip.by_name("program.bin").is_ok());
                assert!(zip.by_name("chunks/0000.bin").is_ok());
            }
            assert!(run_request(&request).is_err());
            assert_eq!(std::fs::read(output)?, exported);
        }
    }
    Ok(())
}

#[test]
fn nsf2_import_does_not_enable_audio_rendering_or_format_conversion() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-nsf2-refusal")?;
    let source = directory.path().join("source.nsf");
    std::fs::write(&source, nsf2_fixture())?;
    for format in [
        SongFormat::Audio(AudioFormat::Wav),
        SongFormat::Nsfe,
        SongFormat::Midi,
    ] {
        let output = directory.path().join(format!("{}.out", format.info().id));
        let request = AudioDiscoveryRequest {
            output_path: directory.path().join(format!("{}.json", format.info().id)),
            input_path: source.clone(),
            archive_member: None,
            max_work: None,
            max_candidates: None,
            driver_evidence: None,
            relations: None,
            export: Some(OfflineExport {
                format,
                output_path: output.clone(),
                selection: SongSelection::Offset(0),
                options: RenderOptions::default(),
                explicit: ExplicitExportSettings::default(),
            }),
        };
        assert!(run_request(&request).is_err());
        assert!(!output.exists());
        assert!(!request.output_path.exists());
    }
    Ok(())
}
