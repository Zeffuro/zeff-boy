use super::*;

#[test]
fn capture_validation_reports_a_prefix_as_capped() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let (mut trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    trace.end_cycle = u64::from(trace.cycle_hz) * 3;
    let input = directory.path().join("capture.zip");
    crate::audio_discovery::trace_capture::write_new(
        &input,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    let request = Request {
        capture: true,
        input,
        output: directory.path().join("report.json"),
        member: None,
        selection: None,
        limit: 1,
        options: RenderOptions {
            max_seconds: 1,
            ..Default::default()
        },
        wav: None,
        reference: None,
    };
    run(&request)?;
    let report: Value = serde_json::from_slice(&std::fs::read(&request.output)?)?;
    let evidence = &report["outcome"]["playback"]["evidence"];
    assert_eq!(evidence["pcm"]["frames"], 48000);
    assert_eq!(evidence["session_duration_frames"], 96000);
    assert_eq!(evidence["validation_duration_capped"], true);
    assert_eq!(
        report["outcome"]["writer_evidence"]["end_cycle"],
        trace.end_cycle
    );
    Ok(())
}

fn arguments(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn validation_parser_bounds_work_and_rejects_mixed_modes() {
    let base = [
        "--audio-validate",
        "new-validation-report.json",
        "input.gba",
    ];
    let request = parse(&arguments(&base)).unwrap().unwrap();
    assert_eq!(request.limit, 32);
    assert_eq!(request.options.max_seconds, 10);
    for extra in [
        vec!["--audio-max-seconds", "0"],
        vec!["--audio-max-seconds", "121"],
        vec!["--audio-validation-limit", "257"],
        vec!["--audio-sample-rate", "123"],
        vec!["--audio-song-id", "vgm:-1"],
        vec!["--audio-capture-wav", "output.wav"],
        vec!["--audio-max-seconds", "2", "--audio-max-seconds", "3"],
        vec!["--audio-discover", "other.json"],
    ] {
        let mut args = arguments(&base);
        args.extend(arguments(&extra));
        assert!(parse(&args).is_err(), "{extra:?}");
    }
    assert!(
        parse(&arguments(&[
            "--audio-capture-check",
            "new-report.json",
            "input.zip",
            "--archive-member",
            "capture.vgm"
        ]))
        .is_err()
    );
    assert!(parse(&arguments(&["--audio-validation-limit", "1"])).is_err());
    assert!(parse(&arguments(&["--audio-capture-reference-f32", "native.f32"])).is_err());
    let mut args = arguments(&[
        "--audio-capture-check",
        "new-report.json",
        "capture.zip",
        "--audio-capture-reference-f32",
        "native.f32",
    ]);
    assert!(parse(&args).is_err());
    args.extend(arguments(&["--audio-sample-rate", "48000"]));
    assert!(parse(&args).is_ok());
    args[4] = "new-report.json".into();
    assert!(parse(&args).is_err());
    args[4] = "native.f32".into();
    args.extend(arguments(&["--audio-capture-wav", "native.f32"]));
    assert!(parse(&args).is_err());
}

#[test]
fn catalog_validation_records_real_playback_and_input_failures() -> Result<()> {
    let directory = crate::test_support::test_directory("audio-validation-cli")?;
    let mut vgm = crate::audio_discovery::vgm_export::tests::fixture(false);
    vgm.truncate(u32::from_le_bytes(vgm[4..8].try_into()?) as usize + 4);
    vgm[8..12].copy_from_slice(&0x171_u32.to_le_bytes());
    let input = directory.path().join("input.vgm");
    std::fs::write(&input, &vgm)?;
    let mut request = Request {
        capture: false,
        output: directory.path().join("report.json"),
        input,
        member: None,
        selection: Some(SongId::Vgm(0)),
        limit: 1,
        options: RenderOptions {
            max_seconds: 1,
            ..RenderOptions::default()
        },
        wav: None,
        reference: None,
    };
    run(&request)?;
    let report: Value = serde_json::from_slice(&std::fs::read(&request.output)?)?;
    assert_eq!(report["outcome"]["attempted"], 1);
    let playback = &report["outcome"]["rows"][0]["playback"];
    assert_eq!(playback["status"], "rendered");
    assert_eq!(playback["evidence"]["fresh_render_matches"], true);
    assert_eq!(playback["evidence"]["reset_render_matches"], true);
    assert_eq!(playback["evidence"]["silent"], false);
    request.input = directory.path().join("missing.gba");
    request.output = directory.path().join("missing.json");
    assert!(run(&request).is_err());
    let report: Value = serde_json::from_slice(&std::fs::read(&request.output)?)?;
    assert_eq!(report["outcome"]["status"], "input_error");
    Ok(())
}

#[test]
fn capture_validation_exports_pcm_with_capture_identity() -> Result<()> {
    let directory = crate::test_support::test_directory("capture-validation-cli")?;
    let (trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    let request = Request {
        capture: true,
        output: directory.path().join("report.json"),
        input: directory.path().join("capture.zip"),
        member: None,
        selection: None,
        limit: 1,
        options: RenderOptions {
            max_seconds: 1,
            ..RenderOptions::default()
        },
        wav: Some(directory.path().join("capture.wav")),
        reference: None,
    };
    crate::audio_discovery::trace_capture::write_new(
        &request.input,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    run(&request)?;
    let report: Value = serde_json::from_slice(&std::fs::read(&request.output)?)?;
    assert_eq!(report["outcome"]["status"], "integrity_verified");
    assert_eq!(report["outcome"]["wav_export"]["status"], "written");
    let mut wav = hound::WavReader::open(request.wav.unwrap())?;
    let pcm: Vec<_> = wav
        .samples::<i16>()
        .collect::<std::result::Result<_, _>>()?;
    let bytes: Vec<_> = pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    assert_eq!(
        report["outcome"]["playback"]["evidence"]["pcm"]["pcm_sha256"],
        zeff_firmware::sha256_hex(&bytes)
    );
    Ok(())
}

#[test]
fn reference_failures_are_reported_without_publishing_a_wav() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let (trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    let input = directory.path().join("capture.zip");
    let reference = directory.path().join("native.f32");
    crate::audio_discovery::trace_capture::write_new(
        &input,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    for (index, bytes) in [vec![0; 8], f32::NAN.to_le_bytes().repeat(2)]
        .into_iter()
        .enumerate()
    {
        std::fs::write(&reference, &bytes)?;
        let request = Request {
            capture: true,
            output: directory.path().join(format!("report-{index}.json")),
            input: input.clone(),
            member: None,
            selection: None,
            limit: 1,
            options: RenderOptions {
                max_seconds: 1,
                ..Default::default()
            },
            wav: Some(directory.path().join(format!("capture-{index}.wav"))),
            reference: Some(reference.clone()),
        };
        assert!(run(&request).is_err());
        let report: Value = serde_json::from_slice(&std::fs::read(&request.output)?)?;
        assert_eq!(report["outcome"]["playback"]["status"], "rendered");
        assert_eq!(
            report["outcome"]["native_reference"]["status"],
            if index == 0 {
                "mismatch"
            } else {
                "reference_error"
            }
        );
        assert_eq!(report["outcome"]["wav_export"]["status"], "not_written");
        assert!(!request.wav.unwrap().exists());
        assert_eq!(std::fs::read(&reference)?, bytes);
    }
    Ok(())
}
