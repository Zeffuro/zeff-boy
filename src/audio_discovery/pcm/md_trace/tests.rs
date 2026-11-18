use super::*;
use std::sync::atomic::Ordering;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, AudioTraceStart, AudioTraceTiming,
    MAX_AUDIO_TRACE_EVENTS, MdTraceChip, MdTraceOrigin, MdTraceRegion, MdTraceReset, MdTraceSource,
    MdTraceWrite, MdTraceWriter,
};

const ORIGIN: MdTraceOrigin = MdTraceOrigin {
    writer: MdTraceWriter::M68000,
    boundary_pc: 0x200,
    source: MdTraceSource::EffectiveRom { offset: 0x200 },
};

fn psg(value: u8) -> MdTraceWrite {
    MdTraceWrite::Psg {
        origin: ORIGIN,
        value,
    }
}

fn opn2(port: u8, value: u8) -> MdTraceWrite {
    MdTraceWrite::Opn2Port {
        origin: ORIGIN,
        port,
        value,
    }
}

fn event(cycle: u64, write: MdTraceWrite) -> AudioTraceEvent<MdTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: ORIGIN.boundary_pc,
        instruction_source: AudioTraceSource::Unknown,
        write,
    }
}

fn fixture(region: MdTraceRegion, rate: u32) -> MdAudioTrace {
    let master_hz: u64 = match region {
        MdTraceRegion::NtscUs | MdTraceRegion::NtscJapan => 53_693_175,
        MdTraceRegion::Pal => 53_203_424,
    };
    let edge = master_hz.div_ceil(u64::from(rate));
    let end_cycle = master_hz / 25 + 1;
    let mut events = vec![
        event(0, psg(0x85)),
        event(0, psg(0x06)),
        event(0, psg(0x90)),
        event(56, opn2(2, 0xb6)),
        event(56, opn2(1, 0x40)),
        event(56, opn2(0, 0x2b)),
        event(56, opn2(3, 0x80)),
        event(7, opn2(0, 0x2a)),
        event(239, psg(0xa7)),
        event(240, psg(0x03)),
        event(1007, opn2(3, 0xff)),
        event(1008, opn2(1, 0)),
        event(edge - 1, psg(0xb0)),
        event(edge, opn2(0, 0x2a)),
        event(edge, opn2(3, 0xff)),
        event(edge + 1, psg(0x92)),
        event(123_457, MdTraceWrite::Opn2Reset { origin: ORIGIN }),
        event(123_457, opn2(3, 0x12)),
        event(123_458, psg(0x94)),
        event(end_cycle, opn2(0, 0x2a)),
        event(end_cycle, opn2(1, 0xff)),
        event(end_cycle, MdTraceWrite::Opn2Reset { origin: ORIGIN }),
    ];
    events.sort_by_key(|event| event.cycle);
    MdAudioTrace {
        generation: 1,
        cycle_hz: master_hz as u32,
        cycle_hz_denominator: 1,
        chip: MdTraceChip {
            master_hz,
            region,
            psg_divisor: 240,
            opn2_divisor: 7,
            reset: MdTraceReset::ZeffPowerOnV1,
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn options(sample_rate: u32) -> RenderOptions {
    RenderOptions {
        sample_rate,
        max_seconds: 1,
        ..RenderOptions::default()
    }
}

fn source_pcm(trace: &MdAudioTrace, rate: u32) -> Result<Vec<f32>> {
    let mut replay = MdAudioReplay::new(&trace.chip, rate)?;
    let mut pcm = Vec::new();
    for event in &trace.events {
        while replay.now() < event.cycle {
            replay.advance_to((replay.now() + 7_777).min(event.cycle), &mut pcm)?;
        }
        replay.write(event.cycle, event.write, &mut pcm)?;
    }
    while replay.now() < trace.end_cycle {
        replay.advance_to((replay.now() + 7_777).min(trace.end_cycle), &mut pcm)?;
    }
    Ok(pcm)
}

fn convert(sample: &f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn read_all(session: &mut MdTraceSession, frames: usize) -> Result<(Vec<i16>, Vec<u32>)> {
    let mut pcm = Vec::new();
    let mut bits = Vec::new();
    let mut output = vec![0; frames * 2];
    loop {
        let start = session.position_frames();
        let count = session.read(&mut output, &AtomicBool::new(false))?;
        assert_eq!(session.position_frames(), start + count / 2);
        if count == 0 {
            break;
        }
        assert!(count.is_multiple_of(2) && count <= MD_AUDIO_REPLAY_MAX_FRAMES * 2);
        pcm.extend_from_slice(&output[..count]);
        bits.extend(session.floats.iter().map(|sample| sample.to_bits()));
    }
    Ok((pcm, bits))
}

#[test]
fn native_core_frames_match_all_regions_rates_chunks_and_reset() -> Result<()> {
    for region in [
        MdTraceRegion::NtscUs,
        MdTraceRegion::NtscJapan,
        MdTraceRegion::Pal,
    ] {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let trace = fixture(region, rate);
            let source = source_pcm(&trace, rate)?;
            let expected: Vec<_> = source.iter().map(convert).collect();
            let bits: Vec<_> = source.iter().map(|sample| sample.to_bits()).collect();
            assert!(expected.iter().any(|sample| *sample != 0));
            assert_eq!(
                source.len() / 2,
                (u128::from(trace.end_cycle) * u128::from(rate) / u128::from(trace.chip.master_hz))
                    as usize
            );
            let mut session =
                MdTraceSession::new(trace.clone(), options(rate), &AtomicBool::new(false))?;
            assert_eq!(session.replay.now(), 0);
            assert_eq!(session.next_event, 0);
            assert_eq!(session.duration_frames() * 2, expected.len());
            assert_eq!(session.sample_rate(), rate);
            assert_eq!(session.track_count(), 1);
            assert!(session.has_source_duration_limit());
            for frames in [1, 257, 1024, 4096] {
                session.reset()?;
                let (actual, actual_bits) = read_all(&mut session, frames)?;
                assert_eq!(actual, expected, "{region:?} {rate} {frames}");
                assert_eq!(actual_bits, bits, "f32 {region:?} {rate} {frames}");
                assert_eq!(session.replay.now(), trace.end_cycle);
                assert_eq!(session.next_event, trace.events.len());
            }
        }
    }
    Ok(())
}

#[test]
fn source_end_events_after_last_sample_are_applied_without_extra_pcm() -> Result<()> {
    let rate = 48_000;
    let mut trace = fixture(MdTraceRegion::NtscUs, rate);
    trace.end_cycle = trace.chip.master_hz.div_ceil(u64::from(rate)) + 56;
    trace.events.retain(|event| event.cycle < trace.end_cycle);
    trace.events.push(event(trace.end_cycle, psg(0x9f)));
    trace.events.push(event(
        trace.end_cycle,
        MdTraceWrite::Opn2Reset { origin: ORIGIN },
    ));
    let mut session = MdTraceSession::new(trace.clone(), options(rate), &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 1);
    let mut output = [123; 8];
    assert_eq!(session.read(&mut output, &AtomicBool::new(false))?, 2);
    assert_eq!(&output[2..], &[123; 6]);
    assert_eq!(session.next_event, trace.events.len());
    assert_eq!(session.replay.now(), trace.end_cycle);
    let before = (session.next_event, session.replay.now());
    assert_eq!(session.read(&mut output, &AtomicBool::new(false))?, 0);
    assert_eq!((session.next_event, session.replay.now()), before);
    assert_eq!(output[2..], [123; 6]);
    Ok(())
}

#[test]
fn duration_cap_stops_before_later_events_and_sparse_gaps_keep_every_frame() -> Result<()> {
    let rate = 48_000;
    let mut trace = fixture(MdTraceRegion::Pal, rate);
    trace.events.retain(|event| event.cycle == 0);
    trace.end_cycle = trace.chip.master_hz / 5 + 17;
    let expected: Vec<_> = source_pcm(&trace, rate)?.iter().map(convert).collect();
    assert!(expected.len() / 2 > 4096);
    let mut session = MdTraceSession::new(trace.clone(), options(rate), &AtomicBool::new(false))?;
    assert_eq!(read_all(&mut session, 257)?.0, expected);
    trace.end_cycle = trace.chip.master_hz * 2 + 17;
    trace
        .events
        .push(event(trace.chip.master_hz + 1, psg(0x9f)));
    let mut session = MdTraceSession::new(trace.clone(), options(rate), &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), rate as usize);
    assert!(!session.has_source_duration_limit());
    let (actual, _) = read_all(&mut session, 1024)?;
    assert_eq!(actual.len(), rate as usize * 2);
    assert_eq!(&actual[..expected.len()], expected);
    assert_eq!(session.replay.now(), trace.chip.master_hz);
    assert_eq!(session.next_event, 3);
    Ok(())
}

#[test]
fn mask_fade_empty_odd_and_initial_cancel_preserve_session_contracts() -> Result<()> {
    let rate = 48_000;
    let trace = fixture(MdTraceRegion::NtscUs, rate);
    let cancel = AtomicBool::new(false);
    let mut settings = options(rate);
    settings.fade_seconds = 1;
    let mut session = MdTraceSession::new(trace.clone(), settings, &cancel)?;
    assert!(
        session
            .warnings()
            .iter()
            .any(|warning| warning.contains("fade"))
    );
    assert_eq!(session.read(&mut [], &cancel)?, 0);
    assert!(session.read(&mut [123; 3], &cancel).is_err());
    assert!(session.read(&mut [123; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(
        (session.position, session.next_event, session.replay.now()),
        (0, 0, 0)
    );
    assert!(!session.interrupted);
    assert!(session.set_track_mask(2).is_err());
    assert_eq!(session.mask, 1);
    session.set_track_mask(0)?;
    let mut muted = [123; 514];
    assert_eq!(session.read(&mut muted, &cancel)?, muted.len());
    assert_eq!(muted, [0; 514]);
    session.reset()?;
    assert_eq!(session.mask, 0);
    session.set_track_mask(1)?;
    let (actual, _) = read_all(&mut session, 257)?;
    let expected: Vec<_> = source_pcm(&trace, rate)?
        .iter()
        .map(convert)
        .enumerate()
        .map(|(index, sample)| fade(sample, index / 2, session.duration, rate, 1))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(&actual[actual.len() - 2..], &[0, 0]);
    assert!(actual.iter().any(|sample| *sample != 0));
    Ok(())
}

#[test]
fn cancellation_checks_cover_dense_events_and_silent_tick_steps_and_poison_retries() -> Result<()> {
    for dense in [false, true] {
        let rate = 48_000;
        let mut trace = fixture(MdTraceRegion::NtscUs, rate);
        trace.events = if dense {
            vec![event(0, psg(0x9f)); 200]
        } else {
            Vec::new()
        };
        let cancel = AtomicBool::new(false);
        let mut session = MdTraceSession::new(trace, options(rate), &cancel)?;
        let mut checks = 0;
        let mut output = [123; 2048];
        let error = session.read_checked(&mut output, || {
            checks += 1;
            if checks == 12 {
                cancel.store(true, Ordering::Relaxed);
            }
            check_cancel(&cancel)
        });
        assert!(error.is_err());
        assert_eq!(output, [123; 2048]);
        assert_eq!(session.position_frames(), 0);
        assert!(session.interrupted);
        if dense {
            assert_eq!(session.next_event, 10);
            assert_eq!(session.replay.now(), 0);
        } else {
            assert_eq!(session.next_event, 0);
            assert_eq!(
                session.replay.now(),
                10 * (session.trace.chip.master_hz / 1000)
            );
        }
        cancel.store(false, Ordering::Relaxed);
        assert!(
            session
                .read(&mut output, &cancel)
                .unwrap_err()
                .to_string()
                .contains("reset")
        );
        session.reset()?;
        assert_eq!(session.read(&mut output, &cancel)?, 2048);
    }
    Ok(())
}

#[test]
fn native_validation_rejects_clock_descriptor_provenance_order_and_incomplete_traces() {
    let reject = |trace| {
        assert!(MdTraceSession::new(trace, options(48_000), &AtomicBool::new(false)).is_err());
    };
    macro_rules! reject_change {
        ($field:ident $(.$nested:ident)*, $value:expr) => {{
            let mut changed = fixture(MdTraceRegion::NtscUs, 48_000);
            changed.$field $(.$nested)* = $value;
            reject(changed);
        }};
    }
    reject_change!(cycle_hz, 1);
    reject_change!(cycle_hz_denominator, 2);
    reject_change!(chip.master_hz, 1);
    reject_change!(chip.psg_divisor, 15);
    reject_change!(chip.opn2_divisor, 8);
    reject_change!(chip.region, MdTraceRegion::Pal);
    reject_change!(timing, AudioTraceTiming::InstructionBoundary);
    reject_change!(dropped_events, 1);
    reject_change!(invalidated, Some(AudioTraceInvalidation::Reset));
    for mutation in 0..6 {
        let mut trace = fixture(MdTraceRegion::NtscUs, 48_000);
        match mutation {
            0 => trace.events[0].write = opn2(4, 0),
            1 => trace.events[0].pc += 1,
            2 => trace.events[0].instruction_source = AudioTraceSource::Unmapped,
            3 => trace.events[0].cycle = trace.end_cycle + 1,
            4 => trace.events[1].cycle = 2,
            _ => trace
                .events
                .resize(MAX_AUDIO_TRACE_EVENTS + 1, event(0, psg(0))),
        }
        reject(trace);
    }
    let mut short = fixture(MdTraceRegion::NtscUs, 48_000);
    short.events.clear();
    short.end_cycle = 56;
    reject(short);
}

#[test]
fn unsupported_options_and_prescheduled_constructor_cancel_are_refused() {
    use crate::audio_discovery::{formats::BankSelect, render::PlaybackGain};
    let trace = fixture(MdTraceRegion::NtscUs, 48_000);
    assert!(MdTraceSession::new(trace.clone(), options(48_000), &AtomicBool::new(true)).is_err());
    for mutation in 0..8 {
        let mut settings = options(48_000);
        match mutation {
            0 => settings.sample_rate = 192_000,
            1 => settings.max_seconds = 0,
            2 => settings.loops = 2,
            3 => settings.fade_seconds = 2,
            4 => settings.fade_seconds = 16,
            5 => settings.playback_gain = PlaybackGain::Mp2kAmplitude,
            6 => settings.skip_channel10 = false,
            _ => settings.bank_select = BankSelect::Mma,
        }
        assert!(MdTraceSession::new(trace.clone(), settings, &AtomicBool::new(false)).is_err());
    }
}
