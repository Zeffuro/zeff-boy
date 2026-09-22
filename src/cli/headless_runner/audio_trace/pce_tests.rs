use super::*;
use crate::audio_discovery::trace_capture::tests::member;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn fixture() -> Vec<u8> {
    let mut program = vec![0x78, 0xD4, 0xA9, 0xFF, 0x53, 1, 0xA9, 0xF8, 0x53, 2];
    let mut write = |register, value| program.extend([0xA9, value, 0x8D, register, 0x08]);
    write(0, 0);
    write(1, 0xFF);
    write(2, 0x80);
    write(3, 1);
    write(4, 0);
    write(5, 0xFF);
    for value in 0..32 {
        write(6, value);
    }
    write(15, 0xA5);
    write(0, 7);
    write(6, 0xFF);
    write(0, 0);
    write(4, 0x9F);
    program.extend([0x80, 0xFE]);
    let mut image = vec![0xEA; 0x2000];
    image[..program.len()].copy_from_slice(&program);
    image[0x1FFE..].copy_from_slice(&0xE000u16.to_le_bytes());
    image
}

fn run(path: &Path, options: &HeadlessOptions) -> Result<()> {
    super::super::run_headless(path, HardwareModePreference::Auto, Vec::new(), options)
}

#[test]
fn hucard_capture_preserves_pcm_and_binds_header_and_five_player_inputs() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let raw = fixture();
    let mut headered = vec![0; 512];
    headered[0] = 1;
    headered.extend_from_slice(&raw);
    let input = crate::cli::types::HeadlessInputEvent {
        start_frame: 1,
        end_frame: 2,
        buttons: 1,
        dpad: 0,
        coleco_keypad: None,
        reset: false,
    };
    let mut original_vgm = None;
    for (index, bytes) in [&raw, &headered].into_iter().enumerate() {
        let path = directory.path().join(format!("fixture-{index}.pce"));
        std::fs::write(&path, bytes)?;
        let zip_path = directory.path().join(format!("fixture-{index}.zip"));
        crate::test_support::write_zip(&zip_path, &[("game/fixture.pce", bytes)])?;
        for (zipped, source) in [false, true].into_iter().zip([&path, &zip_path]) {
            let output = directory
                .path()
                .join(format!("capture-{index}-{zipped}.zip"));
            let pcm = directory.path().join(format!("audio-{index}-{zipped}.f32"));
            let plain_pcm = directory.path().join(format!("plain-{index}-{zipped}.f32"));
            let mut options = HeadlessOptions {
                max_frames: 2,
                audio_dump_path: Some(plain_pcm.clone()),
                no_sram: true,
                pce_controller_mode: Some(zeff_pce_core::hardware::PceControllerMode::Multitap),
                input_events: vec![input],
                input_events_p2: vec![input],
                input_events_p3: vec![input],
                input_events_p4: vec![input],
                input_events_p5: vec![input],
                ..Default::default()
            };
            run(source, &options)?;
            options.audio_trace_path = Some(output.clone());
            options.audio_dump_path = Some(pcm.clone());
            options.no_sram = false;
            run(source, &options)?;
            let samples = std::fs::read(pcm)?;
            assert!(samples.iter().any(|&byte| byte != 0));
            assert_eq!(samples, std::fs::read(plain_pcm)?);
            crate::audio_discovery::capture_artifact::tests::assert_native_pcm(
                &output, &samples, 44_100,
            )?;
            let archive = std::fs::read(&output)?;
            let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
            let trace: Value = serde_json::from_slice(&member(&archive, "trace.json"))?;
            let context = &manifest["context"];
            assert_eq!(context["system"], "pce");
            assert_eq!(context["frame_count_unit"], "headless_steps");
            assert_eq!(context["settings"]["published_video_frames"], 2);
            assert_eq!(
                context["source"]["loaded_media"]["sha256"],
                zeff_firmware::sha256_hex(bytes)
            );
            assert_eq!(
                context["source"]["normalization"]["header_bytes"],
                index * 512
            );
            for player in 1..=5 {
                assert_eq!(context["input"][format!("player_{player}")], json!([input]));
            }
            assert_eq!(trace["cycle_hz"], 236_250_000);
            assert_eq!(trace["cycle_hz_denominator"], 11);
            assert_eq!(trace["timing"], "memory_write_completion");
            let events = trace["events"].as_array().unwrap();
            assert_eq!(events.len(), 43);
            assert_eq!(events[38]["write"]["register"], 15);
            assert_eq!(events[38]["write"]["value"], 0xA5);
            for event in events {
                assert_eq!(
                    event["instruction_source"]["cartridge_rom"]["offset"]
                        .as_u64()
                        .unwrap(),
                    event["pc"].as_u64().unwrap() - 0xE000 + (index * 512) as u64
                );
            }
            let vgm = member(&archive, "capture.vgm");
            if let Some(previous) = original_vgm.replace(vgm.clone()) {
                assert_eq!(previous, vgm);
            }
            assert!(run(source, &options).is_err());
            assert_eq!(archive, std::fs::read(output)?);
        }
        assert_eq!(*bytes, std::fs::read(path)?);
    }
    Ok(())
}

#[test]
fn long_block_capture_reports_video_frames_separately_from_input_steps() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut bytes = fixture();
    bytes[..18].copy_from_slice(&[
        0x78, 0xA9, 0xFF, 0x53, 1, 0xA9, 0xF8, 0x53, 2, 0xD3, 0, 0xE1, 1, 8, 0, 0, 0x80, 0xFE,
    ]);
    let path = directory.path().join("long-block.pce");
    std::fs::write(&path, bytes)?;
    let output = directory.path().join("capture.zip");
    run(
        &path,
        &HeadlessOptions {
            max_frames: 1,
            no_apu: true,
            audio_trace_path: Some(output.clone()),
            ..Default::default()
        },
    )?;
    let archive = std::fs::read(output)?;
    let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
    let context = &manifest["context"];
    assert_eq!(context["frames_run"], 1);
    assert!(
        context["settings"]["published_video_frames"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert_eq!(context["frame_count_unit"], "headless_steps");
    assert_eq!(context["sample_generation"], false);
    assert_eq!(manifest["vgm"]["guest_write_count"], 65_536);
    Ok(())
}

#[test]
fn hucard_capture_rejects_cd_and_overflow_without_publication() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("capture.zip");
    let mut options = HeadlessOptions {
        audio_trace_path: Some(output.clone()),
        max_frames: 100,
        no_apu: true,
        ..Default::default()
    };
    let cd = directory.path().join("missing.cue");
    std::fs::write(
        &cd,
        "FILE missing.bin BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n",
    )?;
    assert!(
        run(&cd, &options)
            .unwrap_err()
            .to_string()
            .contains("HuCard")
    );
    assert!(!output.exists());
    let mut bytes = fixture();
    bytes[..13].copy_from_slice(&[
        0x78, 0xD4, 0xA9, 0xFF, 0x53, 1, 0x8D, 0, 8, 0x4C, 6, 0xE0, 0xEA,
    ]);
    let path = directory.path().join("overflow.pce");
    std::fs::write(&path, &bytes)?;
    options.max_frames = 100;
    assert!(
        run(&path, &options)
            .unwrap_err()
            .to_string()
            .contains("lost")
    );
    assert!(!output.exists());
    assert_eq!(bytes, std::fs::read(path)?);
    Ok(())
}
