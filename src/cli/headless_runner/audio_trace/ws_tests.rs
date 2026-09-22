use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::super::run_headless;
use crate::audio_discovery::trace_capture::tests::member;
use crate::cli::types::{HeadlessInputEvent, HeadlessOptions};

fn fixture(minimum_system: u8) -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[..10].copy_from_slice(&[
        0xB0, 0x03, // mov al, 03h
        0xE6, 0x88, // out 88h, al
        0xB0, 0x0F, // mov al, 0Fh
        0xE6, 0x90, // out 90h, al
        0xEB, 0xF6, // jmp 0000h
    ]);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 1] = minimum_system;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = 0x02;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn run(path: &Path, options: &HeadlessOptions) -> Result<()> {
    run_headless(path, HardwareModePreference::Auto, Vec::new(), options)
}

#[test]
fn ws_and_wsc_captures_are_atomic_and_preserve_source_and_save_sidecars() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let input = HeadlessInputEvent {
        start_frame: 1,
        end_frame: 2,
        buttons: 1,
        dpad: 2,
        coleco_keypad: None,
        reset: false,
    };
    for (extension, minimum_system) in [("ws", 0), ("wsc", 1)] {
        let bytes = fixture(minimum_system);
        let rom = directory.path().join(format!("fixture.{extension}"));
        let sidecar = rom.with_extension("sav");
        std::fs::write(&rom, &bytes)?;
        std::fs::write(&sidecar, vec![0xA7; 32 * 1024])?;
        let zipped = directory.path().join(format!("fixture-{extension}.zip"));
        let selected = format!("game/fixture.{extension}");
        crate::test_support::write_zip(&zipped, &[(selected.as_str(), &bytes)])?;
        let zip_before = std::fs::read(&zipped)?;
        let sidecar_before = std::fs::read(&sidecar)?;
        let mut previous_vgm = None;
        let native_path = directory.path().join(format!("native-{extension}.f32"));
        run(
            &rom,
            &HeadlessOptions {
                max_frames: 2,
                no_sram: true,
                audio_dump_path: Some(native_path.clone()),
                input_events: vec![input],
                ..Default::default()
            },
        )?;
        let native = std::fs::read(native_path)?;
        assert!(native.iter().any(|&byte| byte != 0));

        for (index, source) in [&rom, &zipped].into_iter().enumerate() {
            let output = directory
                .path()
                .join(format!("capture-{extension}-{index}.zip"));
            let options = HeadlessOptions {
                audio_trace_path: Some(output.clone()),
                max_frames: 2,
                no_apu: index == 1,
                input_events: vec![input],
                ..Default::default()
            };
            run(source, &options)?;
            crate::audio_discovery::capture_artifact::tests::assert_native_pcm(
                &output,
                &native,
                zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE,
            )?;

            let archive = std::fs::read(&output)?;
            let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
            let trace: Value = serde_json::from_slice(&member(&archive, "trace.json"))?;
            let context = &manifest["context"];
            assert_eq!(context["system"], "ws");
            assert_eq!(context["frames_run"], 2);
            assert_eq!(context["frame_count_unit"], "headless_steps");
            assert_eq!(context["settings"]["headless_step_count"], 2);
            assert_eq!(context["settings"]["published_video_frames"], 2);
            assert_eq!(
                context["settings"]["minimum_system"],
                if minimum_system == 0 {
                    json!("WonderSwan")
                } else {
                    json!("WonderSwanColor")
                }
            );
            assert_eq!(
                context["settings"]["system_start"],
                "cartridge_reset_without_bios"
            );
            assert_eq!(context["input"]["player_1"], json!([input]));
            assert_eq!(context["sample_generation"], !options.no_apu);
            assert_eq!(context["persistent_save_files"], "not_loaded_or_written");
            assert_eq!(
                context["source"]["loaded_media"]["sha256"],
                zeff_firmware::sha256_hex(&bytes)
            );
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
            assert!(!trace["events"].as_array().unwrap().is_empty());
            let vgm = member(&archive, "capture.vgm");
            assert!(!vgm.is_empty());
            if let Some(previous) = previous_vgm.replace(vgm.clone()) {
                assert_eq!(previous, vgm);
            }
            assert!(run(source, &options).is_err());
            assert_eq!(std::fs::read(&output)?, archive);
            assert_eq!(std::fs::read(&sidecar)?, sidecar_before);
        }
        assert_eq!(std::fs::read(&rom)?, bytes);
        assert_eq!(std::fs::read(&zipped)?, zip_before);
    }
    Ok(())
}

#[test]
fn ws_capture_labels_long_dma_steps_and_execution_modes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut bytes = fixture(1);
    let mut program = vec![0x90; 20_000];
    for (port, value) in [(0x42, 3), (0x46, 0xfe), (0x47, 0xff), (0x48, 0x80)] {
        program.extend_from_slice(&[0xb0, value, 0xe6, port]);
    }
    program.push(0xf4);
    bytes[..program.len()].copy_from_slice(&program);
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&bytes);
    let end = bytes.len();
    bytes[end - 2..].copy_from_slice(&checksum.to_le_bytes());
    let rom = directory.path().join("multi-frame-dma.wsc");
    std::fs::write(&rom, bytes)?;
    for mode in 0..3 {
        let output = directory.path().join(format!("capture-{mode}.zip"));
        let options = HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            max_frames: 1,
            debug_state_path: (mode == 1).then(|| directory.path().join("state.json")),
            trace_opcodes: mode == 2,
            trace_opcode_limit: 1,
            input_events: vec![HeadlessInputEvent {
                start_frame: 1,
                end_frame: 1,
                buttons: 1,
                dpad: 0,
                coleco_keypad: None,
                reset: false,
            }],
            ..Default::default()
        };
        run(&rom, &options)?;
        let archive = std::fs::read(output)?;
        let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
        let trace: Value = serde_json::from_slice(&member(&archive, "trace.json"))?;
        let context = &manifest["context"];
        assert_eq!(context["frames_run"], 1);
        assert_eq!(context["frame_count_unit"], "headless_steps");
        assert_eq!(context["settings"]["headless_step_count"], 1);
        assert_eq!(context["settings"]["published_video_frames"], 1);
        assert!(
            trace["end_cycle"].as_u64().unwrap()
                > u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2
        );
        assert_eq!(context["input"]["player_1"], json!(options.input_events));
        assert!(
            context["input"]["frame_intervals"]
                .as_str()
                .unwrap()
                .starts_with("inclusive_headless_step")
        );
        assert_eq!(
            context["settings"]["execution"],
            json!({
                "stepping": if mode == 2 { "instruction_loop" } else { "frame_loop" },
                "opcode_logging": mode != 0,
            })
        );
    }
    Ok(())
}

#[test]
fn unsupported_ws_audio_paths_publish_no_partial_bundle() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for (name, program, expected) in [
        (
            "hyper-voice",
            vec![0xb0, 1, 0xe6, 0x69, 0xf4],
            "HyperVoice I/O",
        ),
        ("fast-sweep", vec![0xb0, 2, 0xe6, 0x95, 0xf4], "fast-sweep"),
        (
            "hyper-dma",
            vec![0xb0, 1, 0xe6, 0x4e, 0xb0, 0x90, 0xe6, 0x52, 0xf4],
            "Sound DMA targeted HyperVoice",
        ),
    ] {
        let mut bytes = fixture(1);
        bytes[..program.len()].copy_from_slice(&program);
        let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&bytes);
        let end = bytes.len();
        bytes[end - 2..].copy_from_slice(&checksum.to_le_bytes());
        let rom = directory.path().join(format!("{name}.wsc"));
        let output = directory.path().join(format!("{name}.zip"));
        std::fs::write(&rom, &bytes)?;
        let options = HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            max_frames: 1,
            ..Default::default()
        };
        let error = run(&rom, &options).unwrap_err();
        assert!(error.to_string().contains(expected), "{name}: {error:#}");
        assert!(!output.exists());
        assert_eq!(std::fs::read(rom)?, bytes);
    }
    Ok(())
}

#[test]
fn linked_ws_capture_is_rejected_before_sidecar_access_or_publication() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let rom = directory.path().join("linked.ws");
    let sidecar = rom.with_extension("sav");
    let output = directory.path().join("capture.zip");
    let bytes = fixture(0);
    let sidecar_before = vec![0x5A; 32 * 1024];
    std::fs::write(&rom, &bytes)?;
    std::fs::write(&sidecar, &sidecar_before)?;
    let options = HeadlessOptions {
        audio_trace_path: Some(output.clone()),
        ws_link_peer_path: Some("same".into()),
        max_frames: 2,
        ..Default::default()
    };
    let error = run(&rom, &options).unwrap_err();
    assert!(error.to_string().contains("linked WonderSwan"), "{error:#}");
    assert!(!output.exists());
    assert_eq!(std::fs::read(&sidecar)?, sidecar_before);
    assert_eq!(std::fs::read(&rom)?, bytes);
    Ok(())
}
