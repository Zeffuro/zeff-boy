use super::*;

#[test]
fn all_songs_selection_requires_export_and_rejects_competing_selections() {
    let base = ["--audio-discover", "scan.json", "game.gba"];
    for extra in [
        vec!["--audio-all-songs"],
        vec![
            "--audio-all-songs",
            "--audio-all-songs",
            "--audio-export",
            "midi",
            "all.zip",
        ],
        vec![
            "--audio-all-songs",
            "--audio-song-id",
            "mp2k:0",
            "--audio-export",
            "midi",
            "all.zip",
        ],
        vec![
            "--audio-song-offset",
            "0x100",
            "--audio-all-songs",
            "--audio-export",
            "midi",
            "all.zip",
        ],
        vec![
            "--audio-all-songs",
            "--audio-export",
            "midi",
            "all.zip",
            "--audio-relations",
            "graph.json",
        ],
    ] {
        assert!(
            parse_audio_discovery_args(base.into_iter().chain(extra).map(OsString::from)).is_err()
        );
    }
    assert!(parse_audio_discovery_args([OsString::from("--audio-all-songs")]).is_err());
}

#[test]
fn all_songs_cli_writes_zip_and_validates_settings_before_any_output() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let rom = directory.path().join("fixture.gba");
    let mut bytes = zeff_audio_discovery::gbass::fixture_rom_banked();
    bytes[0xB2] = 0x96;
    std::fs::write(&rom, bytes)?;
    let scan = directory.path().join("scan.json");
    let zip = directory.path().join("all.zip");
    let args = vec![
        OsString::from("--audio-discover"),
        scan.as_os_str().into(),
        rom.as_os_str().into(),
        "--audio-all-songs".into(),
        "--audio-export".into(),
        "minigsf".into(),
        zip.as_os_str().into(),
    ];
    let mut invalid = args.clone();
    invalid.extend([OsString::from("--audio-loops"), "3".into()]);
    assert!(run_request(&parse_audio_discovery_args(invalid)?.unwrap()).is_err());
    assert!(!scan.exists());
    assert!(!zip.exists());
    let request = parse_audio_discovery_args(args)?.unwrap();
    assert_eq!(
        request.export.as_ref().unwrap().selection,
        SongSelection::All
    );
    assert!(run_request(&request)?);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip)?)?;
    let report: serde_json::Value = serde_json::from_reader(archive.by_name("batch-report.json")?)?;
    assert_eq!(report["summary"]["exported"], 4);
    assert_eq!(report["summary"]["failed"], 0);
    let before = std::fs::read(&zip)?;
    assert!(run_request(&request).is_err());
    assert_eq!(before, std::fs::read(zip)?);
    Ok(())
}
