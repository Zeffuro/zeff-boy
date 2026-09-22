use super::*;
use crate::audio_discovery::{
    capture_artifact::CaptureArtifact, render::RenderOptions, validation,
};
use crate::cli::types::{HeadlessInputEvent, HeadlessOptions};
use anyhow::Result;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

pub(super) fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 16 + 0x4000 + 0x2000];
    bytes[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 1]);
    let mut entry = vec![
        0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0x40, 0x8d, 0x17, 0x40, 0xa9, 1, 0x8d, 0x15, 0x40,
        0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9, 0, 0x8d, 1, 0x40, 0xa9, 1, 0x8d, 0x16, 0x40, 0xa9, 0,
        0x8d, 0x16, 0x40, 0xad, 0x16, 0x40, 0x29, 1, 0xaa, 0x20, 0x80, 0x80, 0xa9, 8, 0x8d, 3,
        0x40,
    ];
    let end = 0x8000 + entry.len() as u16;
    entry.extend_from_slice(&[0x4c, end as u8, (end >> 8) as u8]);
    bytes[16..16 + entry.len()].copy_from_slice(&entry);
    bytes[16 + 0x80..16 + 0x87].copy_from_slice(&[0xbd, 0, 0x81, 0x8d, 2, 0x40, 0x60]);
    bytes[16 + 0x100..16 + 0x102].copy_from_slice(&[0x40, 0x80]);
    bytes[16 + 0x3ffa..16 + 0x4000].copy_from_slice(&[0, 0x80, 0, 0x80, 0, 0x80]);
    bytes
}

pub(super) fn capture(source: &[u8], button: bool) -> Result<(Value, Value, NesAudioTrace)> {
    let directory = tempfile::tempdir()?;
    let rom = directory.path().join("indexed-cues.nes");
    let path = directory.path().join("capture.zip");
    let native = directory.path().join("native.f32");
    std::fs::write(&rom, source)?;
    let cancel = AtomicBool::new(false);
    let input_events = if button {
        vec![HeadlessInputEvent {
            start_frame: 1,
            end_frame: 3,
            buttons: 1,
            dpad: 0,
            coleco_keypad: None,
            reset: false,
        }]
    } else {
        vec![]
    };
    let options = HeadlessOptions {
        max_frames: 3,
        no_sram: true,
        input_events: input_events.clone(),
        audio_trace_path: Some(path.clone()),
        audio_dump_path: Some(native.clone()),
        ..Default::default()
    };
    crate::cli::run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)?;
    let artifact = CaptureArtifact::load(&path)?;
    let playback = validation::validate(
        || artifact.session(RenderOptions::default(), &cancel),
        48_000,
        &cancel,
    )?;
    let reference = validation::reference::compare_f32_file(&native, &playback.pcm, &cancel)?;
    assert!(reference.projected_pcm_matches);
    let args = vec![
        "--audio-capture-sweep".into(),
        directory.path().join("sweep").into_os_string(),
        rom.into_os_string(),
    ];
    let loaded = super::super::input::load(&super::super::input::parse(&args)?.unwrap(), &cancel)?;
    let mut row = json!({
        "plan": "baseline", "status": "success", "requested_steps": 3,
        "applied_input": {"player_1": input_events, "press": null},
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
        super::super::evidence::reference_for_row(&loaded.candidate_evidence, &row, false);
    let trace = artifact.validated_nes_trace(&cancel)?;
    let inventory = super::super::runtime_writers::summarize(
        &loaded.candidate_evidence,
        &row,
        source,
        &trace,
        &cancel,
    );
    assert_eq!(inventory["status"], "complete", "{inventory}");
    Ok((row, inventory, trace))
}

#[test]
fn native_call_entry_index_reaches_a_rom_byte_and_sound_write() -> Result<()> {
    let source = fixture();
    let mut hashes = Vec::new();
    for button in [false, true] {
        let (row, inventory, trace) = capture(&source, button)?;
        let result = observe(&source, &trace, &row, &inventory, &AtomicBool::new(false));
        assert_eq!(result["status"], "complete", "{result}");
        assert_eq!(result["observed_writes"], 6);
        let reads = result["entry_index_reads"].as_array().unwrap();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0]["entry_pc"], 0x8080);
        assert_eq!(reads[0]["index_register"], "x");
        assert_eq!(reads[0]["entry_index"], u8::from(button));
        assert_eq!(
            reads[0]["read_source_offset"],
            16 + 0x100 + usize::from(button)
        );
        assert_eq!(reads[0]["value"], if button { 0x80 } else { 0x40 });
        assert_eq!(reads[0]["register"], 0x4002);
        let observed = result["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|observation| observation["pc"] == 0x8083)
            .unwrap();
        assert_eq!(observed["call_path"].as_array().unwrap().len(), 1);
        assert_eq!(observed["call_path"][0]["entry"]["pc"], 0x8080);
        assert_eq!(observed["call_path"][0]["entry"]["sp"], 0xfd);
        assert_eq!(observed["call_path"][0]["entry"]["x"], u8::from(button));
        assert_eq!(observed["call_path"][0]["caller_before"]["sp"], 0xff);
        hashes.push(result["verification"]["native_f32_sha256"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    Ok(())
}

#[test]
fn native_probe_refuses_changed_pcm_trace_limits_and_cancellation() -> Result<()> {
    let source = fixture();
    let (row, inventory, trace) = capture(&source, false)?;
    let mut bad = row.clone();
    bad["validation"]["native_reference"]["evidence"]["f32_sha256"] = json!("f".repeat(64));
    let failed = observe(&source, &trace, &bad, &inventory, &AtomicBool::new(false));
    assert_eq!(failed["reason"], "replay_pcm_mismatch");
    assert_eq!(failed["observations"], json!([]));
    let mut changed = trace.clone();
    changed.generation += 1;
    assert_eq!(
        observe(&source, &changed, &row, &inventory, &AtomicBool::new(false))["reason"],
        "replay_trace_mismatch"
    );
    assert_eq!(
        observe(&source, &trace, &row, &inventory, &AtomicBool::new(true))["reason"],
        "cancelled"
    );
    assert_eq!(
        run(
            &source,
            &trace,
            &row,
            &inventory,
            &AtomicBool::new(false),
            1
        )
        .unwrap_err(),
        "instruction_limit"
    );
    Ok(())
}

#[test]
fn intervening_instructions_do_not_gain_entry_index_links() -> Result<()> {
    let mut source = fixture();
    source[16 + 0x80..16 + 0x88].copy_from_slice(&[0xea, 0xbd, 0, 0x81, 0x8d, 2, 0x40, 0x60]);
    let (row, inventory, trace) = capture(&source, true)?;
    let result = observe(&source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["status"], "complete", "{result}");
    assert_eq!(result["entry_index_reads"], json!([]));
    let observed = result["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["pc"] == 0x8084)
        .unwrap();
    assert_eq!(observed["call_path"].as_array().unwrap().len(), 1);
    Ok(())
}

#[test]
fn y_indexed_load_and_store_preserve_the_observed_argument() -> Result<()> {
    let mut source = fixture();
    let transfer = source[16..16 + 0x80]
        .iter()
        .position(|byte| *byte == 0xaa)
        .unwrap();
    source[16 + transfer] = 0xa8;
    source[16 + 0x80..16 + 0x87].copy_from_slice(&[0xb9, 0, 0x81, 0x99, 1, 0x40, 0x60]);
    let (row, inventory, trace) = capture(&source, true)?;
    let result = observe(&source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["status"], "complete", "{result}");
    let reads = result["entry_index_reads"].as_array().unwrap();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0]["index_register"], "y");
    assert_eq!(reads[0]["entry_index"], 1);
    assert_eq!(reads[0]["register"], 0x4002);
    Ok(())
}

#[test]
fn input_intervals_are_inclusive_and_overlapping_masks_are_combined() {
    let inputs = schedule(&json!([
        {"start_frame": 2, "end_frame": 2, "buttons": 1, "dpad": 2, "coleco_keypad": null, "reset": false},
        {"start_frame": 2, "end_frame": 3, "buttons": 8, "dpad": 4, "coleco_keypad": null, "reset": false}
    ]), 3).unwrap();
    assert_eq!(masks(&inputs, 1), (0, 0));
    assert_eq!(masks(&inputs, 2), (9, 6));
    assert_eq!(masks(&inputs, 3), (8, 4));
    assert_eq!(masks(&inputs, 4), (0, 0));
}
