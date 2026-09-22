use super::*;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, Huc6280TraceChip, Huc6280TraceWrite,
};

fn event(cycle: u64, register: u8, value: u8) -> AudioTraceEvent<Huc6280TraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: 0xe000,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0,
            bit_reversed: false,
        },
        write: Huc6280TraceWrite {
            physical_address: 0x1f_e800 | u32::from(register),
            register,
            value,
        },
    }
}

fn trace(revision: Huc6280TraceRevision) -> Huc6280AudioTrace {
    let mut events = vec![event(0, 0, 0), event(0, 1, 0xff), event(0, 5, 0xff)];
    events.extend((0..32).map(|index| event(3 + index * 3, 6, (index * 19) as u8 & 31)));
    events.extend([
        event(120, 2, 0),
        event(123, 3, 8),
        event(126, 4, 0x9f),
        event(40_000, 2, 0x34),
        event(40_003, 3, 2),
        event(80_000, 0, 4),
        event(80_003, 7, 0x87),
        event(80_006, 4, 0x9f),
        event(120_000, 8, 3),
        event(120_003, 9, 0x81),
        event(180_000, 9, 0),
    ]);
    Huc6280AudioTrace {
        generation: 1,
        cycle_hz: (PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR / 8) as u32,
        cycle_hz_denominator: (PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR / 8) as u32,
        chip: Huc6280TraceChip {
            clock_hz_numerator: PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR,
            clock_hz_denominator: PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR as u32,
            master_clock_divisor: PSG_MASTER_CLOCK_DIVISOR as u8,
            internal_master_clock_divisor: PSG_INTERNAL_MASTER_CLOCK_DIVISOR as u8,
            revision,
            reset: Huc6280ResetState::default(),
        },
        timing: AudioTraceTiming::MemoryWriteCompletion,
        start: AudioTraceStart::Reset,
        end_cycle: 500_000,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn source_pcm(trace: &Huc6280AudioTrace, rate: u32, drain_ticks: u64) -> Vec<f32> {
    let mut chip = new_chip(trace.chip.revision, rate);
    let mut output = Vec::new();
    let mut scratch = Vec::new();
    let mut cycle = 0;
    let mut next_drain = drain_ticks;
    for event in &trace.events {
        while cycle < event.cycle {
            let target = event.cycle.min(next_drain);
            chip.advance_master_ticks(target - cycle);
            cycle = target;
            if cycle == next_drain {
                chip.drain_audio_samples_into(&mut scratch);
                output.extend_from_slice(&scratch);
                next_drain += drain_ticks;
            }
        }
        chip.write_port(
            PsgPort::from_offset(event.write.register),
            event.write.value,
        );
    }
    while cycle < trace.end_cycle {
        let target = trace.end_cycle.min(next_drain);
        chip.advance_master_ticks(target - cycle);
        cycle = target;
        if cycle == next_drain {
            chip.drain_audio_samples_into(&mut scratch);
            output.extend_from_slice(&scratch);
            next_drain += drain_ticks;
        }
    }
    chip.drain_audio_samples_into(&mut scratch);
    output.extend_from_slice(&scratch);
    output
}

fn read_all(session: &mut Huc6280TraceSession, size: usize) -> (Vec<i16>, Vec<u32>) {
    let mut output = Vec::new();
    let mut bits = Vec::new();
    let mut buffer = vec![0; size];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
        bits.extend(session.floats.iter().map(|sample| sample.to_bits()));
    }
    (output, bits)
}

#[test]
fn native_psg_pcm_matches_both_revisions_rates_chunks_and_reset() -> Result<()> {
    for revision in [
        Huc6280TraceRevision::HuC6280,
        Huc6280TraceRevision::HuC6280A,
    ] {
        for sample_rate in [44_100, 48_000, 63_072, 96_000] {
            let trace = trace(revision);
            let source = source_pcm(&trace, sample_rate, 7_777);
            let expected: Vec<_> = source
                .iter()
                .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                .collect();
            let bits: Vec<_> = source.iter().map(|sample| sample.to_bits()).collect();
            assert!(expected.iter().any(|sample| *sample != 0));
            let options = RenderOptions {
                sample_rate,
                max_seconds: 1,
                ..RenderOptions::default()
            };
            let mut session =
                Huc6280TraceSession::new(trace.clone(), options, &AtomicBool::new(false))?;
            assert_eq!(session.duration_frames() * 2, expected.len());
            for chunk in [2, 74, 2048, 8192] {
                session.reset()?;
                let (actual, actual_bits) = read_all(&mut session, chunk);
                assert_eq!(actual, expected, "{revision:?} {sample_rate} {chunk}");
                assert_eq!(actual_bits, bits, "f32 {revision:?} {sample_rate} {chunk}");
            }
            let mut fresh = Huc6280TraceSession::new(trace, options, &AtomicBool::new(false))?;
            assert_eq!(read_all(&mut fresh, 190).0, expected);
        }
    }
    Ok(())
}

#[test]
fn reduced_serialized_clock_matches_the_canonical_ratio() -> Result<()> {
    for sample_rate in [44_100, 48_000, 63_072, 96_000] {
        let trace = trace(Huc6280TraceRevision::HuC6280A);
        let mut canonical = trace.clone();
        canonical.cycle_hz = PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR as u32;
        canonical.cycle_hz_denominator = PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR as u32;
        let options = RenderOptions {
            sample_rate,
            max_seconds: 1,
            ..RenderOptions::default()
        };
        let mut serialized = Huc6280TraceSession::new(trace, options, &AtomicBool::new(false))?;
        let mut unreduced = Huc6280TraceSession::new(canonical, options, &AtomicBool::new(false))?;
        assert_eq!(read_all(&mut serialized, 74), read_all(&mut unreduced, 74));
    }
    Ok(())
}

#[test]
fn reset_clock_and_address_contracts_are_checked() {
    let reject = |trace| {
        assert!(
            Huc6280TraceSession::new(trace, RenderOptions::default(), &AtomicBool::new(false))
                .is_err()
        );
    };
    macro_rules! reject_change {
        ($field:ident $(.$nested:ident)*, $value:expr) => {{
            let mut changed = trace(Huc6280TraceRevision::HuC6280);
            changed.$field $(.$nested)* = $value;
            reject(changed);
        }};
    }
    reject_change!(cycle_hz, 1);
    reject_change!(cycle_hz_denominator, 1);
    reject_change!(chip.clock_hz_numerator, 1);
    reject_change!(chip.clock_hz_denominator, 1);
    reject_change!(chip.master_clock_divisor, 1);
    reject_change!(chip.internal_master_clock_divisor, 1);
    reject_change!(chip.reset.attenuation_latch, 0);
    reject_change!(timing, AudioTraceTiming::InstructionBoundary);
    reject_change!(dropped_events, 1);
    reject_change!(invalidated, Some(AudioTraceInvalidation::Reset));
    let mut changed = trace(Huc6280TraceRevision::HuC6280);
    changed.chip.reset.channels[0].noise_seed = 2;
    reject(changed);
    let mut changed = trace(Huc6280TraceRevision::HuC6280);
    changed.events[0].write.physical_address = 0x1f_e7ff;
    reject(changed);
    let mut changed = trace(Huc6280TraceRevision::HuC6280);
    changed.events[0].write.register = 1;
    reject(changed);
    let mut changed = trace(Huc6280TraceRevision::HuC6280);
    changed.events[0].pc = 0x1_0000;
    reject(changed);
    let mut changed = trace(Huc6280TraceRevision::HuC6280);
    changed.events[0].cycle = changed.end_cycle + 1;
    reject(changed);
}

#[test]
fn mute_cancel_and_bounded_long_traces_preserve_the_native_clock() -> Result<()> {
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    let mut long = trace(Huc6280TraceRevision::HuC6280);
    long.end_cycle = u64::MAX;
    let mut session = Huc6280TraceSession::new(long.clone(), options, &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 48_000);
    assert!(!session.has_source_duration_limit());
    assert!(Huc6280TraceSession::new(long, options, &AtomicBool::new(true)).is_err());
    assert!(session.read(&mut [0; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.read(&mut [0; 3], &AtomicBool::new(false)).is_err());
    assert!(session.set_track_mask(2).is_err());
    session.set_track_mask(0)?;
    let mut muted = [1; 74];
    assert_eq!(session.read(&mut muted, &AtomicBool::new(false))?, 74);
    assert_eq!(muted, [0; 74]);
    session.reset()?;
    session.set_track_mask(1)?;
    let mut expected = [0; 74];
    assert_eq!(session.read(&mut expected, &AtomicBool::new(false))?, 74);
    assert!(expected.iter().any(|sample| *sample != 0));
    Ok(())
}
