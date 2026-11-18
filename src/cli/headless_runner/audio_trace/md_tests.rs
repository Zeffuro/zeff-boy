use anyhow::Result;
use serde_json::Value;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, MdAudioTrace, MdAudioTraceCapture, MdTraceSource, MdTraceWrite,
};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::super::super::run_headless;
use crate::audio_discovery::trace_capture::tests::member;
use crate::cli::types::HeadlessOptions;

fn fixture(reset: bool) -> Vec<u8> {
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    let mut code = Vec::<u16>::new();
    for (value, address) in [(1, 0xa130f1), (0x6d, 0x200001)] {
        if address == 0x200001 {
            code.extend([0x1039, 0x0020, 0x0001, 0x13c0, 0x00c0, 0x0011]);
        }
        code.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    for (word, value, address) in [
        (true, 0x100, 0xa11100),
        (true, 0x100, 0xa11200),
        (false, 0x84, 0xc00011),
        (false, 0x01, 0xc00011),
        (false, 0x90, 0xc00011),
        (false, 0x2a, 0xa04000),
        (false, 0x91, 0xa04003),
        (false, 0xb6, 0xa04002),
        (false, 0xc0, 0xa04001),
    ] {
        code.extend([
            if word { 0x33fc } else { 0x13fc },
            value,
            (address >> 16) as u16,
            address as u16,
        ]);
    }
    if reset {
        code.extend([0x33fc, 0, 0x00a1, 0x1200]);
    }
    code.extend([
        0x1039, 0x00a1, 0x0003, 0x13c0, 0x00ff, 0x0000, 0x1039, 0x00a1, 0x0005, 0x13c0, 0x00ff,
        0x0001, 0x60e6,
    ]);
    for (index, word) in code.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

#[test]
fn md_headless_capture_preserves_pcm_state_video_media_and_save() -> Result<()> {
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&target)?;
    let directory = tempfile::tempdir_in(target)?;
    for (region_index, region) in [
        zeff_md_core::Region::NtscUs,
        zeff_md_core::Region::NtscJapan,
        zeff_md_core::Region::Pal,
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = fixture(false);
        let rom = directory.path().join(format!("sound-{region_index}.md"));
        std::fs::write(&rom, &bytes)?;
        let save = rom.with_extension("sav");
        let original_save = vec![0x55; 8192];
        std::fs::write(&save, &original_save)?;
        let zipped = directory.path().join(format!("sound-{region_index}.zip"));
        crate::test_support::write_zip(&zipped, &[("media/sound.md", &bytes)])?;
        let original_zip = std::fs::read(&zipped)?;
        let pcm = directory.path().join(format!("plain-{region_index}.f32"));
        let state = directory
            .path()
            .join(format!("plain-{region_index}.mdstate"));
        let image = directory.path().join(format!("plain-{region_index}.png"));
        let mut options = HeadlessOptions {
            max_frames: 3,
            md_region: Some(region),
            md_controller: Some(zeff_md_core::ControllerKind::ThreeButton),
            input_events: vec![crate::cli::types::HeadlessInputEvent {
                start_frame: 1,
                end_frame: 2,
                buttons: 1,
                dpad: 8,
                coleco_keypad: None,
                reset: false,
            }],
            input_events_p2: vec![crate::cli::types::HeadlessInputEvent {
                start_frame: 2,
                end_frame: 3,
                buttons: 4,
                dpad: 2,
                coleco_keypad: None,
                reset: false,
            }],
            no_sram: true,
            audio_dump_path: Some(pcm.clone()),
            md_save_state_path: Some(state.clone()),
            screenshot_path: Some(image.clone()),
            ..Default::default()
        };
        run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)?;
        let expected_pcm = std::fs::read(&pcm)?;
        let expected_state = std::fs::read(&state)?;
        let expected_image = std::fs::read(&image)?;
        assert!(
            expected_pcm
                .as_chunks::<4>()
                .0
                .iter()
                .any(|v| f32::from_le_bytes(*v) != 0.0)
        );
        let mut previous_trace = None;
        for (index, input) in [&rom, &zipped].into_iter().enumerate() {
            let output = directory
                .path()
                .join(format!("capture-{region_index}-{index}.zip"));
            options.audio_trace_path = Some(output.clone());
            options.audio_dump_path = Some(
                directory
                    .path()
                    .join(format!("capture-{region_index}-{index}.f32")),
            );
            options.md_save_state_path = Some(
                directory
                    .path()
                    .join(format!("capture-{region_index}-{index}.mdstate")),
            );
            options.screenshot_path = Some(
                directory
                    .path()
                    .join(format!("capture-{region_index}-{index}.png")),
            );
            options.no_sram = false;
            run_headless(input, HardwareModePreference::Auto, Vec::new(), &options)?;
            assert_eq!(
                std::fs::read(options.audio_dump_path.as_ref().unwrap())?,
                expected_pcm
            );
            assert_eq!(
                std::fs::read(options.md_save_state_path.as_ref().unwrap())?,
                expected_state
            );
            assert_eq!(
                std::fs::read(options.screenshot_path.as_ref().unwrap())?,
                expected_image
            );
            assert_eq!(std::fs::read(&save)?, original_save);
            let archive = std::fs::read(&output)?;
            crate::audio_discovery::capture_artifact::tests::assert_native_pcm(
                &output,
                &expected_pcm,
                48_000,
            )?;
            let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
            let trace_bytes = member(&archive, "trace.json");
            let trace: MdAudioTrace = serde_json::from_slice(&trace_bytes)?;
            trace.validate_complete()?;
            assert_eq!(trace.events.len(), 8);
            assert!(matches!(
                trace.events[0].write,
                MdTraceWrite::Psg { value: 0xff, .. }
            ));
            assert!(
                trace
                    .events
                    .iter()
                    .all(|event| event.instruction_source == AudioTraceSource::Unknown)
            );
            if let Some(previous) = previous_trace.replace(trace_bytes.clone()) {
                assert_eq!(previous, trace_bytes);
            }
            let context = &manifest["context"];
            assert_eq!(context["system"], "md");
            assert_eq!(context["frames_run"], 3);
            assert_eq!(context["frame_count_unit"], "pacing_fields");
            assert_eq!(context["persistent_save_files"], "not_loaded_or_written");
            assert_eq!(context["settings"]["controller"], "ThreeButton");
            assert_eq!(
                context["input"]["player_1"],
                serde_json::to_value(&options.input_events)?
            );
            assert_eq!(
                context["input"]["player_2"],
                serde_json::to_value(&options.input_events_p2)?
            );
            assert_eq!(
                context["source"]["effective_rom"]["sha256"],
                zeff_firmware::sha256_hex(&bytes)
            );
            assert_eq!(manifest["vgm"]["status"], "available");
            assert_eq!(
                context["source"]["requested_file"]["sha256"],
                zeff_firmware::sha256_hex(if index == 0 { &bytes } else { &original_zip })
            );
            if index == 1 {
                assert_eq!(
                    context["source"]["selected_member"]["name"],
                    "media/sound.md"
                );
            }
            for artifact in manifest["artifacts"].as_array().unwrap() {
                let payload = member(&archive, artifact["path"].as_str().unwrap());
                assert_eq!(artifact["byte_len"], payload.len());
                assert_eq!(artifact["sha256"], zeff_firmware::sha256_hex(&payload));
            }
            assert!(
                run_headless(input, HardwareModePreference::Auto, Vec::new(), &options).is_err()
            );
            assert_eq!(std::fs::read(&output)?, archive);
        }
        assert_eq!(std::fs::read(rom)?, bytes);
        assert_eq!(std::fs::read(zipped)?, original_zip);
    }
    Ok(())
}

#[test]
fn md_capture_requires_a_fresh_complete_run_and_distinct_outputs() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let bytes = fixture(false);
    let rom = directory.path().join("sound.md");
    std::fs::write(&rom, &bytes)?;
    let capture = directory.path().join("capture.zip");
    let mut options = HeadlessOptions {
        max_frames: 1,
        audio_trace_path: Some(capture.clone()),
        ..Default::default()
    };
    for variant in 0..5 {
        let mut rejected = HeadlessOptions {
            max_frames: 1,
            audio_trace_path: Some(capture.clone()),
            ..Default::default()
        };
        match variant {
            0 => rejected.no_apu = true,
            1 => rejected.load_state_path = Some(directory.path().join("missing.mdstate")),
            2 => rejected.max_frames = 0,
            3 => rejected.apply_mods = true,
            _ => rejected.audio_dump_path = Some(capture.clone()),
        }
        assert!(run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &rejected).is_err());
        assert!(!capture.exists());
    }
    let mut fault = bytes.clone();
    fault[0x200..0x202].copy_from_slice(&0x4afcu16.to_be_bytes());
    let fault_path = directory.path().join("fault.md");
    std::fs::write(&fault_path, fault)?;
    options.audio_trace_path = Some(directory.path().join("fault.zip"));
    assert!(
        run_headless(
            &fault_path,
            HardwareModePreference::Auto,
            Vec::new(),
            &options
        )
        .is_err()
    );
    assert!(!options.audio_trace_path.as_ref().unwrap().exists());
    assert_eq!(std::fs::read(rom)?, bytes);
    Ok(())
}

#[test]
fn md_capture_refuses_vgm_reset_and_invalid_native_identity_without_losing_events() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let bytes = fixture(true);
    let rom = directory.path().join("reset.md");
    std::fs::write(&rom, &bytes)?;
    let output = directory.path().join("reset.zip");
    let options = HeadlessOptions {
        max_frames: 1,
        audio_trace_path: Some(output.clone()),
        ..Default::default()
    };
    run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)?;
    let archive = std::fs::read(output)?;
    let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
    let trace: MdAudioTrace = serde_json::from_slice(&member(&archive, "trace.json"))?;
    assert!(matches!(
        trace.events.last().unwrap().write,
        MdTraceWrite::Opn2Reset { .. }
    ));
    assert_eq!(manifest["vgm"]["status"], "unavailable");
    assert_eq!(manifest["vgm"]["unavailable"][0]["code"], "opn2_reset");
    assert_eq!(manifest["artifacts"].as_array().unwrap().len(), 1);
    let capture = MdAudioTraceCapture {
        effective_rom_len: bytes.len() as u32,
        effective_rom_sha256: zeff_firmware::sha256_bytes(&bytes),
        trace,
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let rejected = directory.path().join("rejected.zip");
    for variant in 0..5 {
        let mut invalid = capture.clone();
        match variant {
            0 => invalid.effective_rom_sha256[0] ^= 1,
            1 => invalid.effective_rom_len -= 1,
            2 => invalid.trace.dropped_events = 1,
            3 => {
                let event = &mut invalid.trace.events[0];
                if let MdTraceWrite::Psg { origin, .. } = &mut event.write {
                    origin.source = MdTraceSource::EffectiveRom {
                        offset: invalid.effective_rom_len,
                    };
                    origin.boundary_pc = invalid.effective_rom_len;
                    event.pc = origin.boundary_pc;
                }
            }
            _ => cancel.store(true, std::sync::atomic::Ordering::Relaxed),
        }
        let error = crate::audio_discovery::trace_capture::write_md_new(
            &rejected,
            &invalid,
            manifest["context"].clone(),
            &cancel,
        )
        .unwrap_err();
        if variant == 3 {
            assert!(
                error.to_string().contains("exceeds effective ROM"),
                "{error:#}"
            );
        }
        assert!(!rejected.exists());
    }
    Ok(())
}
