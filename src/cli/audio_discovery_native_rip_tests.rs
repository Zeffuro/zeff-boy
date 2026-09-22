use super::*;
use zeff_audio_discovery::native_rips;
use zeff_emu_common::system::System;

#[test]
fn native_rip_cli_routes_original_drivers_and_never_replaces_outputs() -> anyhow::Result<()> {
    let cases = [
        (
            zeff_audio_discovery::gb_native::fixture_rom(),
            System::Gb,
            "gb",
            "gb_native",
            SongId::GbNative(0),
            "gbs",
        ),
        (
            zeff_audio_discovery::nes_native::fixture_rom(),
            System::Nes,
            "nes",
            "nes_native",
            SongId::NesNative(0),
            "nsf",
        ),
        (
            zeff_audio_discovery::nes_native::fixture_rom(),
            System::Nes,
            "nes",
            "nes_native",
            SongId::NesNative(0),
            "nsfe",
        ),
        (
            zeff_audio_discovery::sega_psg::fixture_rom(),
            System::Sms,
            "sms",
            "sega_psg",
            SongId::SegaPsg(0),
            "sgc",
        ),
    ];
    let directory = tempfile::tempdir()?;
    for (bytes, system, extension, engine, id, format) in cases {
        let input = directory.path().join(format!("fixture.{extension}"));
        let output = directory.path().join(format!("song.{format}"));
        std::fs::write(&input, &bytes)?;
        let mut request = parse_audio_discovery_args(
            [
                "--audio-discover",
                "scan.json",
                "input",
                "--audio-song-id",
                &format!("{engine}:0"),
                "--audio-export",
                format,
                "output",
            ]
            .into_iter()
            .map(OsString::from),
        )?
        .unwrap();
        request.input_path = input.clone();
        request.output_path = directory.path().join(format!("{format}-scan.json"));
        request.export.as_mut().unwrap().output_path = output.clone();
        assert!(run_request(&request)?);
        let report =
            zeff_audio_discovery::scan(system, &bytes, Default::default(), &AtomicBool::new(false));
        let expected = if format == "nsfe" {
            native_rips::encode_as(
                &bytes,
                report.song(id).unwrap(),
                native_rips::NativeRipFormat::Nsfe,
                &AtomicBool::new(false),
            )?
        } else {
            native_rips::encode(&bytes, report.song(id).unwrap(), &AtomicBool::new(false))?
        };
        assert_eq!(std::fs::read(&output)?, expected.bytes);
        assert!(run_request(&request).is_err());
        assert_eq!(std::fs::read(&output)?, expected.bytes);
        assert_eq!(std::fs::read(input)?, bytes);
        let export = request.export.as_mut().unwrap();
        export.explicit.max_seconds = true;
        assert!(export.options_for(id).is_err());
        export.explicit.max_seconds = false;
        export.explicit.sample_rate = true;
        assert!(export.options_for(id).is_err());
    }
    Ok(())
}

#[test]
fn unsupported_banked_gb_profile_does_not_offer_a_gbs_export() {
    let bytes = zeff_audio_discovery::gb_music::native::fixture_rom();
    let report = zeff_audio_discovery::scan(
        System::Gb,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    let song = report.song(SongId::Gb(1)).unwrap();
    assert!(!song.supports(SongFormat::Gbs));
    assert!(native_rips::supported_format(song).is_none());
}
