use super::*;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, WonderSwanTraceChip,
};

fn event(cycle: u64, write: WonderSwanTraceWrite) -> AudioTraceEvent<WonderSwanTraceWrite> {
    let origin = match write {
        WonderSwanTraceWrite::Register { origin, .. }
        | WonderSwanTraceWrite::WaveRam { origin, .. } => origin,
    };
    let (pc, instruction_source) = if matches!(
        origin,
        WonderSwanTraceOrigin::CpuInterrupt | WonderSwanTraceOrigin::SoundDma
    ) {
        (0, AudioTraceSource::Unknown)
    } else {
        (
            0xf0000,
            AudioTraceSource::CartridgeRom {
                offset: 0,
                bit_reversed: false,
            },
        )
    };
    AudioTraceEvent {
        cycle,
        pc,
        instruction_source,
        write,
    }
}

fn register(
    cycle: u64,
    port: u16,
    value: u8,
    origin: WonderSwanTraceOrigin,
) -> AudioTraceEvent<WonderSwanTraceWrite> {
    event(
        cycle,
        WonderSwanTraceWrite::Register {
            port,
            value,
            origin,
        },
    )
}

fn wave_ram(
    cycle: u64,
    address: u16,
    value: u8,
    origin: WonderSwanTraceOrigin,
) -> AudioTraceEvent<WonderSwanTraceWrite> {
    event(
        cycle,
        WonderSwanTraceWrite::WaveRam {
            address,
            value,
            origin,
        },
    )
}

fn trace(color: bool) -> WonderSwanAudioTrace {
    let mut events = (0..32)
        .map(|index| {
            wave_ram(
                0,
                index,
                ((index * 11) as u8 & 15) << 4 | (index as u8 & 15),
                WonderSwanTraceOrigin::Cpu,
            )
        })
        .collect::<Vec<_>>();
    events.extend([
        register(0, 0x80, 0x80, WonderSwanTraceOrigin::Cpu),
        register(0, 0x81, 2, WonderSwanTraceOrigin::Cpu),
        register(0, 0x88, 0xff, WonderSwanTraceOrigin::Cpu),
        register(0, 0x91, 0x0f, WonderSwanTraceOrigin::Cpu),
        register(0, 0x90, 1, WonderSwanTraceOrigin::Cpu),
        wave_ram(
            91_001,
            4,
            0xf0,
            if color {
                WonderSwanTraceOrigin::GeneralDma
            } else {
                WonderSwanTraceOrigin::Cpu
            },
        ),
        register(133_333, 0x88, 0x9f, WonderSwanTraceOrigin::CpuInterrupt),
    ]);
    if color {
        events.push(register(
            180_000,
            0x89,
            0x8f,
            WonderSwanTraceOrigin::SoundDma,
        ));
    }
    events.push(register(220_000, 0x90, 0, WonderSwanTraceOrigin::Cpu));
    WonderSwanAudioTrace {
        generation: 1,
        cycle_hz: MASTER_CLOCK_HZ,
        cycle_hz_denominator: 1,
        chip: WonderSwanTraceChip {
            clock_hz: MASTER_CLOCK_HZ,
            color,
            reset: WonderSwanResetState::default(),
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 260_000,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn source_pcm(trace: &WonderSwanAudioTrace, rate: u32, drain_cycles: u64) -> Vec<f32> {
    let mut apu = Apu::new(rate);
    let mut ram = trace.chip.reset.wave_ram.clone();
    let mut output = Vec::new();
    let mut scratch = Vec::new();
    let mut cycle = 0;
    let mut next_event = 0;
    let mut next_drain = drain_cycles;
    while cycle < trace.end_cycle {
        while let Some(event) = trace.events.get(next_event)
            && event.cycle == cycle
        {
            match event.write {
                WonderSwanTraceWrite::Register { port, value, .. } => apu.write8(port, value),
                WonderSwanTraceWrite::WaveRam { address, value, .. } => {
                    ram[usize::from(address)] = value
                }
            }
            next_event += 1;
        }
        let event_cycle = trace
            .events
            .get(next_event)
            .map_or(trace.end_cycle, |event| event.cycle);
        let target = event_cycle.min(next_drain).min(trace.end_cycle);
        apu.step_cycles((target - cycle) as u32, &ram);
        cycle = target;
        if cycle == next_drain {
            scratch.clear();
            apu.drain_audio_samples_into(&mut scratch);
            output.extend_from_slice(&scratch);
            next_drain += drain_cycles;
        }
    }
    scratch.clear();
    apu.drain_audio_samples_into(&mut scratch);
    output.extend_from_slice(&scratch);
    output
}

fn read_all(session: &mut WonderSwanTraceSession, size: usize) -> (Vec<i16>, Vec<u32>) {
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
fn native_apu_pcm_matches_models_rates_chunks_and_reset() -> Result<()> {
    for color in [false, true] {
        for sample_rate in [44_100, 48_000, 63_072, 96_000] {
            let trace = trace(color);
            let source = source_pcm(&trace, sample_rate, 7_777);
            assert_eq!(source, source_pcm(&trace, sample_rate, 101));
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
                WonderSwanTraceSession::new(trace.clone(), options, &AtomicBool::new(false))?;
            assert_eq!(session.duration_frames() * 2, expected.len());
            for chunk in [2, 74, 2048, 8192] {
                session.reset()?;
                let (actual, actual_bits) = read_all(&mut session, chunk);
                assert_eq!(actual, expected, "{color} {sample_rate} {chunk}");
                assert_eq!(actual_bits, bits, "f32 {color} {sample_rate} {chunk}");
            }
            let mut fresh = WonderSwanTraceSession::new(trace, options, &AtomicBool::new(false))?;
            assert_eq!(read_all(&mut fresh, 190).0, expected);
        }
    }
    Ok(())
}

#[test]
fn native_contract_rejects_reset_clock_and_unsupported_writes() {
    let reject = |trace| {
        assert!(
            WonderSwanTraceSession::new(trace, RenderOptions::default(), &AtomicBool::new(false))
                .is_err()
        );
    };
    macro_rules! reject_change {
        ($field:ident $(.$nested:ident)*, $value:expr) => {{
            let mut changed = trace(true);
            changed.$field $(.$nested)* = $value;
            reject(changed);
        }};
    }
    reject_change!(cycle_hz, 1);
    reject_change!(cycle_hz_denominator, 2);
    reject_change!(chip.clock_hz, 1);
    reject_change!(chip.reset.period_counters, [2; 4]);
    reject_change!(chip.reset.wave_ram, vec![0; 1]);
    reject_change!(timing, AudioTraceTiming::InstructionBoundary);
    reject_change!(dropped_events, 1);
    reject_change!(invalidated, Some(AudioTraceInvalidation::Reset));
    let mut changed = trace(true);
    changed.events[0].cycle = changed.end_cycle + 1;
    reject(changed);
    let mut changed = trace(true);
    changed.events[0].pc = 0x10_0000;
    reject(changed);
    for origin in [
        WonderSwanTraceOrigin::CpuInterrupt,
        WonderSwanTraceOrigin::SoundDma,
    ] {
        let mut changed = trace(true);
        let event = changed
            .events
            .iter_mut()
            .find(|event| match event.write {
                WonderSwanTraceWrite::Register {
                    origin: event_origin,
                    ..
                }
                | WonderSwanTraceWrite::WaveRam {
                    origin: event_origin,
                    ..
                } => event_origin == origin,
            })
            .unwrap();
        event.pc = 1;
        reject(changed);
        let mut changed = trace(true);
        let event = changed
            .events
            .iter_mut()
            .find(|event| match event.write {
                WonderSwanTraceWrite::Register {
                    origin: event_origin,
                    ..
                }
                | WonderSwanTraceWrite::WaveRam {
                    origin: event_origin,
                    ..
                } => event_origin == origin,
            })
            .unwrap();
        event.instruction_source = AudioTraceSource::Unmapped;
        reject(changed);
    }
    for write in [
        WonderSwanTraceWrite::Register {
            port: 0x64,
            value: 0,
            origin: WonderSwanTraceOrigin::Cpu,
        },
        WonderSwanTraceWrite::Register {
            port: 0x69,
            value: 0,
            origin: WonderSwanTraceOrigin::SoundDma,
        },
        WonderSwanTraceWrite::Register {
            port: 0x96,
            value: 0,
            origin: WonderSwanTraceOrigin::Cpu,
        },
        WonderSwanTraceWrite::Register {
            port: 0x89,
            value: 0,
            origin: WonderSwanTraceOrigin::GeneralDma,
        },
        WonderSwanTraceWrite::Register {
            port: 0x95,
            value: 2,
            origin: WonderSwanTraceOrigin::Cpu,
        },
        WonderSwanTraceWrite::WaveRam {
            address: 0,
            value: 0,
            origin: WonderSwanTraceOrigin::SoundDma,
        },
        WonderSwanTraceWrite::WaveRam {
            address: 0x4000,
            value: 0,
            origin: WonderSwanTraceOrigin::Cpu,
        },
    ] {
        let mut changed = trace(true);
        changed.events.push(event(changed.end_cycle, write));
        reject(changed);
    }
    let mut monochrome = trace(false);
    monochrome.events.push(register(
        monochrome.end_cycle,
        0x89,
        0,
        WonderSwanTraceOrigin::SoundDma,
    ));
    reject(monochrome);
    let mut monochrome_dma = trace(false);
    monochrome_dma.events.push(wave_ram(
        monochrome_dma.end_cycle,
        0,
        0,
        WonderSwanTraceOrigin::GeneralDma,
    ));
    reject(monochrome_dma);
}

#[test]
fn mute_cancel_and_bounded_long_traces_preserve_the_native_clock() -> Result<()> {
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    let mut long = trace(true);
    long.end_cycle = u64::MAX;
    let mut session = WonderSwanTraceSession::new(long.clone(), options, &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 48_000);
    assert!(!session.has_source_duration_limit());
    assert!(WonderSwanTraceSession::new(long, options, &AtomicBool::new(true)).is_err());
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
