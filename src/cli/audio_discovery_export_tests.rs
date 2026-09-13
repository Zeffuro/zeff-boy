use super::*;

#[test]
fn explicit_export_settings_must_apply_to_the_selected_engine_and_format() -> anyhow::Result<()> {
    for (flag, value, gb_midi_allowed) in [
        ("--audio-loops", "2", true),
        ("--audio-max-seconds", "10", true),
        ("--audio-fade-seconds", "0", false),
        ("--audio-midi-channel10", "", false),
        ("--audio-bank-select", "gs", false),
        ("--audio-gain", "raw", false),
    ] {
        let request = parse_audio_discovery_args(
            [
                "--audio-discover",
                "report.json",
                "game.gbc",
                "--audio-export",
                "midi",
                "song.mid",
                "--audio-song-offset",
                "0x4000",
                flag,
                value,
            ]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(OsString::from),
        )?
        .unwrap();
        let mut export = request.export.unwrap();
        assert_eq!(
            export.validate_settings(SongId::Gb(0)).is_ok(),
            gb_midi_allowed
        );
        assert_eq!(
            export.validate_settings(SongId::Nes(0)).is_ok(),
            gb_midi_allowed
        );
        assert!(export.validate_settings(SongId::Vgm(0)).is_err());
        assert!(export.validate_settings(SongId::Module(0)).is_err());
        assert!(export.validate_settings(SongId::Gax(0)).is_err());
        assert!(export.validate_settings(SongId::Natsume(0)).is_err());
        assert!(export.validate_settings(SongId::Mp2k(0)).is_ok());
        export.format = SongFormat::MappedAssets;
        assert!(export.validate_settings(SongId::Gb(0)).is_err());
        assert!(export.validate_settings(SongId::Nes(0)).is_err());
        assert!(export.validate_settings(SongId::Natsume(0)).is_err());
    }
    Ok(())
}

#[test]
fn standalone_exports_share_the_loaded_media_pipeline_and_never_replace_files() -> anyhow::Result<()>
{
    let directory = crate::test_support::test_directory("audio-export-end-to-end")?;
    let rom = crate::audio_discovery::test_support::gba_fixture();
    let input_path = directory.path().join("fixture.gba");
    std::fs::write(&input_path, &rom)?;
    for info in crate::audio_discovery::formats::SONG_FORMATS
        .iter()
        .filter(|info| {
            info.format.available()
                && !info.format.is_gsf()
                && !matches!(
                    info.format,
                    SongFormat::Xm
                        | SongFormat::Mod
                        | SongFormat::S3m
                        | SongFormat::It
                        | SongFormat::Vgm
                        | SongFormat::Vgz
                        | SongFormat::Gbs
                        | SongFormat::Nsf
                        | SongFormat::Sgc
                        | SongFormat::TrackerPack
                )
        })
    {
        use crate::audio_discovery::formats::AudioFormat;
        let format = info.format;
        let signature = match format {
            SongFormat::Midi => b"MThd".as_slice(),
            SongFormat::SoundFont | SongFormat::Dls | SongFormat::Audio(AudioFormat::Wav) => {
                b"RIFF".as_slice()
            }
            SongFormat::Audio(AudioFormat::Flac) => b"fLaC".as_slice(),
            SongFormat::Audio(AudioFormat::Ogg) => b"OggS".as_slice(),
            SongFormat::Sfz | SongFormat::MidiSoundFont | SongFormat::MappedAssets => {
                b"PK".as_slice()
            }
            SongFormat::Xm
            | SongFormat::Mod
            | SongFormat::S3m
            | SongFormat::It
            | SongFormat::Vgm
            | SongFormat::Vgz
            | SongFormat::Gbs
            | SongFormat::Nsf
            | SongFormat::Sgc
            | SongFormat::TrackerPack
            | SongFormat::Gsf
            | SongFormat::MiniGsfPack => {
                unreachable!("filtered unsupported MP2k formats")
            }
        };
        let output = directory
            .path()
            .join(format!("{}-song.{}", info.id, info.extension));
        let request = AudioDiscoveryRequest {
            output_path: directory
                .path()
                .join(format!("{}-report.json", format.info().id)),
            input_path: input_path.clone(),
            archive_member: None,
            max_work: None,
            max_candidates: None,
            relations: None,
            export: Some(OfflineExport {
                format,
                output_path: output.clone(),
                selection: SongSelection::Offset(0x100),
                options: RenderOptions::default(),
                explicit: ExplicitExportSettings::default(),
            }),
        };
        assert!(run_request(&request)?);
        let original = std::fs::read(&output)?;
        assert!(original.starts_with(signature));
        let repeated = AudioDiscoveryRequest {
            output_path: directory
                .path()
                .join(format!("{}-retry-report.json", format.info().id)),
            ..request
        };
        assert!(run_request(&repeated).is_err());
        assert_eq!(std::fs::read(output)?, original);
    }
    assert_eq!(std::fs::read(input_path)?, rom);
    Ok(())
}

#[test]
fn gsf_accepts_timing_tags_and_rejects_synthesis_options() {
    for format in ["gsf", "minigsf"] {
        let parse = |extra: &[&str]| {
            let mut args = vec![
                "--audio-discover",
                "report.json",
                "game.gba",
                "--audio-export",
                format,
                "output",
                "--audio-song-offset",
                "256",
            ];
            args.extend_from_slice(extra);
            parse_audio_discovery_args(args.into_iter().map(OsString::from))
        };
        assert!(
            parse(&[
                "--audio-loops",
                "2",
                "--audio-max-seconds",
                "30",
                "--audio-fade-seconds",
                "3"
            ])
            .is_ok()
        );
        for extra in [
            vec!["--audio-midi-channel10"],
            vec!["--audio-bank-select", "gs"],
            vec!["--audio-gain", "raw"],
            vec!["--audio-sample-rate", "48000"],
        ] {
            assert!(
                parse(&extra)
                    .unwrap_err()
                    .to_string()
                    .contains("GSF runs the original driver")
            );
        }
    }
}

fn natsume_export(extra: &[&str]) -> anyhow::Result<OfflineExport> {
    let mut args = vec![
        "--audio-discover",
        "report.json",
        "game.gba",
        "--audio-export",
        "wav",
        "song.wav",
        "--audio-song-offset",
        "0x100",
    ];
    args.extend_from_slice(extra);
    Ok(
        parse_audio_discovery_args(args.into_iter().map(OsString::from))?
            .unwrap()
            .export
            .unwrap(),
    )
}

#[test]
fn natsume_audio_cli_options_are_explicit_and_use_the_short_recording_default() -> anyhow::Result<()>
{
    let export = natsume_export(&[])?;
    let options = export.options_for(SongId::Natsume(0))?;
    assert_eq!(options.max_seconds, DEFAULT_DURATION_SECONDS);
    assert_eq!(options.sample_rate, 48_000);
    assert_eq!(options.fade_seconds, 0);
    assert_eq!(options.loops, 1);
    assert!(options.skip_channel10);
    assert_eq!(options.bank_select, BankSelect::Gs);
    assert_eq!(options.playback_gain, PlaybackGain::Raw);

    for rate in [44_100, 48_000, 63_072, 96_000] {
        let rate_text = rate.to_string();
        let export = natsume_export(&[
            "--audio-sample-rate",
            &rate_text,
            "--audio-max-seconds",
            "1800",
            "--audio-fade-seconds",
            "15",
        ])?;
        let options = export.options_for(SongId::Natsume(0))?;
        assert_eq!(options.sample_rate, rate);
        assert_eq!(options.max_seconds, 1800);
        assert_eq!(options.fade_seconds, 15);
    }

    let equal_fade = natsume_export(&["--audio-max-seconds", "15", "--audio-fade-seconds", "15"])?;
    assert_eq!(equal_fade.options_for(SongId::Natsume(0))?.max_seconds, 15);
    for seconds in [1_u16, 7_200] {
        let seconds_text = seconds.to_string();
        assert_eq!(
            natsume_export(&["--audio-max-seconds", &seconds_text])?
                .options_for(SongId::Natsume(0))?
                .max_seconds,
            seconds
        );
    }
    assert_eq!(
        natsume_export(&["--audio-fade-seconds", "0"])?
            .options_for(SongId::Natsume(0))?
            .fade_seconds,
        0
    );
    assert!(
        natsume_export(&["--audio-max-seconds", "1", "--audio-fade-seconds", "2",])?
            .options_for(SongId::Natsume(0))
            .is_err()
    );
    for value in ["0", "7201"] {
        assert!(natsume_export(&["--audio-max-seconds", value]).is_err());
    }
    assert!(natsume_export(&["--audio-fade-seconds", "16"]).is_err());
    Ok(())
}

#[test]
fn natsume_cli_rejects_non_audio_and_mp2k_only_options_even_when_default_valued()
-> anyhow::Result<()> {
    for extra in [
        vec!["--audio-loops", "1"],
        vec!["--audio-loops", "2"],
        vec!["--audio-midi-channel10"],
        vec!["--audio-bank-select", "gs"],
        vec!["--audio-gain", "raw"],
    ] {
        assert!(
            natsume_export(&extra)?
                .options_for(SongId::Natsume(0))
                .is_err()
        );
    }

    for extra in [
        vec!["--audio-sample-rate", "44100"],
        vec!["--audio-max-seconds", "180"],
        vec!["--audio-fade-seconds", "0"],
    ] {
        for format in [SongFormat::MappedAssets, SongFormat::Midi] {
            let mut export = natsume_export(&extra)?;
            export.format = format;
            assert!(export.options_for(SongId::Natsume(0)).is_err());
        }
    }
    Ok(())
}

#[test]
fn new_engine_cli_recording_options_match_the_shared_pcm_contract() -> anyhow::Result<()> {
    for id in [
        SongId::EngineSoftware(0),
        SongId::Gax(0),
        SongId::Krawall(0),
        SongId::GaxNative(0),
        SongId::Musyx(0),
        SongId::Aas(0),
        SongId::Radriver(0),
    ] {
        assert_eq!(
            natsume_export(&[])?.options_for(id)?.max_seconds,
            DEFAULT_DURATION_SECONDS
        );
        let mut export = natsume_export(&[
            "--audio-sample-rate",
            "44100",
            "--audio-max-seconds",
            "2",
            "--audio-fade-seconds",
            "1",
        ])?;
        let options = export.options_for(id)?;
        assert_eq!(
            (
                options.sample_rate,
                options.max_seconds,
                options.fade_seconds
            ),
            (44100, 2, 1)
        );
        crate::audio_discovery::pcm::validate_options(options)?;
        for format in [
            SongFormat::MappedAssets,
            SongFormat::Xm,
            SongFormat::TrackerPack,
        ] {
            export.format = format;
            assert!(export.options_for(id).is_err());
        }
        for flags in [
            vec!["--audio-loops", "1"],
            vec!["--audio-gain", "raw"],
            vec!["--audio-bank-select", "gs"],
            vec!["--audio-midi-channel10"],
            vec!["--audio-max-seconds", "1", "--audio-fade-seconds", "2"],
        ] {
            assert!(natsume_export(&flags)?.options_for(id).is_err());
        }
    }
    Ok(())
}

#[test]
fn explicit_catalog_ids_select_engine_songs_and_reject_ambiguous_cli_arguments()
-> anyhow::Result<()> {
    let parse = |id: &str, extra: &[&str]| {
        let mut args = vec![
            "--audio-discover",
            "report.json",
            "game.gba",
            "--audio-export",
            "wav",
            "song.wav",
            "--audio-song-id",
            id,
        ];
        args.extend_from_slice(extra);
        parse_audio_discovery_args(args.into_iter().map(OsString::from))
    };
    for (value, id) in [
        ("engine_software:0", SongId::EngineSoftware(0)),
        ("krawall:7", SongId::Krawall(7)),
        ("musyx:3", SongId::Musyx(3)),
        ("aas:4", SongId::Aas(4)),
        ("radriver:4", SongId::Radriver(4)),
        ("gbass:4", SongId::Gbass(4)),
        ("nes_native:2", SongId::NesNative(2)),
        ("gb_native:2", SongId::GbNative(2)),
        ("gb:1", SongId::Gb(1)),
        ("aas_stream:1", SongId::AasStream(1)),
        ("aas_pcm:1", SongId::AasPcm(1)),
        ("sega_psg:4", SongId::SegaPsg(4)),
        ("gax_native:2", SongId::GaxNative(2)),
        ("gax:1", SongId::Gax(1)),
    ] {
        assert_eq!(
            parse(value, &[])?.unwrap().export.unwrap().selection,
            SongSelection::Id(id)
        );
        assert!(parse(value, &["--audio-song-offset", "0x100"]).is_err());
        assert!(parse(value, &["--audio-track", "1"]).is_err());
    }
    for value in [
        "krawall",
        "krawall:-1",
        "krawall:1:2",
        "unknown:0",
        "krawall:4294967296",
    ] {
        assert!(parse(value, &[]).is_err());
    }
    let directory = tempfile::tempdir()?;
    let mut bytes = vec![0; 0xc0];
    bytes[..4].copy_from_slice(&0xeaff_fffeu32.to_le_bytes());
    bytes[0xb2] = 0x96;
    bytes.extend(crate::audio_discovery::test_support::engine_software::fixture());
    let input = directory.path().join("fixture.gba");
    std::fs::write(&input, bytes)?;
    let mut request = parse(
        "engine_software:0",
        &["--audio-max-seconds", "1", "--audio-sample-rate", "44100"],
    )?
    .unwrap();
    request.input_path = input;
    request.output_path = directory.path().join("scan.json");
    let wav = directory.path().join("selected.wav");
    request.export.as_mut().unwrap().output_path = wav.clone();
    assert!(run_request(&request)?);
    let mut reader = hound::WavReader::open(wav)?;
    assert_eq!(reader.spec().sample_rate, 44_100);
    assert_eq!(reader.duration(), 44_100);
    assert!(reader.samples::<i16>().any(|sample| sample.unwrap() != 0));
    Ok(())
}

#[test]
fn banked_gb_cli_records_native_audio_with_explicit_duration_and_rate() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("fixture.gbc");
    std::fs::write(
        &input,
        zeff_audio_discovery::gb_music::native::fixture_rom(),
    )?;
    let mut request = parse_audio_discovery_args(
        [
            "--audio-discover",
            "scan.json",
            "fixture.gbc",
            "--audio-export",
            "wav",
            "song.wav",
            "--audio-song-id",
            "gb:1",
            "--audio-max-seconds",
            "1",
            "--audio-sample-rate",
            "44100",
        ]
        .into_iter()
        .map(OsString::from),
    )?
    .unwrap();
    request.input_path = input;
    request.output_path = directory.path().join("scan.json");
    let output = directory.path().join("song.wav");
    request.export.as_mut().unwrap().output_path = output.clone();
    assert!(run_request(&request)?);
    let mut wave = hound::WavReader::open(output)?;
    assert_eq!(wave.spec().sample_rate, 44_100);
    assert_eq!(wave.duration(), 44_100);
    assert!(wave.samples::<i16>().any(|sample| sample.unwrap() != 0));
    let export = request.export.as_mut().unwrap();
    export.explicit.loops = true;
    assert!(export.options_for(SongId::Gb(1)).is_err());
    export.format = SongFormat::Midi;
    export.explicit.sample_rate = false;
    assert!(export.options_for(SongId::Gb(1)).is_ok());
    Ok(())
}
#[test]
fn native_gsf_uses_duration_tags_and_rejects_unapplied_controls() -> anyhow::Result<()> {
    for format in ["gsf", "minigsf"] {
        let request = parse_audio_discovery_args(
            [
                "--audio-discover",
                "scan.json",
                "game.gba",
                "--audio-song-id",
                "gbass:0",
                "--audio-export",
                format,
                "song.out",
                "--audio-max-seconds",
                "12",
                "--audio-fade-seconds",
                "2",
            ]
            .into_iter()
            .map(OsString::from),
        )?
        .unwrap();
        let mut export = request.export.unwrap();
        let options = export.options_for(SongId::Gbass(0))?;
        assert_eq!((options.max_seconds, options.fade_seconds), (12, 2));
        export.explicit.max_seconds = false;
        assert_eq!(export.options_for(SongId::Gbass(0))?.max_seconds, 180);
        export.explicit.loops = true;
        assert!(export.options_for(SongId::Gbass(0)).is_err());
        export.explicit.loops = false;
        export.explicit.sample_rate = true;
        assert!(export.options_for(SongId::Gbass(0)).is_err());
        export.explicit.sample_rate = false;
        export.explicit.gain = true;
        assert!(export.options_for(SongId::Gbass(0)).is_err());
    }
    Ok(())
}

use crate::audio_discovery::formats::FormatAvailability;
