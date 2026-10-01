use std::io::Read;
use std::sync::{Arc, atomic::AtomicBool};

use super::*;
use crate::audio_discovery::{
    formats::AudioFormat, preview::PreviewRequest, rips::RipFormat, test_support::rips::fixture,
};

#[test]
fn hes_direct_and_selected_zip_imports_preserve_an_unenumerated_container() -> anyhow::Result<()> {
    check_imports(RipFormat::Hes, SongFormat::Hes)
}

#[test]
fn wsr_direct_and_selected_zip_imports_preserve_an_unenumerated_container() -> anyhow::Result<()> {
    check_imports(RipFormat::Wsr, SongFormat::Wsr)
}

fn check_imports(rip_format: RipFormat, source_format: SongFormat) -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-unenumerated")?;
    let bytes = fixture(rip_format);
    let source = directory
        .path()
        .join(format!("source.{}", rip_format.extension()));
    let member = format!("music/source.{}", rip_format.extension());
    let zip_path = directory.path().join("source.zip");
    std::fs::write(&source, &bytes)?;
    crate::test_support::write_zip(&zip_path, &[(&member, &bytes)])?;
    for zipped in [false, true] {
        let label = if zipped { "zip" } else { "direct" };
        for format in [source_format, SongFormat::MappedAssets] {
            let output = directory
                .path()
                .join(format!("{label}-{}.out", format.info().id));
            let request = AudioDiscoveryRequest {
                output_path: directory
                    .path()
                    .join(format!("{label}-{}.json", format.info().id)),
                input_path: if zipped {
                    zip_path.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| member.clone()),
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
            let rip = &manifest.scan.music_rips[0];
            assert_eq!(
                (rip.song_count, rip.first_song, rip.load_address, rip.play),
                (None, None, None, None)
            );
            if rip_format == RipFormat::Hes {
                assert!(rip.init.unwrap().initial_source_offset.is_none());
            } else {
                assert!(rip.init.is_none());
                assert!(rip.version.is_none());
            }
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
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&request.output_path)?)?;
            let details = &report["scan"]["music_rips"][0];
            assert_eq!(details["details"]["raw_start_song"], 255);
            for key in ["song_count", "first_song", "load_address", "play"] {
                assert!(details[key].is_null());
            }
            let exported = std::fs::read(&output)?;
            if format == source_format {
                assert_eq!(exported, bytes);
            } else {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&exported))?;
                let mut preserved = Vec::new();
                archive
                    .by_name(&format!("source.{}", rip_format.extension()))?
                    .read_to_end(&mut preserved)?;
                assert_eq!(preserved, bytes);
                let metadata: serde_json::Value =
                    serde_json::from_reader(archive.by_name("manifest.json")?)?;
                assert_eq!(metadata["assets"].as_array().unwrap().len(), 2);
                let mut rebuilt = Vec::new();
                for asset in metadata["assets"].as_array().unwrap() {
                    assert_eq!(
                        asset["span"]["offset"].as_u64().unwrap() as usize,
                        rebuilt.len()
                    );
                    let mut data = Vec::new();
                    archive
                        .by_name(asset["path"].as_str().unwrap())?
                        .read_to_end(&mut data)?;
                    assert_eq!(asset["sha256"], sha256_hex(&data));
                    rebuilt.extend(data);
                }
                assert_eq!(rebuilt, bytes);
            }
            assert!(run_request(&request).is_err());
            assert_eq!(std::fs::read(output)?, exported);
        }
    }
    Ok(())
}

#[test]
fn hes_header_declarations_do_not_enable_playback_or_conversion() -> anyhow::Result<()> {
    check_refusals(RipFormat::Hes)
}

#[test]
fn wsr_trailer_declarations_do_not_enable_playback_or_conversion() -> anyhow::Result<()> {
    check_refusals(RipFormat::Wsr)
}

fn check_refusals(rip_format: RipFormat) -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-unenumerated-refusal")?;
    let source = directory
        .path()
        .join(format!("source.{}", rip_format.extension()));
    std::fs::write(&source, fixture(rip_format))?;
    for format in [
        SongFormat::Audio(AudioFormat::Wav),
        SongFormat::Nsf,
        SongFormat::Midi,
        SongFormat::SoundFont,
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
