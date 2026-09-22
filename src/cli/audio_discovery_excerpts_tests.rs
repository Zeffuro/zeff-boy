use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn excerpt_options_require_explicit_rate_and_preserve_inputs() -> Result<()> {
    let base = ["--audio-capture-excerpts", "new-excerpts", "capture.zip"];
    assert!(parse(&args(&base)).is_err());
    let mut good = args(&base);
    good.extend(args(&["--audio-sample-rate", "48000"]));
    assert!(parse(&good)?.is_some());
    for extra in [
        vec!["--audio-sample-rate", "44100"],
        vec!["--audio-max-seconds", "5"],
        vec!["--audio-capture-reference-f32"],
        vec!["--audio-capture-sweep", "out"],
    ] {
        let mut invalid = good.clone();
        invalid.extend(args(&extra));
        assert!(parse(&invalid).is_err());
    }
    good[4] = "123".into();
    assert!(parse(&good).is_err());
    good[4] = "48000".into();
    good[2] = "new-excerpts/capture.zip".into();
    assert!(parse(&good).is_err());
    good[2] = "capture.zip".into();
    good.extend(args(&[
        "--audio-capture-reference-f32",
        "new-excerpts/native.f32",
    ]));
    assert!(parse(&good).is_err());
    Ok(())
}

#[test]
fn excerpt_cli_records_load_failure_and_preserves_existing_directory() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let request = Request {
        output: temp.path().join("out"),
        input: temp.path().join("missing.zip"),
        sample_rate: 48000,
        reference: None,
    };
    assert!(run(&request).is_err());
    let report_path = request.output.join("report.json");
    let before = std::fs::read(&report_path)?;
    let report: Value = serde_json::from_slice(&before)?;
    assert_eq!(report["schema"], "zeff-audio-capture-excerpts/1");
    assert_eq!(report["outcome"]["status"], "failed");
    assert!(run(&request).is_err());
    assert_eq!(std::fs::read(&report_path)?, before);
    Ok(())
}

#[test]
fn excerpt_cli_publishes_complete_report_and_verified_wav() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let input = temp.path().join("capture.zip");
    let (trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    crate::audio_discovery::trace_capture::write_new(
        &input,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    let request = Request {
        output: temp.path().join("out"),
        input,
        sample_rate: 48000,
        reference: None,
    };
    run(&request)?;
    let report: Value =
        serde_json::from_slice(&std::fs::read(request.output.join("report.json"))?)?;
    assert_eq!(report["outcome"]["status"], "complete");
    assert_eq!(report["outcome"]["excerpt_count"], 1);
    assert!(request.output.join("excerpt-000.wav").is_file());
    Ok(())
}
