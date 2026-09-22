use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, MAX_AUDIO_TRACE_EVENTS,
};
use zeff_sega8_core::hardware::cartridge::SystemHint;

use super::*;

fn program(ti: bool, stereo: bool) -> Vec<u8> {
    let mut bytes = vec![0xf3];
    for (index, value) in [
        0x80, 0x00, 0x90, 0x81, 0x02, 0xa7, 0x01, 0xb4, 0xc3, 0x03, 0xd2, 0xe7, 0xf3, 0x84, 0x00,
        0xe4, 0x98, 0xa1, 0x00, 0xe0, 0x90,
    ]
    .into_iter()
    .enumerate()
    {
        let port = if ti {
            0xe0 + index as u8
        } else {
            0x40 + index as u8
        };
        bytes.extend([0x3e, value, 0xd3, port, 0x06, 17, 0x10, 0xfe]);
        if index % 5 == 0 && stereo {
            bytes.extend([0x3e, [0xf0, 0x0f, 0xff][index % 3], 0xd3, 6]);
        }
    }
    bytes.extend([0xc3, 1, 0]);
    bytes
}

fn capture(
    hint: Option<SystemHint>,
    video: Sega8VideoStandard,
    rate: u32,
) -> (AudioTrace, Vec<f32>) {
    let mut output = Vec::new();
    if let Some(hint) = hint {
        let mut rom = vec![0; 0x10000];
        let program = program(false, hint == SystemHint::GameGear);
        rom[..program.len()].copy_from_slice(&program);
        let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint_and_video_standard(
            &rom, rate, hint, video,
        )
        .unwrap();
        emu.reset_and_begin_audio_trace(8192).unwrap();
        for _ in 0..3 {
            emu.step_frame();
            output.extend(emu.drain_audio_samples());
        }
        (emu.finish_audio_trace().unwrap(), output)
    } else {
        let mut bios = vec![0; zeff_coleco_core::constants::BIOS_SIZE];
        let program = program(true, false);
        bios[..program.len()].copy_from_slice(&program);
        let mut rom = vec![0; 0x8000];
        rom[..2].copy_from_slice(&[0xaa, 0x55]);
        let mut emu = zeff_coleco_core::emulator::Emulator::new(&rom, &bios, rate).unwrap();
        emu.reset_and_begin_audio_trace(8192).unwrap();
        for _ in 0..3 {
            emu.step_frame();
            output.extend(emu.drain_audio_samples());
        }
        (emu.finish_audio_trace().unwrap(), output)
    }
}

fn read_all(session: &mut SnTraceSession, size: usize) -> (Vec<i16>, Vec<u32>) {
    let mut output = Vec::new();
    let mut floats = Vec::new();
    let mut buffer = vec![0; size];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
        floats.extend(session.floats.iter().map(|value| value.to_bits()));
    }
    (output, floats)
}

fn base_trace() -> AudioTrace {
    AudioTrace {
        generation: 1,
        cycle_hz: Sega8VideoStandard::Ntsc.clock_hz_approx(),
        cycle_hz_denominator: 1,
        chip: Model::Sega.contract(Sega8VideoStandard::Ntsc.clock_hz_approx(), false),
        timing: AudioTraceTiming::InstructionBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 1000,
        events: vec![event(0, 0x90)],
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(cycle: u64, value: u8) -> AudioTraceEvent {
    AudioTraceEvent {
        cycle,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write: AudioTraceWrite::Sn76489 { port: 0x7f, value },
    }
}

#[test]
fn native_emulator_pcm_is_exact_across_models_rates_chunks_and_reset() -> Result<()> {
    for hint in [
        Some(SystemHint::MasterSystem),
        Some(SystemHint::GameGear),
        Some(SystemHint::Sg1000),
        None,
    ] {
        for video in [Sega8VideoStandard::Ntsc, Sega8VideoStandard::Pal] {
            if hint.is_none() && video == Sega8VideoStandard::Pal {
                continue;
            }
            for sample_rate in [44_100, 48_000, 63_072, 96_000] {
                let (trace, floats) = capture(hint, video, sample_rate);
                assert!(
                    trace.events.len() > 100,
                    "{hint:?} {video:?} {sample_rate}: {} events through {} cycles",
                    trace.events.len(),
                    trace.end_cycle
                );
                let expected: Vec<_> = floats
                    .iter()
                    .map(|value| (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                    .collect();
                let float_bits: Vec<_> = floats.iter().map(|value| value.to_bits()).collect();
                assert!(expected.iter().any(|value| *value != 0));
                let mut session = SnTraceSession::new(
                    trace.clone(),
                    RenderOptions {
                        sample_rate,
                        max_seconds: 1,
                        ..RenderOptions::default()
                    },
                    &AtomicBool::new(false),
                )?;
                for size in [2, 74, 2048, 8192] {
                    session.reset()?;
                    let (actual, bits) = read_all(&mut session, size);
                    assert_eq!(actual.len(), expected.len());
                    assert!(
                        actual == expected,
                        "{hint:?} {video:?} {sample_rate} {size}"
                    );
                    assert!(
                        bits == float_bits,
                        "f32 {hint:?} {video:?} {sample_rate} {size}"
                    );
                    assert_eq!(session.position_frames(), session.duration_frames());
                }
                let mut fresh = SnTraceSession::new(
                    trace,
                    RenderOptions {
                        sample_rate,
                        max_seconds: 1,
                        ..RenderOptions::default()
                    },
                    &AtomicBool::new(false),
                )?;
                assert_eq!(read_all(&mut fresh, 190).0, expected);
            }
        }
    }
    Ok(())
}

#[test]
fn same_cycle_writes_and_exact_sample_boundaries_keep_their_order() -> Result<()> {
    for sample_rate in [44_100, 48_000, 63_072, 96_000] {
        let mut trace = base_trace();
        let first = u64::from(trace.cycle_hz).div_ceil(u64::from(sample_rate));
        trace.events = vec![event(0, 0x9f), event(0, 0x90), event(first, 0x9f)];
        trace.end_cycle = first * 2;
        let mut session = SnTraceSession::new(
            trace,
            RenderOptions {
                sample_rate,
                ..RenderOptions::default()
            },
            &AtomicBool::new(false),
        )?;
        let (samples, _) = read_all(&mut session, 2);
        assert_eq!(samples.len(), 4);
        assert!(samples[..2].iter().all(|sample| *sample > 0));
        assert_eq!(samples[2..], [0, 0]);
    }
    Ok(())
}

#[test]
fn preserving_writes_while_quantizing_cycles_can_change_the_native_waveform() -> Result<()> {
    let (trace, source) = capture(Some(SystemHint::GameGear), Sega8VideoStandard::Ntsc, 48_000);
    let mut quantized = trace.clone();
    let clock = u64::from(trace.cycle_hz);
    for event in &mut quantized.events {
        event.cycle = (event.cycle * 44_100 / clock * clock).div_ceil(44_100);
    }
    assert!(
        trace
            .events
            .iter()
            .zip(&quantized.events)
            .all(|(left, right)| left.write == right.write
                && left.instruction_source == right.instruction_source)
    );
    let expected: Vec<_> = source
        .iter()
        .map(|value| (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect();
    let cancel = AtomicBool::new(false);
    let options = RenderOptions::default();
    let mut native = SnTraceSession::new(trace, options, &cancel)?;
    let mut projected = SnTraceSession::new(quantized, options, &cancel)?;
    let (native, _) = read_all(&mut native, 190);
    let (projected, _) = read_all(&mut projected, 190);
    assert_eq!(native, expected);
    assert_eq!(projected.len(), expected.len());
    assert_ne!(projected, expected);
    Ok(())
}

#[test]
fn all_reset_model_clock_and_event_contract_fields_are_checked() {
    let reject = |trace| {
        assert!(
            SnTraceSession::new(trace, RenderOptions::default(), &AtomicBool::new(false)).is_err()
        );
    };
    macro_rules! reject_change {
        ($field:ident $(.$nested:ident)*, $value:expr) => {{
            let mut trace = base_trace();
            trace.$field $(.$nested)* = $value;
            reject(trace);
        }};
    }
    reject_change!(cycle_hz, 0);
    reject_change!(cycle_hz_denominator, 0);
    reject_change!(cycle_hz_denominator, 2);
    reject_change!(chip.clock_hz, 3_579_545);
    reject_change!(chip.feedback_mask, 3);
    reject_change!(chip.shift_register_width, 15);
    reject_change!(chip.zero_period, Sn76489ZeroPeriod::Period1024);
    reject_change!(chip.period_one_constant_high, false);
    reject_change!(chip.tone_counter_clock_divider, 8);
    reject_change!(chip.noise_tone2_clock, Sn76489Tone2NoiseClock::RisingEdge);
    reject_change!(chip.noise_output_high_when_lfsr_bit_zero, false);
    reject_change!(chip.reset.tone_periods, [1; 3]);
    reject_change!(chip.reset.volumes, [0; 4]);
    reject_change!(chip.reset.noise_control, 1);
    reject_change!(chip.reset.stereo_control, 0);
    reject_change!(chip.reset.latched_register, 1);
    reject_change!(chip.reset.noise_lfsr, 0x4000);
    reject_change!(chip.reset.tone_output_high, [false; 3]);
    reject_change!(chip.reset.tone_clocks_remaining, [15; 3]);
    reject_change!(chip.reset.noise_clocks_remaining, 16);
    reject_change!(timing, AudioTraceTiming::BusServiceBoundary);
    reject_change!(timing, AudioTraceTiming::IoWriteCompletion);
    reject_change!(dropped_events, 1);
    reject_change!(invalidated, Some(AudioTraceInvalidation::Reset));
    reject_change!(events, vec![event(10, 0), event(9, 0)]);
    reject_change!(events, vec![event(1001, 0)]);
    reject_change!(events, vec![event(0, 0); MAX_AUDIO_TRACE_EVENTS + 1]);
    reject_change!(end_cycle, 0);
    let mut trace = base_trace();
    trace.events[0].write = AudioTraceWrite::GameGearStereo { port: 6, value: 0 };
    reject(trace.clone());
    trace.chip.stereo = true;
    trace.events[0].write = AudioTraceWrite::GameGearStereo { port: 7, value: 0 };
    reject(trace);
    let mut trace = base_trace();
    trace.events[0].write = AudioTraceWrite::Sn76489 {
        port: 0xe0,
        value: 0,
    };
    reject(trace);
}

#[test]
fn limits_mute_fade_cancellation_and_long_clock_arithmetic_are_bounded() -> Result<()> {
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    let mut trace = base_trace();
    trace.end_cycle = u64::MAX;
    let mut session = SnTraceSession::new(trace.clone(), options, &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 48_000);
    assert!(session.has_source_duration_limit());
    assert!(SnTraceSession::new(trace.clone(), options, &AtomicBool::new(true)).is_err());
    assert!(session.read(&mut [0; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.read(&mut [0; 3], &AtomicBool::new(false)).is_err());
    assert!(session.set_track_mask(2).is_err());
    let (expected, _) = read_all(&mut session, 2048);
    session.reset()?;
    session.set_track_mask(0)?;
    let mut silent = [1; 74];
    assert_eq!(session.read(&mut silent, &AtomicBool::new(false))?, 74);
    assert_eq!(silent, [0; 74]);
    session.set_track_mask(1)?;
    assert_eq!(read_all(&mut session, 2048).0, expected[74..]);
    let mut faded = SnTraceSession::new(
        trace,
        RenderOptions {
            fade_seconds: 1,
            ..options
        },
        &AtomicBool::new(false),
    )?;
    let (faded, _) = read_all(&mut faded, 2048);
    assert_eq!(&faded[faded.len() - 2..], &[0, 0]);
    assert_ne!(faded, expected);
    Ok(())
}
