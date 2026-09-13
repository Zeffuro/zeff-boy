use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::super::super::run_headless;
use crate::audio_discovery::trace_capture::tests::member;
use crate::cli::types::{HeadlessInputEvent, HeadlessOptions};

fn fixture(cgb: bool, program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x100 + program.len()].copy_from_slice(program);
    rom[0x143] = if cgb { 0x80 } else { 0 };
    rom[0x147] = 0;
    rom[0x148] = 0;
    rom[0x149] = 0;
    rom
}

fn run(path: &Path, options: &HeadlessOptions) -> Result<()> {
    run_headless(path, HardwareModePreference::Auto, Vec::new(), options)
}

fn has_member(archive: &[u8], name: &str) -> bool {
    zip::ZipArchive::new(Cursor::new(archive)).is_ok_and(|mut zip| zip.by_name(name).is_ok())
}

#[test]
fn gb_capture_retains_trace_and_authenticated_source_without_sram_side_effects() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let bytes = fixture(
        false,
        &[
            0x3e, 0x0a, 0xea, 0x00, 0x00, 0xfa, 0x00, 0xa0, 0xea, 0x24, 0xff, 0x18, 0xf3,
        ],
    );
    let mut bytes = bytes;
    bytes[0x147] = 0x03;
    bytes[0x149] = 0x02;
    let plain = directory.path().join("trace.gb");
    let save = plain.with_extension("sav");
    std::fs::write(&plain, &bytes)?;
    std::fs::write(&save, vec![0x77; 8 * 1024])?;
    let zip = directory.path().join("trace.zip");
    let selected = "media/trace.gb";
    crate::test_support::write_zip(&zip, &[(selected, &bytes)])?;
    let zip_before = std::fs::read(&zip)?;
    let save_before = std::fs::read(&save)?;

    for (index, input) in [&plain, &zip].into_iter().enumerate() {
        let output = directory.path().join(format!("capture-{index}.zip"));
        let options = HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            max_frames: 2,
            input_events: vec![HeadlessInputEvent {
                start_frame: 1,
                end_frame: 2,
                buttons: 1,
                dpad: 2,
                coleco_keypad: None,
                reset: false,
            }],
            ..Default::default()
        };
        run(input, &options)?;
        let archive = std::fs::read(&output)?;
        let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
        let trace: Value = serde_json::from_slice(&member(&archive, "trace.json"))?;
        let context = &manifest["context"];
        assert_eq!(context["system"], "gb");
        assert_eq!(context["frames_run"], 2);
        assert_eq!(context["frame_count_unit"], "headless_steps");
        assert_eq!(context["persistent_save_files"], "not_loaded_or_written");
        assert_eq!(
            context["settings"]["reset_seed"],
            "hle_post_boot_without_firmware"
        );
        assert_eq!(
            context["settings"]["execution"]["opcode_trace_output"],
            false
        );
        assert!(context["settings"]["execution"]["opcode_logging"].is_null());
        assert_eq!(context["firmware"], Value::Null);
        assert_eq!(
            context["source"]["loaded_media"]["sha256"],
            zeff_firmware::sha256_hex(&bytes)
        );
        assert_eq!(context["input"]["player_1"], json!(options.input_events));
        assert_eq!(manifest["vgm"]["status"], "unavailable");
        assert!(
            manifest["vgm"]["unavailable"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["code"] == "post_boot_seed" })
        );
        assert!(
            trace["events"]
                .as_array()
                .is_some_and(|events| !events.is_empty())
        );
        assert!(trace["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| {
                event["write"]["register"]["address"] == 0xff24
                    && event["write"]["register"]["value"] == 0
            })
        }));
        assert!(!has_member(&archive, "capture.vgm"));
        if index == 0 {
            assert_eq!(
                context["source"]["requested_file"]["sha256"],
                zeff_firmware::sha256_hex(&bytes)
            );
        } else {
            assert_eq!(context["source"]["selected_member"]["name"], selected);
            assert_eq!(
                context["source"]["requested_file"]["sha256"],
                zeff_firmware::sha256_hex(&zip_before)
            );
        }
        assert!(run(input, &options).is_err());
        assert_eq!(std::fs::read(&output)?, archive);
    }
    assert_eq!(std::fs::read(&plain)?, bytes);
    assert_eq!(std::fs::read(&zip)?, zip_before);
    assert_eq!(std::fs::read(&save)?, save_before);
    Ok(())
}

#[test]
fn cgb_divider_and_speed_events_remain_lossless_when_vgm_is_unavailable() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let bytes = fixture(
        true,
        &[
            0xaf, 0xea, 0x04, 0xff, 0x3e, 0x01, 0xea, 0x4d, 0xff, 0x10, 0x00, 0x18, 0xfe,
        ],
    );
    let input = directory.path().join("speed.gbc");
    let output = directory.path().join("capture.zip");
    std::fs::write(&input, &bytes)?;
    run(
        &input,
        &HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            max_frames: 2,
            ..Default::default()
        },
    )?;
    let archive = std::fs::read(output)?;
    let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
    let trace: Value = serde_json::from_slice(&member(&archive, "trace.json"))?;
    let reasons = manifest["vgm"]["unavailable"].as_array().unwrap();
    for code in [
        "cgb_model",
        "post_boot_seed",
        "divider_reset",
        "speed_switch",
    ] {
        assert!(reasons.iter().any(|row| row["code"] == code), "{code}");
    }
    let events = trace["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event["write"]["divider_reset"].is_object())
    );
    assert!(
        events
            .iter()
            .any(|event| event["write"]["speed_switch"].is_object())
    );
    Ok(())
}

#[test]
fn gb_capture_rejects_master_apu_disable_without_publishing() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("muted.gb");
    let output = directory.path().join("capture.zip");
    std::fs::write(&input, fixture(false, &[0x18, 0xfe]))?;
    let error = run(
        &input,
        &HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            no_apu: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("--no-apu"), "{error:#}");
    assert!(!output.exists());
    Ok(())
}
