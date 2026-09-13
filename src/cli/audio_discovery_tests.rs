use std::ffi::OsString;
use std::path::PathBuf;

use super::*;
use zeff_emu_common::system::System;

#[test]
fn parser_accepts_only_the_dedicated_discovery_surface() {
    let request = parse_audio_discovery_args([
        OsString::from("--audio-discover"),
        OsString::from("target/report.json"),
        OsString::from("game.zip"),
        OsString::from("--archive-member"),
        OsString::from("nested/game.gba"),
        OsString::from("--audio-scan-work"),
        OsString::from("0x40"),
        OsString::from("--audio-scan-candidates"),
        OsString::from("0x1000"),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(request.output_path, PathBuf::from("target/report.json"));
    assert_eq!(request.input_path, PathBuf::from("game.zip"));
    assert_eq!(request.archive_member.as_deref(), Some("nested/game.gba"));
    assert_eq!(request.max_work, Some(0x40));
    assert_eq!(request.max_candidates, Some(MAX_CANDIDATES));

    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("--headless"),
            OsString::from("game.gba"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--headless"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.zip"),
            OsString::from("--archive-member"),
            OsString::from("one.gba"),
            OsString::from("--archive-member"),
            OsString::from("two.gba"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--audio-scan-work"),
            OsString::from("1"),
            OsString::from("--audio-scan-work"),
            OsString::from("2"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.zip"),
            OsString::from("--archive-member"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--audio-discover"),
            OsString::from("again.json"),
            OsString::from("again.gba"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--archive-member"),
            OsString::from("game.gba"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([OsString::from("--audio-scan-work"), OsString::from("10"),])
            .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-scan-candidates"),
            OsString::from("10"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([OsString::from("--headless")])
            .unwrap()
            .is_none()
    );
}

#[test]
fn candidate_limit_argument_requires_a_unique_bounded_u32() {
    let valid = |value: &str| {
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--audio-scan-candidates"),
            OsString::from(value),
        ])
    };
    assert_eq!(valid("1").unwrap().unwrap().max_candidates, Some(1));
    assert_eq!(
        valid(&MAX_CANDIDATES.to_string())
            .unwrap()
            .unwrap()
            .max_candidates,
        Some(MAX_CANDIDATES)
    );

    let above_maximum = (u64::from(MAX_CANDIDATES) + 1).to_string();
    for value in [
        "0",
        "-1",
        "not-a-number",
        "4294967296",
        above_maximum.as_str(),
    ] {
        assert!(valid(value).is_err(), "{value:?}");
    }
    assert!(
        valid("0")
            .unwrap_err()
            .to_string()
            .contains("must be between 1")
    );
    assert!(
        valid("not-a-number")
            .unwrap_err()
            .to_string()
            .contains("must be an unsigned integer")
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--audio-scan-candidates"),
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args([
            OsString::from("--audio-discover"),
            OsString::from("report.json"),
            OsString::from("game.gba"),
            OsString::from("--audio-scan-candidates"),
            OsString::from("1"),
            OsString::from("--audio-scan-candidates"),
            OsString::from("2"),
        ])
        .is_err()
    );
}

#[test]
fn archive_member_rejects_traversal_and_absolute_paths() -> anyhow::Result<()> {
    for member in [
        "",
        "../game.gba",
        "nested/../game.gba",
        "C:\\game.gba",
        "nested/game.bin",
    ] {
        assert!(normalize_archive_member(member).is_err(), "{member:?}");
    }
    assert_eq!(
        normalize_archive_member("nested\\game.gba")?,
        "nested/game.gba"
    );
    Ok(())
}

#[test]
fn direct_input_is_bounded_and_never_mutates_the_source() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-direct-input")?;
    let rom_path = directory.path().join("fixture.gba");
    let bytes = vec![0xA5; MIN_GBA_ROM_BYTES as usize];
    std::fs::write(&rom_path, &bytes)?;

    let loaded = read_bounded_cartridge_file(&rom_path, System::Gba)?;
    assert_eq!(loaded, bytes);
    assert_eq!(std::fs::read(&rom_path)?, bytes);
    Ok(())
}

#[test]
fn direct_input_rejects_lengths_outside_the_gba_bounds() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-direct-bounds")?;
    let too_small = directory.path().join("small.gba");
    std::fs::write(&too_small, vec![0; MIN_GBA_ROM_BYTES as usize - 1])?;
    assert!(read_bounded_cartridge_file(&too_small, System::Gba).is_err());

    let too_large = directory.path().join("large.gba");
    let file = std::fs::File::create(&too_large)?;
    file.set_len(MAX_ROM_BYTES_U64 + 1)?;
    assert!(read_bounded_cartridge_file(&too_large, System::Gba).is_err());
    Ok(())
}

#[test]
fn selected_zip_member_is_reported_without_scanning_siblings() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-selected-zip")?;
    let archive_path = directory.path().join("fixture.zip");
    let selected = vec![0x11; MIN_GBA_ROM_BYTES as usize];
    let sibling = vec![0x22; MIN_GBA_ROM_BYTES as usize];
    crate::test_support::write_zip(
        &archive_path,
        &[
            ("selected.gba", selected.as_slice()),
            ("sibling.gba", sibling.as_slice()),
        ],
    )?;
    let request = AudioDiscoveryRequest {
        output_path: directory.path().join("report.json"),
        input_path: archive_path,
        archive_member: Some("selected.gba".to_owned()),
        max_work: None,
        max_candidates: None,
        driver_evidence: None,
        relations: None,
        export: None,
    };

    let input = load_input(&request)?;
    assert_eq!(&*input.bytes, selected.as_slice());
    let source = &input.provenance.as_ref().unwrap().source;
    assert_eq!(
        source.selected_member.as_ref().unwrap().name,
        "selected.gba"
    );
    assert_eq!(
        source.container.as_ref().unwrap().sha256,
        sha256_hex(&std::fs::read(&request.input_path)?)
    );
    Ok(())
}

#[test]
fn report_writer_never_overwrites_an_existing_file() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-output")?;
    let report_path = directory.path().join("report.json");
    std::fs::write(&report_path, b"keep")?;

    let input_path = directory.path().join("fixture.gba");
    std::fs::write(&input_path, vec![0; MIN_GBA_ROM_BYTES as usize])?;
    assert!(
        run_request(&AudioDiscoveryRequest {
            output_path: report_path.clone(),
            input_path,
            archive_member: None,
            max_work: Some(1),
            max_candidates: None,
            driver_evidence: None,
            relations: None,
            export: None,
        })
        .is_err()
    );
    assert_eq!(std::fs::read(report_path)?, b"keep");
    Ok(())
}

#[test]
fn request_runs_end_to_end_without_mutating_the_input() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-end-to-end")?;
    let rom_path = directory.path().join("fixture.gba");
    let report_path = directory.path().join("report.json");
    let bytes = vec![0; MIN_GBA_ROM_BYTES as usize];
    std::fs::write(&rom_path, &bytes)?;
    let request = AudioDiscoveryRequest {
        output_path: report_path.clone(),
        input_path: rom_path,
        archive_member: None,
        max_work: Some(1),
        max_candidates: Some(1),
        driver_evidence: None,
        relations: None,
        export: None,
    };

    assert!(run_request(&request)?);
    assert_eq!(std::fs::read(&request.input_path)?, bytes);
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report_path)?)?;
    assert_eq!(report["schema"], "zeff-audio-discovery/1");
    assert_eq!(report["analysis_profile"], "standalone-unmodified-v1");
    assert_eq!(report["transforms"], serde_json::json!([]));
    assert_eq!(report["source"]["kind"], "direct_gba_file");
    assert_eq!(report["scan"]["limits"]["max_candidates"], 1);
    Ok(())
}

#[test]
fn output_alias_of_existing_input_is_rejected() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-discovery-output-alias")?;
    let input = directory.path().join("fixture.gba");
    std::fs::write(&input, vec![0; MIN_GBA_ROM_BYTES as usize])?;
    let alias = directory.path().join(".").join("fixture.gba");

    assert!(ensure_distinct_output_path(&alias, &input).is_err());
    Ok(())
}

#[test]
fn audio_export_requires_an_explicit_song_and_supported_format() {
    let parse = |extra: &[&str]| {
        let mut args = vec!["--audio-discover", "report.json", "game.gba"];
        args.extend_from_slice(extra);
        parse_audio_discovery_args(args.into_iter().map(OsString::from))
    };
    let request = parse(&[
        "--audio-export",
        "sf2",
        "instruments.sf2",
        "--audio-song-offset",
        "0x100",
    ])
    .unwrap()
    .unwrap();
    let export = request.export.unwrap();
    assert_eq!(export.selection, SongSelection::Offset(0x100));
    assert_eq!(export.format, SongFormat::SoundFont);
    assert_eq!(export.options.loops, 1);
    assert_eq!(export.options.max_seconds, 1800);
    assert!(export.options.skip_channel10);
    assert_eq!(export.options.bank_select, BankSelect::Gs);
    assert_eq!(export.options.playback_gain.id(), "raw");
    assert!(parse(&["--audio-export", "sf2", "instruments.sf2"]).is_err());
    assert!(parse(&["--audio-song-offset", "0x100"]).is_err());
    assert!(
        parse(&[
            "--audio-export",
            "mp3",
            "song.mp3",
            "--audio-song-offset",
            "0x100"
        ])
        .is_err()
    );
    assert!(parse(&["--audio-export", "sf2", "--audio-song-offset", "0x100"]).is_err());

    let configured = parse(&[
        "--audio-export",
        "midi-sf2",
        "song.zip",
        "--audio-song-offset",
        "0x100",
        "--audio-loops",
        "8",
        "--audio-max-seconds",
        "7200",
        "--audio-fade-seconds",
        "15",
        "--audio-midi-channel10",
        "--audio-bank-select",
        "mma",
        "--audio-gain",
        "mp2k",
    ])
    .unwrap()
    .unwrap()
    .export
    .unwrap();
    assert_eq!(configured.format, SongFormat::MidiSoundFont);
    assert_eq!(configured.options.loops, 8);
    assert_eq!(configured.options.max_seconds, 7200);
    assert_eq!(configured.options.fade_seconds, 15);
    assert!(!configured.options.skip_channel10);
    assert_eq!(configured.options.bank_select, BankSelect::Mma);
    assert_eq!(
        configured.options.playback_gain,
        PlaybackGain::Mp2kAmplitude
    );
    assert!(parse(&["--audio-loops", "2"]).is_err());
    assert!(
        parse(&[
            "--audio-export",
            "midi",
            "song.mid",
            "--audio-song-offset",
            "0x100",
            "--audio-max-seconds",
            "7201",
        ])
        .is_err()
    );
}

#[test]
fn track_and_gain_arguments_require_a_unique_applicable_selection() {
    let parse = |extra: &[&str]| {
        let mut args = vec!["--audio-discover", "report.json", "disc.cue"];
        args.extend_from_slice(extra);
        parse_audio_discovery_args(args.into_iter().map(OsString::from))
    };
    let export = parse(&["--audio-export", "wav", "track.wav", "--audio-track", "2"])
        .unwrap()
        .unwrap()
        .export
        .unwrap();
    assert_eq!(export.selection, SongSelection::Track(2));
    for args in [
        vec!["--audio-track", "2"],
        vec!["--audio-gain", "mp2k"],
        vec!["--audio-export", "wav", "track.wav", "--audio-track", "0"],
        vec!["--audio-export", "wav", "track.wav", "--audio-track", "100"],
        vec![
            "--audio-export",
            "wav",
            "track.wav",
            "--audio-track",
            "2",
            "--audio-song-offset",
            "0",
        ],
        vec![
            "--audio-export",
            "wav",
            "track.wav",
            "--audio-track",
            "2",
            "--audio-track",
            "3",
        ],
        vec![
            "--audio-export",
            "wav",
            "track.wav",
            "--audio-track",
            "2",
            "--audio-gain",
            "mp2k",
        ],
        vec![
            "--audio-export",
            "midi",
            "song.mid",
            "--audio-song-offset",
            "0",
            "--audio-gain",
            "linear",
        ],
        vec![
            "--audio-export",
            "midi",
            "song.mid",
            "--audio-song-offset",
            "0",
            "--audio-gain",
            "raw",
            "--audio-gain",
            "mp2k",
        ],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    assert_eq!(
        parse(&["--audio-export", "xm", "song.xm", "--audio-song", "0x40"])
            .unwrap()
            .unwrap()
            .export
            .unwrap()
            .selection,
        SongSelection::Offset(0x40)
    );
    for flag in ["--audio-track", "--audio-song", "--audio-gain"] {
        assert!(parse_audio_discovery_args([OsString::from(flag), OsString::from("1")]).is_err());
    }
}

#[test]
fn cartridge_extension_inference_matches_the_emulator_registry() {
    for spec in System::specs() {
        for extension in spec.rom_extensions {
            let path = PathBuf::from(format!("game.{extension}"));
            if ["cue", "chd", "iso"].contains(extension) {
                assert!(cartridge_system(&path).is_none());
            } else {
                assert_eq!(cartridge_system(&path), Some(spec.system));
                assert_eq!(
                    cartridge_system(&PathBuf::from(format!(
                        "GAME.{}",
                        extension.to_ascii_uppercase()
                    ))),
                    Some(spec.system)
                );
                assert!(normalize_archive_member(&format!("nested/game.{extension}")).is_ok());
            }
        }
    }
    for extension in ["bin", "xm", "mod", "s3m", "it", "7z", "rar"] {
        assert!(cartridge_system(&PathBuf::from(format!("game.{extension}"))).is_none());
    }
    assert!(normalize_archive_member("sets/disc.cue").is_ok());
}
