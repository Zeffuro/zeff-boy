use std::sync::atomic::AtomicBool;

use super::*;
use zeff_emu_common::system::System;
#[test]
fn embedded_modules_export_from_non_gba_cartridges_without_fake_addresses() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-tracker")?;
    for (format, module, extension) in [
        (
            SongFormat::Xm,
            crate::audio_discovery::test_support::tracker::xm_fixture(),
            "sms",
        ),
        (
            SongFormat::Mod,
            crate::audio_discovery::test_support::tracker::mod_fixture(),
            "ws",
        ),
    ] {
        let input_path = directory.path().join(format!("source.{extension}"));
        let mut source = vec![0xFF; 73];
        source.extend_from_slice(&module);
        source.extend_from_slice(&[0xA5; 37]);
        std::fs::write(&input_path, &source)?;
        let output_path = directory
            .path()
            .join(format!("song.{}", format.info().extension));
        let request = AudioDiscoveryRequest {
            output_path: directory.path().join(format!("{extension}-report.json")),
            input_path: input_path.clone(),
            archive_member: None,
            max_work: None,
            max_candidates: None,
            driver_evidence: None,
            relations: None,
            export: Some(OfflineExport {
                format,
                output_path: output_path.clone(),
                selection: SongSelection::Offset(73),
                options: RenderOptions::default(),
                explicit: ExplicitExportSettings::default(),
            }),
        };
        let input = load_input(&request)?;
        assert_eq!(input.system, Some(System::from_path(&input_path).unwrap()));
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        let id = manifest.scan.song_at_offset(73)?;
        assert!(matches!(id, SongId::Module(_)));
        assert_eq!(
            manifest
                .scan
                .song(id)
                .unwrap()
                .span()
                .unwrap()
                .canonical_cpu_address,
            None
        );
        run_request(&request)?;
        assert_eq!(std::fs::read(output_path)?, module);
        assert_eq!(std::fs::read(input_path)?, source);
    }
    Ok(())
}

#[test]
fn standalone_modules_require_the_selected_type_at_offset_zero_and_preserve_full_source()
-> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-standalone-module")?;
    let mut source = crate::audio_discovery::test_support::tracker::xm_fixture();
    source.extend_from_slice(&[0xD0, 0x0D, 0xFA, 0xCE]);
    let input_path = directory.path().join("fixture.xm");
    let report_path = directory.path().join("report.json");
    let output_path = directory.path().join("export.xm");
    std::fs::write(&input_path, &source)?;
    let request = AudioDiscoveryRequest {
        output_path: report_path.clone(),
        input_path: input_path.clone(),
        archive_member: None,
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: Some(OfflineExport {
            format: SongFormat::Xm,
            output_path: output_path.clone(),
            selection: SongSelection::Offset(0),
            options: RenderOptions::default(),
            explicit: ExplicitExportSettings::default(),
        }),
    };
    let input = load_input(&request)?;
    assert_eq!(input.system, None);
    assert_eq!(input.display_name.as_deref(), Some("fixture"));
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert_eq!(
        serde_json::to_value(&manifest.scan.media)?["system"],
        "standalone_tracker"
    );
    assert_eq!(
        manifest.scan.media.sha256.as_deref(),
        Some(sha256_hex(&source).as_str())
    );
    let module = &manifest.scan.tracker_modules[0];
    assert_eq!(module.span.offset, 0);
    assert_eq!(module.span.byte_len as usize + 4, source.len());
    assert_eq!(
        module.source,
        crate::audio_discovery::tracker::ModuleSource::Standalone { trailing_bytes: 4 }
    );
    let id = manifest.scan.song_at_offset(0)?;
    assert_eq!(
        manifest
            .scan
            .song(id)
            .unwrap()
            .span()
            .unwrap()
            .canonical_cpu_address,
        None
    );
    run_request(&request)?;
    assert_eq!(std::fs::read(&output_path)?, source);
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report_path)?)?;
    assert_eq!(report["source"]["sha256"], sha256_hex(&source));
    assert_eq!(report["display_name"], "fixture");
    assert_eq!(report["scan"]["media"]["system"], "standalone_tracker");
    assert_eq!(
        report["scan"]["tracker_modules"][0]["source"]["kind"],
        "standalone"
    );
    Ok(())
}

#[test]
fn standalone_zip_requires_an_explicit_matching_member_and_rejects_malformed_or_wrong_type()
-> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-standalone-module-zip")?;
    let archive_path = directory.path().join("modules.zip");
    let module = crate::audio_discovery::test_support::tracker::mod_fixture();
    crate::test_support::write_zip(
        &archive_path,
        &[
            ("music/fixture.mod", module.as_slice()),
            ("note.txt", b"ignore"),
        ],
    )?;
    let request = AudioDiscoveryRequest {
        output_path: directory.path().join("report.json"),
        input_path: archive_path.clone(),
        archive_member: Some("music/fixture.mod".to_owned()),
        max_work: Some(40_000_000),
        max_candidates: Some(1),
        driver_evidence: None,
        relations: None,
        export: None,
    };
    let input = load_input(&request)?;
    assert_eq!(input.system, None);
    assert_eq!(input.display_name.as_deref(), Some("fixture"));
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert_eq!(
        manifest.scan.status,
        crate::audio_discovery::ScanStatus::Complete
    );
    assert_eq!(manifest.scan.tracker_modules.len(), 1);
    assert_eq!(manifest.source.as_ref().unwrap().kind, "zip_mod_member");
    assert_eq!(
        manifest
            .source
            .as_ref()
            .unwrap()
            .selected_member
            .as_ref()
            .unwrap()
            .name,
        "music/fixture.mod"
    );

    assert!(normalize_archive_member("music/fixture.txt").is_err());
    let wrong_type = directory.path().join("wrong.xm");
    std::fs::write(&wrong_type, &module)?;
    let wrong = load_input(&AudioDiscoveryRequest {
        output_path: directory.path().join("wrong.json"),
        input_path: wrong_type,
        archive_member: None,
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: None,
    })?;
    assert_eq!(
        wrong
            .analyze(ScanLimits::default(), &AtomicBool::new(false))
            .scan
            .status,
        crate::audio_discovery::ScanStatus::Unsupported
    );
    let malformed = directory.path().join("broken.xm");
    std::fs::write(&malformed, b"Extended Module: broken")?;
    let malformed = load_input(&AudioDiscoveryRequest {
        output_path: directory.path().join("broken.json"),
        input_path: malformed,
        archive_member: None,
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: None,
    })?;
    assert_eq!(
        malformed
            .analyze(ScanLimits::default(), &AtomicBool::new(false))
            .scan
            .status,
        crate::audio_discovery::ScanStatus::Unsupported
    );
    Ok(())
}

#[test]
fn s3m_and_it_direct_and_selected_zip_exports_preserve_complete_sources() -> anyhow::Result<()> {
    use crate::audio_discovery::{
        test_support::tracker as tests,
        tracker::{EmbeddedFormat, ModuleSource},
    };
    let directory = crate::test_support::test_directory("audio-cli-s3m-it")?;
    for (format, native, mut bytes) in [
        (EmbeddedFormat::S3m, SongFormat::S3m, tests::s3m_fixture()),
        (EmbeddedFormat::It, SongFormat::It, tests::it_fixture()),
    ] {
        let extent = bytes.len();
        bytes.extend_from_slice(b"unrecognized tracker extension");
        let extension = format.extension();
        let source_path = directory.path().join(format!("fixture.{extension}"));
        let archive_path = directory.path().join(format!("{extension}.zip"));
        let member = format!("music/fixture.{extension}");
        std::fs::write(&source_path, &bytes)?;
        crate::test_support::write_zip(
            &archive_path,
            &[
                (member.as_str(), &bytes),
                ("unselected.bin", b"leave alone"),
            ],
        )?;
        assert!(normalize_archive_member(&member).is_ok());
        assert_eq!(
            standalone_format(std::path::Path::new(&format!(
                "FIXTURE.{}",
                extension.to_uppercase()
            ))),
            Some(format)
        );
        for zipped in [false, true] {
            let label = format!("{extension}-{}", if zipped { "zip" } else { "direct" });
            let output = directory.path().join(format!("{label}.{extension}"));
            let request = AudioDiscoveryRequest {
                output_path: directory.path().join(format!("{label}.json")),
                input_path: if zipped {
                    archive_path.clone()
                } else {
                    source_path.clone()
                },
                archive_member: zipped.then(|| member.clone()),
                max_work: None,
                max_candidates: None,
                driver_evidence: None,
                relations: None,
                export: Some(OfflineExport {
                    format: native,
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions::default(),
                    explicit: ExplicitExportSettings::default(),
                }),
            };
            let input = load_input(&request)?;
            let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
            assert_eq!(manifest.scan.song_count(), 1);
            assert_eq!(input.system, None);
            let module = &manifest.scan.tracker_modules[0];
            assert_eq!(module.format, format);
            assert_eq!(module.span.byte_len as usize, extent);
            assert_eq!(
                module.source,
                ModuleSource::Standalone {
                    trailing_bytes: (bytes.len() - extent) as u32,
                }
            );
            assert_eq!(manifest.source.as_ref().unwrap().sha256, sha256_hex(&bytes));
            if zipped {
                let source = manifest.source.as_ref().unwrap();
                assert_eq!(source.selected_member.as_ref().unwrap().name, member);
                assert_eq!(
                    source.container.as_ref().unwrap().sha256,
                    sha256_hex(&std::fs::read(&archive_path)?)
                );
            }
            run_request(&request)?;
            assert_eq!(std::fs::read(&output)?, bytes);
            assert!(run_request(&request).is_err());
            assert_eq!(std::fs::read(&output)?, bytes);
        }
        assert_eq!(std::fs::read(source_path)?, bytes);
        let mismatched = AudioDiscoveryRequest {
            output_path: directory.path().join(format!("{extension}-wrong.json")),
            input_path: archive_path,
            archive_member: None,
            max_work: None,
            max_candidates: None,
            driver_evidence: None,
            relations: None,
            export: None,
        };
        assert!(load_input(&mismatched).is_err());
    }
    Ok(())
}

fn cd_fixture() -> (Vec<u8>, Vec<u8>, &'static str) {
    let mut audio = Vec::new();
    for value in [0x1111i16, 0x2222] {
        for _ in 0..588 {
            audio.extend_from_slice(&value.to_le_bytes());
            audio.extend_from_slice(&(-value).to_le_bytes());
        }
    }
    (
        vec![0xA5; 2048],
        audio,
        "FILE \"data.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"audio.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n",
    )
}

#[test]
fn direct_cue_exports_selected_audio_track_without_loading_a_core() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-cue")?;
    let (data, audio, cue) = cd_fixture();
    let cue_path = directory.path().join("disc.cue");
    std::fs::write(&cue_path, cue)?;
    std::fs::write(directory.path().join("data.bin"), &data)?;
    std::fs::write(directory.path().join("audio.bin"), &audio)?;
    let output = directory.path().join("track.wav");
    let request = AudioDiscoveryRequest {
        output_path: directory.path().join("report.json"),
        input_path: cue_path.clone(),
        archive_member: None,
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: Some(OfflineExport {
            format: SongFormat::Audio(crate::audio_discovery::formats::AudioFormat::Wav),
            output_path: output.clone(),
            selection: SongSelection::Track(2),
            options: RenderOptions::default(),
            explicit: ExplicitExportSettings::default(),
        }),
    };
    let input = load_input(&request)?;
    let disc = input.cdda.as_ref().unwrap();
    assert_eq!(disc.original_disc_len, Some(2048 + 2 * 2352));
    assert_eq!(disc.original_disc_sha256, disc.effective_disc_sha256);
    assert!(!disc.provenance.transforms_applied);
    run_request(&request)?;
    assert!(std::fs::read(output)?.ends_with(&audio[2352..]));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(request.output_path)?)?;
    assert_eq!(report["scan"]["cdda_tracks"].as_array().unwrap().len(), 1);
    assert_eq!(report["scan"]["cdda_tracks"][0]["number"], 2);
    assert_eq!(report["disc"]["provenance"]["source_kind"], "cue");
    assert_eq!(std::fs::read(cue_path)?, cue.as_bytes());
    assert_eq!(std::fs::read(directory.path().join("audio.bin"))?, audio);
    Ok(())
}

#[test]
fn zip_cue_requires_explicit_member_and_records_archive_identity() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-zip-cue")?;
    let (data, audio, cue) = cd_fixture();
    let archive_path = directory.path().join("disc.zip");
    crate::test_support::write_zip(
        &archive_path,
        &[
            ("set/disc.cue", cue.as_bytes()),
            ("set/data.bin", &data),
            ("set/audio.bin", &audio),
            (
                "unused.cue",
                b"FILE \"absent.bin\" BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n",
            ),
        ],
    )?;
    let original = std::fs::read(&archive_path)?;
    let mut request = AudioDiscoveryRequest {
        output_path: directory.path().join("report.json"),
        input_path: archive_path.clone(),
        archive_member: None,
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: None,
    };
    assert!(load_input(&request).is_err());
    request.archive_member = Some("set/disc.cue".to_owned());
    let input = load_input(&request)?;
    let disc = input.cdda.as_ref().unwrap();
    assert_eq!(disc.provenance.source_kind, "zip_cue");
    assert_eq!(disc.provenance.source_media_sha256, sha256_hex(&original));
    assert_eq!(disc.provenance.source_media_len, original.len());
    assert_eq!(
        disc.provenance
            .selected_member_path_sha256
            .as_ref()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(disc.track(2)?.pcm_frames, 588);
    assert!(disc.track(1).is_err());
    assert_eq!(std::fs::read(archive_path)?, original);
    Ok(())
}
