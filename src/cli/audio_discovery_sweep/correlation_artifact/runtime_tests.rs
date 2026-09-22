use super::*;
use crate::audio_discovery::{render::RenderOptions, validation};
use crate::cli::types::HeadlessOptions;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

#[test]
fn indexed_runtime_writers_survive_an_empty_static_inventory() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("indirect-entry.nes");
    let capture = directory.path().join("capture.zip");
    let native = directory.path().join("native.f32");
    let mut bytes = vec![0; 16 + 0x4000 + 0x2000];
    bytes[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 1]);
    let entry = [
        0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0x40, 0x8d, 0, 2, 0xa9, 0x80, 0x8d, 1, 2, 0x6c, 0, 2,
    ];
    bytes[16..16 + entry.len()].copy_from_slice(&entry);
    let driver = [
        0xa9, 0, 0xa2, 0, 0x9d, 0, 0x40, 0xe8, 0xe0, 4, 0xd0, 0xf8, 0xa0, 0x15, 0x99, 0, 0x40,
        0x8d, 0x17, 0x40, 0x4c, 0x54, 0x80,
    ];
    bytes[80..80 + driver.len()].copy_from_slice(&driver);
    bytes[16 + 0x3ffa..16 + 0x4000].copy_from_slice(&[0, 0x80, 0, 0x80, 0, 0x80]);
    std::fs::write(&source, &bytes)?;
    let cancel = AtomicBool::new(false);
    let args = vec![
        "--audio-capture-sweep".into(),
        directory.path().join("sweep").into_os_string(),
        source.clone().into_os_string(),
    ];
    let loaded = input::load(&input::parse(&args)?.unwrap(), &cancel)?;
    assert!(
        loaded.candidate_evidence["report"]
            .get("driver_candidates")
            .is_none()
    );
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
    let reference = validation::reference::compare_f32_file(&native, &playback.pcm, &cancel)?;
    assert!(reference.projected_pcm_matches);
    let mut row = json!({
        "plan": "baseline", "status": "success", "requested_steps": 3,
        "applied_input": {"player_1": [], "press": null},
        "capture": {"archive_sha256": artifact.archive_sha256, "trace_sha256": artifact.trace_sha256,
            "context": artifact.manifest["context"]},
        "validation": {
            "status": "integrity_verified", "archive_sha256": artifact.archive_sha256,
            "trace_sha256": artifact.trace_sha256, "capture_manifest": artifact.manifest,
            "playback": {"status": "rendered", "evidence": playback},
            "native_reference": {"status": "matched", "evidence": reference}
        }
    });
    row["candidate_evidence"] =
        evidence::reference_for_row(&loaded.candidate_evidence, &row, false);
    let result = observe(&loaded, &row, &capture, &cancel);
    assert_eq!(result.correlation["reason"], "no_nrom_static_writers");
    assert_eq!(result.runtime["status"], "complete", "{}", result.runtime);
    let sites = result.runtime["sites"].as_array().unwrap();
    assert_eq!(sites.len(), 3);
    assert_eq!(sites[0]["pc"], 0x8044);
    assert_eq!(sites[0]["instruction"]["bytes"], "9d0040");
    assert_eq!(sites[0]["observed"]["count"], 4);
    let registers = sites[0]["registers"].as_array().unwrap();
    assert_eq!(
        registers
            .iter()
            .map(|register| register["address"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![0x4000, 0x4001, 0x4002, 0x4003]
    );
    assert!(
        registers
            .iter()
            .all(|register| register["observed"]["count"] == 1)
    );
    assert_eq!(sites[1]["instruction"]["bytes"], "990040");
    assert_eq!(sites[1]["observed"]["count"], 1);
    assert_eq!(sites[2]["instruction"]["bytes"], "8d1740");
    assert_eq!(sites[2]["observed"]["count"], 1);
    assert_eq!(result.runtime["observed_event_count"], 6);
    assert_eq!(result.runtime["identity"]["capture"], row["capture"]);
    Ok(())
}
