use std::sync::atomic::AtomicBool;

use super::*;
use crate::audio_discovery::{
    ScanLimits, catalog::SongId, rips::RipFormat, test_support::rips::fixture,
};

fn native_format(format: RipFormat) -> SongFormat {
    match format {
        RipFormat::Gbs => SongFormat::Gbs,
        RipFormat::Nsf => SongFormat::Nsf,
    }
}

#[test]
fn direct_and_selected_zip_rips_preserve_identity_and_do_not_overwrite_exports()
-> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-rips")?;
    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let bytes = fixture(format);
        let extension = format.extension();
        let source = directory.path().join(format!("direct.{extension}"));
        let archive = directory.path().join(format!("{extension}.zip"));
        let member = format!("music/imported.{extension}");
        std::fs::write(&source, &bytes)?;
        crate::test_support::write_zip(
            &archive,
            &[
                (member.as_str(), bytes.as_slice()),
                ("other.bin", b"unselected"),
            ],
        )?;

        for zipped in [false, true] {
            let label = if zipped { "zip" } else { "direct" };
            let output = directory
                .path()
                .join(format!("{extension}-{label}.{extension}"));
            let request = AudioDiscoveryRequest {
                output_path: directory.path().join(format!("{extension}-{label}.json")),
                input_path: if zipped {
                    archive.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| member.clone()),
                max_work: None,
                max_candidates: None,
                relations: None,
                export: Some(OfflineExport {
                    format: native_format(format),
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions::default(),
                    explicit: ExplicitExportSettings::default(),
                }),
            };

            let input = load_input(&request)?;
            assert_eq!(input.system, None);
            assert_eq!(input.bytes.as_ref(), bytes);
            let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
            assert_eq!(manifest.scan.song_count(), 1);
            assert_eq!(
                manifest.scan.song_ids().collect::<Vec<_>>(),
                [SongId::Rip(0)]
            );
            let rip = &manifest.scan.music_rips[0];
            assert_eq!(rip.format, format);
            assert_eq!((rip.song_count, rip.first_song), (3, 2));
            assert_eq!(
                manifest.source.as_ref().unwrap().selected_member.is_some(),
                zipped
            );

            assert!(run_request(&request)?);
            assert_eq!(std::fs::read(&output)?, bytes);
            assert!(run_request(&request).is_err());
            assert_eq!(std::fs::read(&output)?, bytes);

            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&request.output_path)?)?;
            assert_eq!(report["scan"]["media"]["system"], format.system_id());
            assert_eq!(report["scan"]["music_rips"][0]["song_count"], 3);
            assert_eq!(report["scan"]["music_rips"][0]["first_song"], 2);
            if zipped {
                let source = manifest.source.as_ref().unwrap();
                assert_eq!(source.selected_member.as_ref().unwrap().name, member);
                assert_eq!(
                    source.container.as_ref().unwrap().sha256,
                    sha256_hex(&std::fs::read(&archive)?)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn rip_assets_export_retains_the_complete_original_source() -> anyhow::Result<()> {
    use std::io::Read;

    let directory = crate::test_support::test_directory("audio-cli-rip-assets")?;
    let bytes = fixture(RipFormat::Nsf);
    let source = directory.path().join("source.nsf");
    let output = directory.path().join("assets.zip");
    std::fs::write(&source, &bytes)?;
    let request = AudioDiscoveryRequest {
        output_path: directory.path().join("report.json"),
        input_path: source,
        archive_member: None,
        max_work: None,
        max_candidates: None,
        relations: None,
        export: Some(OfflineExport {
            format: SongFormat::MappedAssets,
            output_path: output.clone(),
            selection: SongSelection::Offset(0),
            options: RenderOptions::default(),
            explicit: ExplicitExportSettings::default(),
        }),
    };

    assert!(run_request(&request)?);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output)?)?;
    let mut preserved = Vec::new();
    archive.by_name("source.nsf")?.read_to_end(&mut preserved)?;
    assert_eq!(preserved, bytes);
    Ok(())
}

#[test]
fn rips_reject_wrong_native_format_and_render_settings() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-rip-rejections")?;
    let bytes = fixture(RipFormat::Gbs);
    let source = directory.path().join("source.gbs");
    std::fs::write(&source, &bytes)?;

    let request = |format, sample_rate_set, timing_set, mp2k_settings_set, label: &str| {
        AudioDiscoveryRequest {
            output_path: directory.path().join(format!("{label}.json")),
            input_path: source.clone(),
            archive_member: None,
            max_work: None,
            max_candidates: None,
            relations: None,
            export: Some(OfflineExport {
                format,
                output_path: directory.path().join(format!("{label}.out")),
                selection: SongSelection::Offset(0),
                options: RenderOptions::default(),
                explicit: ExplicitExportSettings {
                    sample_rate: sample_rate_set,
                    loops: timing_set,
                    gain: mp2k_settings_set,
                    ..Default::default()
                },
            }),
        }
    };

    for (label, format, sample_rate_set, timing_set, mp2k_settings_set) in [
        ("wrong", SongFormat::Nsf, false, false, false),
        ("sample-rate", SongFormat::Gbs, true, false, false),
        ("timing", SongFormat::Gbs, false, true, false),
        ("mp2k", SongFormat::Gbs, false, false, true),
    ] {
        let request = request(
            format,
            sample_rate_set,
            timing_set,
            mp2k_settings_set,
            label,
        );
        assert!(run_request(&request).is_err());
        assert!(!request.output_path.exists());
        assert!(!request.export.as_ref().unwrap().output_path.exists());
    }
    Ok(())
}

#[test]
fn cartridge_extensions_do_not_import_embedded_rip_headers() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-rip-cartridges")?;
    for (format, extension) in [(RipFormat::Gbs, "gb"), (RipFormat::Nsf, "nes")] {
        let source = directory.path().join(format!("looks-like-rip.{extension}"));
        std::fs::write(&source, fixture(format))?;
        let request = AudioDiscoveryRequest {
            output_path: directory.path().join(format!("{extension}.json")),
            input_path: source,
            archive_member: None,
            max_work: None,
            max_candidates: None,
            relations: None,
            export: None,
        };
        let input = load_input(&request)?;
        assert!(input.system.is_some());
        assert!(input.standalone_audio.is_none());
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert!(manifest.scan.music_rips.is_empty());
        assert!(
            !manifest
                .scan
                .song_ids()
                .any(|id| matches!(id, SongId::Rip(_)))
        );
    }
    Ok(())
}
