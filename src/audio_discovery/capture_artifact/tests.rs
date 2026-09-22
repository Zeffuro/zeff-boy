use super::*;
use serde_json::json;

pub(crate) fn assert_native_pcm(path: &Path, native: &[u8], sample_rate: u32) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let artifact = CaptureArtifact::load(path)?;
    let options = RenderOptions {
        sample_rate,
        ..Default::default()
    };
    let expected: Vec<i16> = native
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| (f32::from_le_bytes(*bytes).clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    assert_eq!(native.len() % 8, 0);
    let mut session = artifact.session(options, &cancel)?;
    for chunk in [257, 1024] {
        session.reset()?;
        let mut actual = Vec::new();
        let mut buffer = vec![0; chunk * 2];
        loop {
            let count = session.read(&mut buffer, &cancel)?;
            if count == 0 {
                break;
            }
            actual.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(actual, expected);
    }
    Ok(())
}

fn bundle(trace: &[u8], change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut manifest = json!({"schema": "zeff-audio-trace-capture/1",
        "context": {"source": {"loaded_media": {"byte_len": 32768, "sha256": "0".repeat(64)}}},
        "artifacts": [{"path": "trace.json", "byte_len": trace.len(),
            "sha256": zeff_firmware::sha256_hex(trace)}]});
    change(&mut manifest);
    let mut bundle = crate::audio_discovery::bundle::Bundle::new();
    bundle
        .add("manifest.json", &serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    bundle.add("trace.json", trace).unwrap();
    bundle.finish().unwrap()
}

#[test]
fn integrity_checks_precede_playback_admission() {
    let trace = b"{}";
    let artifact = CaptureArtifact::from_bytes(&bundle(trace, |_| {})).unwrap();
    assert_eq!(artifact.trace_sha256, zeff_firmware::sha256_hex(trace));
    assert!(
        artifact
            .session(RenderOptions::default(), &AtomicBool::new(false))
            .is_err()
    );
    for change in [
        ("size", 0),
        ("hash", 1),
        ("missing", 2),
        ("schema", 3),
        ("source", 4),
        ("duplicate", 5),
        ("path", 6),
    ] {
        let bytes = bundle(trace, |manifest| match change.1 {
            0 => manifest["artifacts"][0]["byte_len"] = json!(999),
            1 => manifest["artifacts"][0]["sha256"] = json!("0".repeat(64)),
            2 => manifest["artifacts"] = json!([]),
            3 => manifest["schema"] = json!("future"),
            4 => manifest["context"]["source"] = Value::Null,
            5 => {
                let entry = manifest["artifacts"][0].clone();
                manifest["artifacts"].as_array_mut().unwrap().push(entry);
            }
            _ => manifest["artifacts"][0]["path"] = json!("../trace.json"),
        });
        assert!(CaptureArtifact::from_bytes(&bytes).is_err(), "{}", change.0);
    }
}

#[test]
fn native_capture_roundtrip_preserves_omitted_clock_denominator() -> Result<()> {
    let (trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    let directory = crate::test_support::test_directory("native-capture-import")?;
    let path = directory.path().join("capture.zip");
    let cancel = AtomicBool::new(false);
    crate::audio_discovery::trace_capture::write_new(&path, &trace, context, &cancel)?;
    let artifact = CaptureArtifact::load(&path)?;
    let decoded: AudioTrace = serde_json::from_slice(&artifact.trace)?;
    assert_eq!(decoded, trace);
    assert_eq!(decoded.cycle_hz_denominator, 1);
    let evidence = crate::audio_discovery::validation::validate(
        || artifact.session(RenderOptions::default(), &cancel),
        48000,
        &cancel,
    )?;
    assert!(evidence.deterministic());
    assert!(!evidence.silent);
    Ok(())
}

#[test]
fn boot_rom_events_require_a_complete_firmware_identity() -> Result<()> {
    let (mut trace, _) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    trace.events[0].instruction_source =
        zeff_emu_common::audio_trace::AudioTraceSource::BootRom { offset: 0 };
    let raw = serde_json::to_vec(&trace)?;
    for firmware in [
        Value::Null,
        json!({"byte_len": 1}),
        json!({"byte_len": 1, "sha256": "invalid"}),
        json!({"byte_len": 0, "sha256": "0".repeat(64)}),
    ] {
        let bytes = bundle(&raw, |manifest| manifest["context"]["firmware"] = firmware);
        let artifact = CaptureArtifact::from_bytes(&bytes)?;
        let error = artifact
            .session(RenderOptions::default(), &AtomicBool::new(false))
            .err()
            .unwrap();
        assert!(error.to_string().contains("firmware identity"));
    }
    let bytes = bundle(&raw, |manifest| {
        manifest["context"]["firmware"] = json!({"byte_len": 1, "sha256": "0".repeat(64)})
    });
    CaptureArtifact::from_bytes(&bytes)?
        .session(RenderOptions::default(), &AtomicBool::new(false))?;
    Ok(())
}

#[test]
fn huc6280_artifact_requires_hucard_context_before_replay() -> Result<()> {
    use zeff_emu_common::audio_trace::*;
    use zeff_pce_core::hardware::*;
    let trace = Huc6280AudioTrace {
        generation: 1,
        cycle_hz: PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR as u32,
        cycle_hz_denominator: PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR as u32,
        chip: Huc6280TraceChip {
            clock_hz_numerator: PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR,
            clock_hz_denominator: PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR as u32,
            master_clock_divisor: PSG_MASTER_CLOCK_DIVISOR as u8,
            internal_master_clock_divisor: PSG_INTERNAL_MASTER_CLOCK_DIVISOR as u8,
            revision: Huc6280TraceRevision::HuC6280,
            reset: Huc6280ResetState::default(),
        },
        timing: AudioTraceTiming::MemoryWriteCompletion,
        start: AudioTraceStart::Reset,
        end_cycle: 25_000,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    };
    let raw = serde_json::to_vec(&trace)?;
    let cancel = AtomicBool::new(false);
    for change in 0..5 {
        let bytes = bundle(&raw, |manifest| {
            let context = &mut manifest["context"];
            context["system"] = json!("pce");
            context["settings"] = json!({"arcade_card": "Disabled"});
            context["source"]["normalization"] = json!({
                "trace_rom_offsets": "loaded_media_before_header_removal"
            });
            match change {
                1 => context["system"] = json!("pce-cd"),
                2 => context["firmware"] = json!({"byte_len": 1, "sha256": "0".repeat(64)}),
                3 => context["settings"]["arcade_card"] = json!("Pro"),
                4 => context["source"]["normalization"] = Value::Null,
                _ => {}
            }
        });
        let result =
            CaptureArtifact::from_bytes(&bytes)?.session(RenderOptions::default(), &cancel);
        if change == 0 {
            assert!(result.is_ok());
        } else {
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("HuCard capture context")
            );
        }
    }
    Ok(())
}

#[test]
fn wonderswan_artifact_requires_cartridge_reset_context() -> Result<()> {
    use zeff_emu_common::audio_trace::*;
    let trace = WonderSwanAudioTrace {
        generation: 1,
        cycle_hz: 3_072_000,
        cycle_hz_denominator: 1,
        chip: WonderSwanTraceChip {
            clock_hz: 3_072_000,
            color: false,
            reset: WonderSwanResetState::default(),
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 30_720,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    };
    let raw = serde_json::to_vec(&trace)?;
    let cancel = AtomicBool::new(false);
    for change in 0..4 {
        let bytes = bundle(&raw, |manifest| {
            let context = &mut manifest["context"];
            context["system"] = json!("ws");
            context["settings"] = json!({"system_start": "cartridge_reset_without_bios",
                "minimum_system": "WonderSwan"});
            match change {
                1 => context["system"] = json!("gb"),
                2 => context["firmware"] = json!({"byte_len": 1, "sha256": "0".repeat(64)}),
                3 => context["settings"]["system_start"] = json!("unknown"),
                _ => {}
            }
        });
        let result =
            CaptureArtifact::from_bytes(&bytes)?.session(RenderOptions::default(), &cancel);
        if change == 0 {
            assert!(result.is_ok());
        } else {
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("cartridge-reset capture context")
            );
        }
    }
    for color in [false, true] {
        let mut trace = trace.clone();
        trace.chip.color = color;
        let raw = serde_json::to_vec(&trace)?;
        for (name, expected) in [
            ("WonderSwan", Some(false)),
            ("WonderSwanColor", Some(true)),
            ("Unknown(2)", Some(true)),
            ("Unknown(255)", Some(true)),
            ("Unknown(0)", None),
            ("Unknown(256)", None),
            ("", None),
        ] {
            let bytes = bundle(&raw, |manifest| {
                manifest["context"]["system"] = json!("ws");
                manifest["context"]["settings"] = json!({
                    "system_start": "cartridge_reset_without_bios", "minimum_system": name
                });
            });
            let result =
                CaptureArtifact::from_bytes(&bytes)?.session(RenderOptions::default(), &cancel);
            assert_eq!(result.is_ok(), expected == Some(color), "{color} / {name}");
        }
    }
    Ok(())
}
