use anyhow::Result;
use serde_json::Value;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::super::super::run_headless;
use crate::audio_discovery::trace_capture::tests::member;
use crate::cli::types::HeadlessOptions;

fn fixture() -> Vec<u8> {
    let mut rom = vec![0; 0x400];
    rom[..4].copy_from_slice(&0xea00_002eu32.to_le_bytes());
    rom[0xb2] = 0x96;
    let stores = [
        (0x0400_0084u32, 0x80u32, 0xe1c0_10b0u32),
        (0x0400_0082, 0x0304, 0xe1c0_10b0),
        (0x0400_00a0, 0x7f01_80ff, 0xe580_1000),
        (0x0400_0100, 0x0080_fe00, 0xe580_1000),
    ];
    let pool = 0xc0 + stores.len() * 12 + 4;
    for (index, (address, value, opcode)) in stores.into_iter().enumerate() {
        let pc = 0xc0 + index * 12;
        let literal = pool + index * 8;
        for (offset, instruction) in [
            (0, 0xe59f_0000 | (literal - pc - 8) as u32),
            (4, 0xe59f_1000 | (literal + 4 - pc - 12) as u32),
            (8, opcode),
        ] {
            rom[pc + offset..pc + offset + 4].copy_from_slice(&instruction.to_le_bytes());
        }
        rom[literal..literal + 4].copy_from_slice(&address.to_le_bytes());
        rom[literal + 4..literal + 8].copy_from_slice(&value.to_le_bytes());
    }
    rom[pool - 4..pool].copy_from_slice(&0xeaff_fffeu32.to_le_bytes());
    rom
}

#[test]
fn gba_fifo_capture_preserves_audio_and_binds_raw_and_zip_inputs() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let bytes = fixture();
    let rom = directory.path().join("feed.gba");
    std::fs::write(&rom, &bytes)?;
    let zip = directory.path().join("feed.zip");
    crate::test_support::write_zip(&zip, &[("media/feed.gba", &bytes)])?;
    let plain = directory.path().join("plain.f32");
    let mut options = HeadlessOptions {
        max_frames: 2,
        no_sram: true,
        audio_dump_path: Some(plain.clone()),
        ..Default::default()
    };
    run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)?;
    let expected_pcm = std::fs::read(plain)?;
    assert!(expected_pcm.iter().any(|&byte| byte != 0));
    for (index, input) in [&rom, &zip].into_iter().enumerate() {
        let output = directory.path().join(format!("capture-{index}.zip"));
        let pcm = directory.path().join(format!("capture-{index}.f32"));
        options.audio_trace_path = Some(output.clone());
        options.audio_dump_path = Some(pcm.clone());
        options.no_sram = false;
        run_headless(input, HardwareModePreference::Auto, Vec::new(), &options)?;
        assert_eq!(std::fs::read(pcm)?, expected_pcm);
        let archive = std::fs::read(&output)?;
        let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
        assert_eq!(manifest["schema"], "zeff-gba-fifo-capture/1");
        assert_eq!(manifest["context"]["system"], "gba");
        assert_eq!(manifest["context"]["frames_run"], 2);
        assert_eq!(
            manifest["context"]["source"]["loaded_media"]["sha256"],
            zeff_firmware::sha256_hex(&bytes)
        );
        let a = member(&archive, "fifo-a.s8");
        assert!(a.len() > 4);
        assert_eq!(&a[..4], &[0xff, 0x80, 0x01, 0x7f]);
        assert!(a[4..].iter().all(|&byte| byte == 0));
        assert!(member(&archive, "fifo-b.s8").is_empty());
        let feed: Value = serde_json::from_slice(&member(&archive, "feed.json"))?;
        assert_eq!(feed["pops"].as_array().unwrap().len(), a.len());
        assert!(run_headless(input, HardwareModePreference::Auto, Vec::new(), &options).is_err());
        assert_eq!(std::fs::read(output)?, archive);
    }
    assert_eq!(std::fs::read(rom)?, bytes);
    Ok(())
}

#[test]
fn gba_fifo_capture_rejects_partial_and_muted_runs() {
    let mut options = HeadlessOptions {
        audio_trace_path: Some("capture.zip".into()),
        ..Default::default()
    };
    assert!(super::validate_system("gba", &options).is_ok());
    options.no_apu = true;
    assert!(super::validate_system("gba", &options).is_err());
    options.no_apu = false;
    options.gba_audio_mutes[4] = true;
    assert!(super::validate_system("gba", &options).is_err());
    options.gba_audio_mutes[4] = false;
    options.break_on_gba_bad_state = true;
    assert!(super::validate_system("gba", &options).is_err());
    options.break_on_gba_bad_state = false;
    options
        .input_events
        .push(crate::cli::types::HeadlessInputEvent {
            start_frame: 1,
            end_frame: 1,
            buttons: 1,
            dpad: 0,
            coleco_keypad: None,
            reset: false,
        });
    assert!(super::validate_system("gba", &options).is_err());
}
