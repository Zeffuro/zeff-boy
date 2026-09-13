use super::*;

fn args(extra: &[&str]) -> anyhow::Result<Option<AudioDiscoveryRequest>> {
    let mut args = vec!["--audio-discover", "scan.json", "game.gba"];
    args.extend_from_slice(extra);
    parse_audio_discovery_args(args.into_iter().map(OsString::from))
}

#[test]
fn relationships_require_an_explicit_selection_and_no_render_settings() {
    let request = args(&[
        "--audio-relations",
        "graph.json",
        "--audio-song-offset",
        "0x100",
    ])
    .unwrap()
    .unwrap();
    assert!(request.export.is_none());
    assert_eq!(
        request.relations.unwrap().selection,
        SongSelection::Offset(0x100)
    );
    assert!(args(&["--audio-relations", "graph.json"]).is_err());
    assert!(args(&["--audio-song-offset", "0x100"]).is_err());
    assert!(
        args(&[
            "--audio-relations",
            "one.json",
            "--audio-relations",
            "two.json",
            "--audio-track",
            "1"
        ])
        .is_err()
    );
    assert!(
        args(&[
            "--audio-relations",
            "graph.json",
            "--audio-song-offset",
            "0x100",
            "--audio-loops",
            "2"
        ])
        .is_err()
    );
    assert!(
        parse_audio_discovery_args(["--audio-relations", "graph.json"].map(OsString::from))
            .is_err()
    );
    assert!(args(&["--audio-relations", "graph.json", "--audio-track", "2"]).is_ok());
    assert!(
        args(&[
            "--audio-relations",
            "graph.json",
            "--audio-export",
            "midi",
            "song.mid",
            "--audio-song-offset",
            "0x100"
        ])
        .is_ok()
    );
}

#[test]
fn relationship_json_preserves_zip_identity_and_existing_outputs() -> anyhow::Result<()> {
    let dir = crate::test_support::test_directory("audio-relations-export")?;
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let archive = dir.path().join("game.zip");
    crate::test_support::write_zip(
        &archive,
        &[("selected.gba", &bytes), ("other.gba", b"untouched")],
    )?;
    let graph_path = dir.path().join("relations.json");
    let mut request = AudioDiscoveryRequest {
        output_path: dir.path().join("scan.json"),
        input_path: archive.clone(),
        archive_member: Some("selected.gba".into()),
        max_work: None,
        max_candidates: None,
        export: None,
        driver_evidence: None,
        relations: Some(RelationsExport {
            output_path: graph_path.clone(),
            selection: SongSelection::Offset(0x100),
        }),
    };
    let before = std::fs::read(&archive)?;
    run_request(&request)?;
    let scan: serde_json::Value = serde_json::from_slice(&std::fs::read(&request.output_path)?)?;
    let graph_bytes = std::fs::read(&graph_path)?;
    let graph: serde_json::Value = serde_json::from_slice(&graph_bytes)?;
    assert_eq!(graph["schema"], "zeff-audio-relations-export/1");
    assert_eq!(graph["source"], scan["source"]);
    assert_eq!(graph["transforms"], scan["transforms"]);
    assert_eq!(
        graph["graph"]["media_sha256"],
        scan["scan"]["media"]["sha256"]
    );
    assert_eq!(graph["graph"]["status"]["kind"], "complete");
    assert!(!graph["graph"]["edges"].as_array().unwrap().is_empty());
    assert!(scan["scan"].get("relations").is_none());
    request.output_path = dir.path().join("second-scan.json");
    assert!(run_request(&request).is_err());
    assert_eq!(std::fs::read(&graph_path)?, graph_bytes);
    assert_eq!(std::fs::read(archive)?, before);
    Ok(())
}

#[test]
fn relationship_selection_and_path_conflicts_fail_before_publication() -> anyhow::Result<()> {
    let dir = crate::test_support::test_directory("audio-relations-reject")?;
    let input = dir.path().join("game.gba");
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    std::fs::write(&input, &bytes)?;
    for (index, alias) in [false, true].into_iter().enumerate() {
        let scan = dir.path().join(format!("scan-{index}.json"));
        let output = if alias {
            input.clone()
        } else {
            dir.path().join("graph.json")
        };
        let request = AudioDiscoveryRequest {
            output_path: scan.clone(),
            input_path: input.clone(),
            archive_member: None,
            max_work: None,
            max_candidates: None,
            export: None,
            driver_evidence: None,
            relations: Some(RelationsExport {
                output_path: output.clone(),
                selection: SongSelection::Offset(if alias { 0x100 } else { 0x999 }),
            }),
        };
        assert!(run_request(&request).is_err());
        assert!(!scan.exists());
        if !alias {
            assert!(!output.exists());
        }
    }
    assert_eq!(std::fs::read(input)?, bytes);
    Ok(())
}

#[test]
fn new_relative_and_absolute_output_aliases_fail_before_writing() -> anyhow::Result<()> {
    let dir = tempfile::Builder::new()
        .prefix("audio-path-test-")
        .tempdir_in(std::env::current_dir()?)?;
    let absolute = dir.path().join("new.json");
    let relative = PathBuf::from(dir.path().file_name().unwrap()).join("new.json");
    assert!(ensure_distinct_output_path(&absolute, &relative).is_err());
    assert!(
        ensure_distinct_output_path(
            &dir.path().join("nested/new.json"),
            &dir.path().join("./nested/new.json")
        )
        .is_err()
    );
    #[cfg(windows)]
    assert!(ensure_distinct_output_path(&absolute, &dir.path().join("NEW.JSON")).is_err());
    let input = dir.path().join("game.gba");
    std::fs::write(&input, crate::audio_discovery::test_support::gba_fixture())?;
    let request = AudioDiscoveryRequest {
        output_path: relative,
        input_path: input,
        archive_member: None,
        max_work: None,
        max_candidates: None,
        export: None,
        driver_evidence: None,
        relations: Some(RelationsExport {
            output_path: absolute.clone(),
            selection: SongSelection::Offset(0x100),
        }),
    };
    assert!(run_request(&request).is_err());
    assert!(!absolute.exists());
    Ok(())
}
