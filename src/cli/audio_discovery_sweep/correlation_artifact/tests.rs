use super::*;
use crate::audio_discovery::{render::RenderOptions, validation};
use crate::cli::types::HeadlessOptions;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn observe(loaded: &input::Loaded, row: &Value, path: &Path, cancel: &AtomicBool) -> Value {
    let result = super::observe(loaded, row, path, cancel);
    assert_eq!(result.correlation["status"], result.runtime["status"]);
    assert_eq!(result.correlation["reason"], result.runtime["reason"]);
    if result.runtime["status"] == "complete" {
        assert_eq!(result.control["status"], "complete", "{}", result.control);
        assert_eq!(result.control["observed_writes"], 2);
    } else {
        assert_eq!(result.control["reason"], result.runtime["reason"]);
        assert_eq!(result.control["observations"], json!([]));
    }
    result.correlation
}

#[test]
fn native_capture_join_requires_the_validated_archive_and_source() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fixture.nes");
    let capture = directory.path().join("capture.zip");
    let native = directory.path().join("native.f32");
    let mut bytes = vec![0; 16 + 0x4000 + 0x2000];
    bytes[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 1]);
    bytes[16..29].copy_from_slice(&[
        0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0x20, 0x20, 0x80, 0x4c, 0x0a, 0x80,
    ]);
    bytes[48..55].copy_from_slice(&[0x8d, 0x15, 0x40, 0x8d, 0, 0x40, 0x60]);
    bytes[16 + 0x3ffa..16 + 0x4000].copy_from_slice(&[0, 0x80, 0, 0x80, 0, 0x80]);
    std::fs::write(&source, &bytes)?;
    let cancel = AtomicBool::new(false);
    let args = vec![
        "--audio-capture-sweep".into(),
        directory.path().join("sweep").into_os_string(),
        source.clone().into_os_string(),
    ];
    let request = input::parse(&args)?.unwrap();
    let loaded = input::load(&request, &cancel)?;
    crate::cli::run_headless(
        &source,
        HardwareModePreference::Auto,
        Vec::new(),
        &HeadlessOptions {
            max_frames: 3,
            no_sram: true,
            audio_trace_path: Some(capture.clone()),
            audio_dump_path: Some(native.clone()),
            ..Default::default()
        },
    )?;
    let artifact = CaptureArtifact::load(&capture)?;
    let playback = validation::validate(
        || artifact.session(RenderOptions::default(), &cancel),
        48_000,
        &cancel,
    )?;
    assert!(playback.silent);
    let reference = validation::reference::compare_f32_file(&native, &playback.pcm, &cancel)?;
    assert!(reference.projected_pcm_matches);
    let context = &artifact.manifest["context"];
    let mut row = json!({
        "plan": "baseline", "status": "success", "requested_steps": 3,
        "applied_input": {"player_1": [], "press": null},
        "capture": {"archive_sha256": artifact.archive_sha256, "trace_sha256": artifact.trace_sha256, "context": context},
        "validation": {
            "status": "integrity_verified", "archive_sha256": artifact.archive_sha256,
            "trace_sha256": artifact.trace_sha256, "capture_manifest": artifact.manifest,
            "playback": {"status": "rendered", "evidence": playback},
            "native_reference": {"status": "matched", "evidence": reference}
        }
    });
    row["candidate_evidence"] =
        evidence::reference_for_row(&loaded.candidate_evidence, &row, false);
    assert!(evidence::binding_matches(&loaded.candidate_evidence, &row));
    let result = observe(&loaded, &row, &capture, &cancel);
    assert_eq!(result["status"], "complete", "{result}");
    let writers = result["candidates"][0]["writers"].as_array().unwrap();
    assert_eq!(writers.len(), 2);
    assert!(
        writers
            .iter()
            .all(|writer| writer["observed"]["count"] == 1)
    );
    assert_eq!(
        observe(&loaded, &row, &capture, &AtomicBool::new(true))["reason"],
        "cancelled"
    );
    let saved = std::fs::read(&capture)?;
    std::fs::write(&capture, b"invalid archive")?;
    assert_eq!(
        observe(&loaded, &row, &capture, &cancel)["reason"],
        "invalid_capture"
    );
    std::fs::write(&capture, saved)?;
    let mut altered = row.clone();
    for location in ["/capture/archive_sha256", "/validation/archive_sha256"] {
        *altered.pointer_mut(location).unwrap() = json!("f".repeat(64));
    }
    assert!(evidence::binding_matches(
        &loaded.candidate_evidence,
        &altered
    ));
    assert_eq!(
        observe(&loaded, &altered, &capture, &cancel)["reason"],
        "invalid_capture"
    );
    let mut altered = row.clone();
    for location in ["/capture/trace_sha256", "/validation/trace_sha256"] {
        *altered.pointer_mut(location).unwrap() = json!("f".repeat(64));
    }
    assert!(evidence::binding_matches(
        &loaded.candidate_evidence,
        &altered
    ));
    assert_eq!(
        observe(&loaded, &altered, &capture, &cancel)["reason"],
        "invalid_capture"
    );
    let mut altered = row.clone();
    altered["capture"]["context"]["settings"]["unexpected"] = json!(true);
    altered["validation"]["capture_manifest"]["context"] = altered["capture"]["context"].clone();
    assert!(evidence::binding_matches(
        &loaded.candidate_evidence,
        &altered
    ));
    assert_eq!(
        observe(&loaded, &altered, &capture, &cancel)["reason"],
        "invalid_capture"
    );
    row["status"] = json!("source_changed");
    assert_eq!(
        observe(&loaded, &row, &capture, &cancel)["reason"],
        "capture_not_bound"
    );
    Ok(())
}
