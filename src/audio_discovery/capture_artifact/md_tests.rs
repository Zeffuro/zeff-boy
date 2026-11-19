use std::sync::atomic::AtomicU32;

use super::*;
use serde_json::json;
use zeff_emu_common::audio_trace::*;
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

mod fixture;

fn capture(region: Region, rate: u32) -> Result<(MdAudioTraceCapture, Vec<f32>, Value)> {
    let rom = fixture::rom();
    let mut machine = Emulator::new(
        rom.clone(),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.set_sample_rate(rate);
    machine.reset_and_begin_audio_trace(4096)?;
    assert_eq!(machine.timing_snapshot().now().get(), 56);
    let mut pcm = Vec::new();
    for field in 1..=3 {
        let end = field * region.frame_ticks();
        let mut steps = 0;
        while machine.timing_snapshot().now().get() < end {
            machine.step_instruction().unwrap();
            steps += 1;
            assert!(steps < 200_000);
        }
        assert_eq!(machine.sound_fault(), None);
        assert_eq!(machine.peek(0xa00700), Some(field as u8));
        assert_ne!(machine.peek(0xa00701), Some(0xff));
        machine.drain_audio_samples_into(&mut pcm);
    }
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| *sample != 0.0));
    let capture = machine.finish_audio_trace().unwrap();
    assert!(
        capture
            .trace
            .events
            .iter()
            .any(|event| matches!(event.write, MdTraceWrite::Opn2Reset { .. }))
    );
    assert!(capture.trace.events.iter().any(|event| matches!(
        event.write, MdTraceWrite::Psg { origin, .. }
            if origin.writer == MdTraceWriter::M68000
    )));
    assert!(capture.trace.events.iter().any(|event| matches!(
        event.write, MdTraceWrite::Opn2Port { origin, .. }
            if origin.writer == MdTraceWriter::Z80
    )));
    let context = json!({
        "system": "md", "firmware": null,
        "settings": {"region": format!("{region:?}"), "sample_rate": rate},
        "source": {
            "loaded_media": {"byte_len": rom.len(), "sha256": zeff_firmware::sha256_hex(&rom)},
            "effective_rom": {"byte_len": capture.effective_rom_len,
                "sha256": const_hex::encode(capture.effective_rom_sha256),
                "address_space": "effective_core_rom"},
        },
    });
    Ok((capture, pcm, context))
}

fn round_trip(region: Region, rate: u32) -> Result<()> {
    let (capture, native, context) = capture(region, rate)?;
    let directory = tempfile::tempdir()?;
    let archive = directory.path().join("capture.zip");
    let cancel = AtomicBool::new(false);
    crate::audio_discovery::trace_capture::write_md_new(&archive, &capture, context, &cancel)?;
    let bytes: Vec<_> = native
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    super::tests::assert_native_pcm(&archive, &bytes, rate)?;
    let artifact = CaptureArtifact::load(&archive)?;
    assert_eq!(artifact.manifest["vgm"]["status"], "unavailable");
    let options = RenderOptions {
        sample_rate: rate,
        ..Default::default()
    };
    let evidence = crate::audio_discovery::validation::validate(
        || artifact.session(options, &cancel),
        rate as usize,
        &cancel,
    )?;
    assert!(evidence.deterministic());
    assert!(!evidence.silent);
    assert!(!evidence.validation_duration_capped);
    assert_eq!(evidence.pcm.frames * 2, native.len());
    assert!(
        artifact
            .excerpt_intervals(options, &evidence, &cancel)?
            .is_none()
    );
    let writers = artifact.writer_evidence(options, &cancel)?;
    assert_eq!(writers["status"], "unsupported");
    assert_eq!(writers["system"], "md");
    assert_eq!(writers["trace_sha256"], artifact.trace_sha256);
    let output = directory.path().join("capture.wav");
    let progress = AtomicU32::new(0);
    crate::audio_discovery::pcm::write_new(
        artifact.session(options, &cancel)?,
        crate::audio_discovery::formats::AudioFormat::Wav,
        options,
        json!({}),
        &output,
        &cancel,
        &progress,
    )?;
    let wav = std::fs::read(&output)?;
    let mut decoded = hound::WavReader::new(Cursor::new(&wav))?;
    let expected: Vec<i16> = native
        .iter()
        .map(|value| (value.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    assert_eq!(decoded.spec().sample_rate, rate);
    assert_eq!(
        decoded
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    assert_eq!(progress.load(std::sync::atomic::Ordering::Relaxed), 100);
    assert!(
        crate::audio_discovery::pcm::write_new(
            artifact.session(options, &cancel)?,
            crate::audio_discovery::formats::AudioFormat::Wav,
            options,
            json!({}),
            &output,
            &cancel,
            &progress,
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&output)?, wav);
    Ok(())
}

#[test]
fn md_native_capture_round_trip() -> Result<()> {
    round_trip(Region::NtscUs, 48_000)
}

#[test]
fn md_native_capture_regional_rate_matrix() -> Result<()> {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            if region != Region::NtscUs || rate != 48_000 {
                round_trip(region, rate)?;
            }
        }
    }
    Ok(())
}

#[test]
fn md_import_rejects_inconsistent_identity_context_and_native_trace() -> Result<()> {
    let (capture, _, context) = capture(Region::NtscUs, 48_000)?;
    let cancel = AtomicBool::new(false);
    for variant in 0..17 {
        let mut trace = capture.trace.clone();
        let mut context = context.clone();
        match variant {
            0 => context["system"] = json!("sms"),
            1 => context["settings"]["region"] = json!("Pal"),
            2 => context["firmware"] = json!({"byte_len": 1, "sha256": "0".repeat(64)}),
            3 => context["source"]["effective_rom"]["byte_len"] = json!(0),
            4 => context["source"]["effective_rom"]["sha256"] = json!("bad"),
            5 => context["source"]["effective_rom"]["address_space"] = json!("zip_archive"),
            6 => context["source"]["effective_rom"]["byte_len"] = json!(8),
            7 => trace.dropped_events = 1,
            8 => trace.invalidated = Some(AudioTraceInvalidation::Reset),
            9 => trace.cycle_hz_denominator = 2,
            10 => trace.chip.psg_divisor = 15,
            11 => trace.events[0].pc ^= 2,
            12 => trace.events[0].instruction_source = AudioTraceSource::Unmapped,
            13 => trace.events[0].cycle = trace.end_cycle + 1,
            14 => trace.events.swap(0, 1),
            15 => {
                trace.events[0].write = MdTraceWrite::Opn2Port {
                    origin: MdTraceOrigin {
                        writer: MdTraceWriter::M68000,
                        boundary_pc: trace.events[0].pc,
                        source: MdTraceSource::Unknown,
                    },
                    port: 4,
                    value: 0,
                }
            }
            _ => trace.timing = AudioTraceTiming::MemoryWriteCompletion,
        }
        let bytes = super::tests::bundle(&serde_json::to_vec(&trace)?, |manifest| {
            manifest["context"] = context;
        });
        let artifact = CaptureArtifact::from_bytes(&bytes)?;
        assert!(
            artifact.session(Default::default(), &cancel).is_err(),
            "variant {variant}"
        );
        assert!(
            artifact
                .writer_evidence(Default::default(), &cancel)
                .is_err(),
            "variant {variant}"
        );
    }
    let bytes = super::tests::bundle(&serde_json::to_vec(&capture.trace)?, |manifest| {
        manifest["context"] = context;
    });
    let artifact = CaptureArtifact::from_bytes(&bytes)?;
    let cancelled = AtomicBool::new(true);
    assert!(artifact.session(Default::default(), &cancelled).is_err());
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("cancelled.wav");
    assert!(
        crate::audio_discovery::pcm::write_new(
            artifact.session(Default::default(), &cancel)?,
            crate::audio_discovery::formats::AudioFormat::Wav,
            Default::default(),
            json!({}),
            &output,
            &cancelled,
            &AtomicU32::new(0),
        )
        .is_err()
    );
    assert!(!output.exists());
    Ok(())
}
