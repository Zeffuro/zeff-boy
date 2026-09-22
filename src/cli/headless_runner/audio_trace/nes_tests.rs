use anyhow::Result;
use serde_json::Value;
use zeff_emu_common::audio_trace::{AudioTraceSource, NesAudioTrace, NesTraceWrite};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::super::super::run_headless;
use crate::audio_discovery::trace_capture::tests::member;
use crate::cli::types::HeadlessOptions;

fn fixture(region: u8) -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x8000];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 2;
    rom[6] = 2;
    rom[7] = 8;
    rom[10] = 0x70;
    rom[12] = region;
    let mut code = vec![0x78];
    for (address, value) in [
        (0x4017_u16, 0x40_u8),
        (0x4010, 0x0f),
        (0x4011, 0x40),
        (0x4012, 0),
        (0x4013, 0x20),
        (0x4015, 0x10),
    ] {
        code.extend([0xa9, value, 0x8d, address as u8, (address >> 8) as u8]);
    }
    let target = 0x8000 + code.len() as u16;
    code.extend([0xad, 0x15, 0x40, 0x4c, target as u8, (target >> 8) as u8]);
    rom[16..16 + code.len()].copy_from_slice(&code);
    for (index, byte) in rom[16 + 0x4000..16 + 0x6000].iter_mut().enumerate() {
        *byte = index as u8 ^ 0xaa;
    }
    for offset in [0x7ffa, 0x7ffc, 0x7ffe] {
        rom[16 + offset..16 + offset + 2].copy_from_slice(&0x8000_u16.to_le_bytes());
    }
    rom
}

#[test]
fn nes_headless_capture_preserves_pcm_source_and_saves() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for region in [0, 1, 3] {
        let bytes = fixture(region);
        let rom = directory.path().join(format!("fixture-{region}.nes"));
        std::fs::write(&rom, &bytes)?;
        let save = rom.with_extension("sav");
        std::fs::write(&save, vec![0x55; 8192])?;
        let zip = directory.path().join(format!("fixture-{region}.zip"));
        crate::test_support::write_zip(&zip, &[("media/source.nes", &bytes)])?;
        let plain_pcm = directory.path().join(format!("plain-{region}.f32"));
        let mut options = HeadlessOptions {
            max_frames: 3,
            no_sram: true,
            audio_dump_path: Some(plain_pcm.clone()),
            ..Default::default()
        };
        run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)?;
        let expected_pcm = std::fs::read(&plain_pcm)?;
        assert!(!expected_pcm.is_empty());
        for (index, input) in [&rom, &zip].into_iter().enumerate() {
            let capture = directory
                .path()
                .join(format!("capture-{region}-{index}.zip"));
            let pcm = directory
                .path()
                .join(format!("capture-{region}-{index}.f32"));
            options.audio_trace_path = Some(capture.clone());
            options.audio_dump_path = Some(pcm.clone());
            options.no_sram = false;
            run_headless(input, HardwareModePreference::Auto, Vec::new(), &options)?;
            assert_eq!(std::fs::read(pcm)?, expected_pcm);
            assert_eq!(std::fs::read(&save)?, vec![0x55; 8192]);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut session =
                crate::audio_discovery::capture_artifact::CaptureArtifact::load(&capture)?
                    .session(
                        crate::audio_discovery::render::RenderOptions::default(),
                        &cancel,
                    )?;
            let mut replayed = Vec::new();
            let mut buffer = [0; 514];
            loop {
                let count = session.read(&mut buffer, &cancel)?;
                if count == 0 {
                    break;
                }
                replayed.extend_from_slice(&buffer[..count]);
            }
            let expected: Vec<i16> = expected_pcm
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| (f32::from_le_bytes(*bytes).clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            assert_eq!(replayed, expected);
            let archive = std::fs::read(capture)?;
            let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
            let trace: NesAudioTrace = serde_json::from_slice(&member(&archive, "trace.json"))?;
            trace.validate_complete()?;
            assert_eq!(manifest["context"]["system"], "nes");
            assert_eq!(manifest["context"]["frames_run"], 3);
            assert_eq!(
                manifest["context"]["source"]["loaded_media"]["sha256"],
                zeff_firmware::sha256_hex(&bytes)
            );
            assert_eq!(manifest["vgm"]["status"], "unavailable");
            assert!(
                manifest["vgm"]["unavailable"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason["code"] == "dmc_fetch")
            );
            if region == 0 && index == 0 {
                let mut unknown_source = trace.clone();
                for event in &mut unknown_source.events {
                    if let NesTraceWrite::DmcFetch { source, .. } = &mut event.write {
                        *source = AudioTraceSource::Unknown;
                    }
                }
                let retained = directory.path().join("unknown-dmc-source.zip");
                crate::audio_discovery::trace_capture::write_nes_new(
                    &retained,
                    &unknown_source,
                    manifest["context"].clone(),
                    &cancel,
                )?;
                let retained = std::fs::read(retained)?;
                let saved: NesAudioTrace =
                    serde_json::from_slice(&member(&retained, "trace.json"))?;
                assert_eq!(saved, unknown_source);
                let saved: Value = serde_json::from_slice(&member(&retained, "manifest.json"))?;
                assert_eq!(saved["vgm"]["status"], "unavailable");
                assert_eq!(saved["artifacts"].as_array().unwrap().len(), 1);
            }
            let mut fetched = 0;
            for event in trace.events {
                if let NesTraceWrite::DmcFetch {
                    value,
                    source:
                        AudioTraceSource::CartridgeRom {
                            offset,
                            bit_reversed,
                        },
                    ..
                } = event.write
                {
                    assert!(!bit_reversed);
                    assert_eq!(value, bytes[offset as usize]);
                    fetched += 1;
                }
            }
            assert!(fetched > 0);
        }
    }
    Ok(())
}

#[test]
fn nes_capture_rejects_incomplete_timelines_and_expansion_boards() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("unsupported.nes");
    let mut bytes = fixture(0);
    bytes[6] = 0x80;
    bytes[7] = 0x10;
    std::fs::write(&path, &bytes)?;
    let mut options = HeadlessOptions {
        max_frames: 1,
        audio_trace_path: Some(directory.path().join("capture.zip")),
        ..Default::default()
    };
    assert!(run_headless(&path, HardwareModePreference::Auto, Vec::new(), &options).is_err());
    assert!(!options.audio_trace_path.as_ref().unwrap().exists());
    options.no_apu = true;
    assert!(super::validate_system("nes", &options).is_err());
    options.no_apu = false;
    options.expect_test_pass = true;
    assert!(super::validate_system("nes", &options).is_err());
    Ok(())
}

#[test]
fn nes_audio_outputs_cannot_replace_the_rom_or_each_other() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("input.nes");
    let bytes = fixture(0);
    std::fs::write(&input, &bytes)?;
    let capture = directory.path().join("capture.zip");
    let mut options = HeadlessOptions {
        max_frames: 1,
        audio_dump_path: Some(directory.path().join(".").join("input.nes")),
        audio_trace_path: Some(capture.clone()),
        ..Default::default()
    };
    assert!(run_headless(&input, HardwareModePreference::Auto, Vec::new(), &options).is_err());
    assert_eq!(std::fs::read(&input)?, bytes);
    assert!(!capture.exists());
    options.audio_dump_path = Some(directory.path().join(".").join("capture.zip"));
    assert!(run_headless(&input, HardwareModePreference::Auto, Vec::new(), &options).is_err());
    assert!(!capture.exists());
    options.audio_dump_path = None;
    options.screenshot_path = Some(directory.path().join(".").join("capture.zip"));
    assert!(run_headless(&input, HardwareModePreference::Auto, Vec::new(), &options).is_err());
    assert!(!capture.exists());
    Ok(())
}

#[test]
fn nes_headless_vgm_preserves_register_timeline_and_native_pcm() -> Result<()> {
    for region in [0, 1] {
        verify_vgm(region)?;
    }
    Ok(())
}

fn verify_vgm(region: u8) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut bytes = fixture(region);
    let mut code = vec![0x78];
    for (address, value) in [
        (0x4017_u16, 0x40_u8),
        (0x4011, 0x20),
        (0x4015, 1),
        (0x4000, 0xbf),
        (0x4002, 0xfe),
        (0x4003, 8),
    ] {
        code.extend([0xa9, value, 0x8d, address as u8, (address >> 8) as u8]);
    }
    let target = 0x8000 + code.len() as u16;
    code.extend([0xad, 0x15, 0x40, 0x4c, target as u8, (target >> 8) as u8]);
    bytes[16..16 + code.len()].copy_from_slice(&code);
    let input = directory.path().join("tone.nes");
    std::fs::write(&input, &bytes)?;
    let pcm = directory.path().join("plain.f32");
    let capture = directory.path().join("capture.zip");
    let mut options = HeadlessOptions {
        max_frames: 3,
        no_sram: true,
        audio_dump_path: Some(pcm.clone()),
        ..Default::default()
    };
    run_headless(&input, HardwareModePreference::Auto, Vec::new(), &options)?;
    let plain = std::fs::read(pcm)?;
    let traced_pcm = directory.path().join("traced.f32");
    options.audio_dump_path = Some(traced_pcm.clone());
    options.audio_trace_path = Some(capture.clone());
    run_headless(&input, HardwareModePreference::Auto, Vec::new(), &options)?;
    assert_eq!(std::fs::read(traced_pcm)?, plain);
    let archive = std::fs::read(&capture)?;
    let metadata: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
    assert_eq!(metadata["vgm"]["status"], "available");
    assert_eq!(metadata["vgm"]["unavailable"], serde_json::json!([]));
    let trace: NesAudioTrace = serde_json::from_slice(&member(&archive, "trace.json"))?;
    let vgm = member(&archive, "capture.vgm");
    assert_eq!(
        u32::from_le_bytes(vgm[0x84..0x88].try_into().unwrap()),
        if region == 0 { 1_789_773 } else { 1_662_607 }
    );
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let scanned = zeff_audio_discovery::vgm::scan(&vgm, Default::default(), &cancel);
    assert_eq!(scanned.vgm_logs.len(), 1);
    assert!(scanned.vgm_logs[0].warnings.is_empty());
    let mut at = 0x100;
    let mut sample = 0_u64;
    let mut writes = Vec::new();
    loop {
        match vgm[at] {
            0xb4 => {
                writes.push((sample, u16::from(vgm[at + 1]) + 0x4000, vgm[at + 2]));
                at += 3;
            }
            0x61 => {
                sample += u64::from(u16::from_le_bytes([vgm[at + 1], vgm[at + 2]]));
                at += 3;
            }
            0x66 => break,
            command => panic!("unexpected VGM command {command:02x}"),
        }
    }
    assert_eq!(at + 1, vgm.len());
    let expected: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| {
            if let NesTraceWrite::Register { address, value, .. } = event.write {
                let sample =
                    (u128::from(event.cycle) * 44_100 * u128::from(trace.cycle_hz_denominator)
                        / u128::from(trace.cycle_hz)) as u64;
                Some((sample, address, value))
            } else {
                None
            }
        })
        .collect();
    let preamble = metadata["vgm"]["capture"]["preamble_write_count"]
        .as_u64()
        .unwrap() as usize;
    assert_eq!(&writes[preamble..], expected);
    assert_eq!(sample, scanned.vgm_logs[0].samples);
    let mut session = crate::audio_discovery::capture_artifact::CaptureArtifact::load(&capture)?
        .session(
            crate::audio_discovery::render::RenderOptions::default(),
            &cancel,
        )?;
    let mut replayed = Vec::new();
    let mut buffer = [0; 514];
    loop {
        let count = session.read(&mut buffer, &cancel)?;
        if count == 0 {
            break;
        }
        replayed.extend_from_slice(&buffer[..count]);
    }
    let expected: Vec<i16> = plain
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| (f32::from_le_bytes(*bytes).clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    assert_eq!(replayed, expected);
    assert!(expected.iter().any(|sample| *sample != 0));
    Ok(())
}
