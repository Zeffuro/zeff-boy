use std::io::{Cursor, Read};

use zeff_emu_common::audio_trace::{
    AudioTrace, AudioTraceInvalidation, AudioTraceStart, AudioTraceTiming, AudioTraceWrite,
    WonderSwanAudioTrace, WonderSwanResetState, WonderSwanTraceChip,
};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::SystemHint;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::*;
use crate::audio_discovery::{
    ScanLimits,
    catalog::SongId,
    export::SongExportRequest,
    formats::SongFormat,
    media::{ScanInput, SourceIdentity, StandaloneFormat},
    render::RenderOptions,
};

pub(crate) fn sega_fixture(extension: &str) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    let program = [
        0xf3, 0x3e, 0x80, 0xd3, 0x7f, 0x3e, 0x10, 0xd3, 0x7f, 0x3e, 0x90, 0xd3, 0x7f, 0x3e, 0xe4,
        0xd3, 0x7f, 0x3e, 0xf3, 0xd3, 0x7f, 0x3e, 0x10, 0xd3, 0x06, 0x76,
    ];
    bytes[..program.len()].copy_from_slice(&program);
    if extension != "sg" {
        bytes[0x7ff0..0x7ff8].copy_from_slice(b"TMR SEGA");
        bytes[0x7fff] = if extension == "gg" { 0x6c } else { 0x4c };
    }
    bytes
}

fn context(bytes: &[u8], firmware: Option<&[u8]>) -> Value {
    json!({
        "source": {"loaded_media": {"sha256": zeff_firmware::sha256_hex(bytes), "byte_len": bytes.len()}},
        "firmware": firmware.map(|bytes| json!({"sha256": zeff_firmware::sha256_hex(bytes), "byte_len": bytes.len()})),
    })
}

pub(crate) fn sega_trace(extension: &str) -> (AudioTrace, Value) {
    let bytes = sega_fixture(extension);
    let hint = match extension {
        "gg" => SystemHint::GameGear,
        "sg" => SystemHint::Sg1000,
        _ => SystemHint::MasterSystem,
    };
    let mut emulator =
        Emulator::new_with_hint_and_video_standard(&bytes, 44_100, hint, Sega8VideoStandard::Ntsc)
            .unwrap();
    emulator.reset_and_begin_audio_trace(64).unwrap();
    emulator.step_frame();
    emulator.step_frame();
    assert!(
        emulator
            .drain_audio_samples()
            .iter()
            .any(|sample| *sample != 0.0)
    );
    (
        emulator.finish_audio_trace().unwrap(),
        context(&bytes, None),
    )
}

fn coleco_trace() -> (AudioTrace, Value) {
    let mut bios = vec![0; 8192];
    bios[..3].copy_from_slice(&[0xc3, 0x02, 0x80]);
    let mut bytes = vec![0; 0x8000];
    bytes[..2].copy_from_slice(&[0xaa, 0x55]);
    let program = [
        0xf3, 0x3e, 0x80, 0xd3, 0xe0, 0x3e, 0x10, 0xd3, 0xff, 0x3e, 0x90, 0xd3, 0xe1, 0x76,
    ];
    bytes[2..2 + program.len()].copy_from_slice(&program);
    let mut emulator = zeff_coleco_core::Emulator::new(&bytes, &bios, 44_100).unwrap();
    emulator.reset_and_begin_audio_trace(64).unwrap();
    emulator.step_frame();
    emulator.step_frame();
    let mut samples = Vec::new();
    emulator.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().any(|sample| *sample != 0.0));
    (
        emulator.finish_audio_trace().unwrap(),
        context(&bytes, Some(&bios)),
    )
}

pub(crate) fn member(archive: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn decoded_writes(bytes: &[u8]) -> (Vec<(u64, u8, u8)>, u64) {
    let mut offset = 0x100;
    let mut samples = 0u64;
    let mut writes = Vec::new();
    loop {
        match bytes[offset] {
            command @ (0x50 | 0x4f) => {
                writes.push((samples, command, bytes[offset + 1]));
                offset += 2;
            }
            0x61 => {
                samples += u64::from(u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]));
                offset += 3;
            }
            0x66 => {
                assert_eq!(offset + 1, bytes.len());
                break;
            }
            command => panic!("unexpected capture command {command:02x}"),
        }
    }
    (writes, samples)
}

#[test]
fn four_system_captures_retain_exact_events_and_enter_existing_vgm_catalog() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let cases = [
        sega_trace("sms"),
        sega_trace("gg"),
        sega_trace("sg"),
        coleco_trace(),
    ];
    for (index, (trace, context)) in cases.into_iter().enumerate() {
        let output = directory.path().join(format!("capture-{index}.zip"));
        write_new(&output, &trace, context.clone(), &AtomicBool::new(false))?;
        let archive = std::fs::read(&output)?;
        let events = member(&archive, "trace.json");
        assert_eq!(
            serde_json::from_slice::<Value>(&events)?,
            serde_json::to_value(&trace)?
        );
        let vgm = member(&archive, "capture.vgm");
        let manifest: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
        for artifact in manifest["artifacts"].as_array().unwrap() {
            let data = member(&archive, artifact["path"].as_str().unwrap());
            assert_eq!(artifact["sha256"], zeff_firmware::sha256_hex(&data));
            assert_eq!(artifact["byte_len"], data.len());
        }
        let (writes, samples) = decoded_writes(&vgm);
        let preamble = manifest["vgm"]["preamble_write_count"].as_u64().unwrap() as usize;
        let expected: Vec<_> = trace
            .events
            .iter()
            .map(|event| {
                let (command, value) = match event.write {
                    AudioTraceWrite::Sn76489 { value, .. } => (0x50, value),
                    AudioTraceWrite::GameGearStereo { value, .. } => (0x4f, value),
                };
                (
                    (u128::from(event.cycle) * 44_100 / u128::from(trace.cycle_hz)) as u64,
                    command,
                    value,
                )
            })
            .collect();
        assert_eq!(&writes[preamble..], expected);
        assert_eq!(
            samples,
            trace.end_cycle * 44_100 / u64::from(trace.cycle_hz)
        );
        let source = SourceIdentity {
            kind: "generated_vgm",
            sha256: zeff_firmware::sha256_hex(&vgm),
            len: vgm.len(),
            container: None,
            selected_member: None,
        };
        let input = ScanInput::standalone(vgm.clone(), source, StandaloneFormat::Vgm, None);
        let scan = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(scan.scan.song_count(), 1);
        assert_eq!(scan.scan.vgm_logs[0].samples, samples);
        assert!(scan.scan.vgm_logs[0].warnings.is_empty());
        let selected = scan.scan.song(SongId::Vgm(0)).unwrap();
        let mut player = crate::audio_discovery::pcm::song::PcmSong::from_ref(selected)
            .expect("captured PSG configuration is playable")
            .session(&vgm, RenderOptions::default(), &AtomicBool::new(false))?;
        assert_eq!(player.duration_frames() as u64, samples * 48_000 / 44_100);
        let mut pcm = [0; 1024];
        let mut audible = false;
        while player.position_frames() < player.duration_frames() {
            let count = player.read(&mut pcm, &AtomicBool::new(false))?;
            assert!(count > 0);
            audible |= pcm[..count].iter().any(|sample| *sample != 0);
        }
        assert!(audible);
        let retained = directory.path().join(format!("retained-{index}.zip"));
        SongExportRequest::prepare(
            &input,
            &scan,
            SongId::Vgm(0),
            SongFormat::MappedAssets,
            RenderOptions::default(),
        )?
        .write_new(&retained, &AtomicBool::new(false), &AtomicU32::new(0))?;
        assert_eq!(member(&std::fs::read(retained)?, "source.vgm"), vgm);
        assert!(write_new(&output, &trace, context, &AtomicBool::new(false)).is_err());
        assert_eq!(std::fs::read(output)?, archive);
    }
    Ok(())
}

#[test]
fn cancelled_incomplete_and_unbound_captures_publish_nothing() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let (trace, context) = sega_trace("sms");
    let output = directory.path().join("capture.zip");
    assert!(write_new(&output, &trace, context.clone(), &AtomicBool::new(true)).is_err());
    let mut incomplete = trace.clone();
    incomplete.dropped_events = 1;
    assert!(
        write_new(
            &output,
            &incomplete,
            context.clone(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    incomplete.dropped_events = 0;
    incomplete.invalidated = Some(AudioTraceInvalidation::StateRestore);
    assert!(
        write_new(
            &output,
            &incomplete,
            context.clone(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let mut outside = trace;
    outside.events[0].instruction_source = AudioTraceSource::CartridgeRom {
        offset: 0x8000,
        bit_reversed: false,
    };
    assert!(write_new(&output, &outside, context.clone(), &AtomicBool::new(false)).is_err());
    outside.events[0].instruction_source = AudioTraceSource::BootRom { offset: 0 };
    assert!(write_new(&output, &outside, context, &AtomicBool::new(false)).is_err());
    assert!(!output.exists());
    Ok(())
}

#[test]
fn wonderswan_manifest_names_wave_ram_and_bounds_dma_provenance() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("capture.zip");
    let source = [0u8];
    let trace = WonderSwanAudioTrace {
        generation: 1,
        cycle_hz: 3_072_000,
        cycle_hz_denominator: 1,
        chip: WonderSwanTraceChip {
            clock_hz: 3_072_000,
            color: true,
            reset: WonderSwanResetState::default(),
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 0,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    };

    write_new(
        &output,
        &trace,
        context(&source, None),
        &AtomicBool::new(false),
    )?;
    let manifest: Value =
        serde_json::from_slice(&member(&std::fs::read(output)?, "manifest.json"))?;
    assert_eq!(
        manifest["kind"],
        "reset_to_end_hardware_register_and_wave_ram_capture"
    );
    let limitations = manifest["limitations"].as_array().unwrap();
    assert!(limitations.iter().any(|value| {
        value.as_str().is_some_and(|text| {
            text.contains("autonomous Sound DMA have no attributed instruction")
        })
    }));
    assert!(limitations.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|text| text.contains("sample source spans are not identified"))
    }));
    assert!(limitations.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|text| text.contains("16 KiB wave RAM"))
    }));
    Ok(())
}
