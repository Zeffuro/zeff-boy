use super::*;
use zeff_nes_core::emulator::Emulator;

fn capture(region: u8, rate: u32) -> (NesAudioTrace, Vec<i16>, Vec<u32>) {
    let mut rom = vec![0; 16 + 0x8000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 2;
    rom[5] = 1;
    rom[7] = 8;
    rom[12] = region;
    let mut program = vec![0x78];
    for (address, value) in [
        (0x4015u16, 1),
        (0x4000, 0xbf),
        (0x4002, 0x40),
        (0x4003, 8),
        (0x4017, 0x80),
        (0x4010, 0x4f),
        (0x4011, 0x40),
        (0x4012, 0),
        (0x4013, 1),
        (0x4015, 0x11),
    ] {
        program.extend([0xa9, value, 0x8d, address as u8, (address >> 8) as u8]);
    }
    let loop_address = 0x8000 + program.len() as u16;
    program.extend([
        0xad,
        0x15,
        0x40,
        0x4c,
        loop_address as u8,
        (loop_address >> 8) as u8,
    ]);
    rom[16..16 + program.len()].copy_from_slice(&program);
    for (index, value) in rom[16 + 0x4000..16 + 0x7ffa].iter_mut().enumerate() {
        *value = (index as u8).wrapping_mul(73) ^ 0xa5;
    }
    rom[16 + 0x7ffa..16 + 0x8000].copy_from_slice(&[0, 0x80, 0, 0x80, 0, 0x80]);
    let mut emu = Emulator::new_with_audio_trace(&rom, f64::from(rate), 16_384).unwrap();
    while emu.cpu_cycles() < 40_007 {
        emu.step_instruction();
    }
    let source = emu.drain_audio_samples();
    let expected = source
        .iter()
        .flat_map(|value| [(value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16; 2])
        .collect();
    let bits = source
        .iter()
        .flat_map(|value| [value.to_bits(); 2])
        .collect();
    (emu.finish_audio_trace().unwrap(), expected, bits)
}

fn read_all(session: &mut NesTraceSession, size: usize) -> (Vec<i16>, Vec<u32>) {
    let mut output = Vec::new();
    let mut floats = Vec::new();
    let mut buffer = vec![0; size];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
        floats.extend(
            session
                .apu
                .sample_buffer
                .iter()
                .flat_map(|value| [value.to_bits(); 2]),
        );
    }
    (output, floats)
}

#[test]
fn native_nes_pcm_matches_all_regions_rates_chunks_resets_and_fresh_sessions() -> Result<()> {
    for region in [0, 1, 3] {
        for sample_rate in [44_100, 48_000, 63_072, 96_000] {
            let (trace, expected, bits) = capture(region, sample_rate);
            let options = RenderOptions {
                sample_rate,
                max_seconds: 1,
                ..RenderOptions::default()
            };
            let mut session =
                NesTraceSession::new(trace.clone(), options, &AtomicBool::new(false))?;
            assert_eq!(session.duration_frames() * 2, expected.len());
            assert!(expected.iter().any(|sample| *sample != 0));
            for chunk in [2, 74, 2048, 8192] {
                session.reset()?;
                let (actual, float_bits) = read_all(&mut session, chunk);
                assert_eq!(
                    actual, expected,
                    "region {region} rate {sample_rate} chunk {chunk}"
                );
                assert_eq!(float_bits, bits);
                assert_eq!(session.cycle, trace.end_cycle);
                assert_eq!(session.next_event, trace.events.len());
            }
            let mut fresh = NesTraceSession::new(trace, options, &AtomicBool::new(false))?;
            assert_eq!(read_all(&mut fresh, 190).0, expected);
        }
    }
    Ok(())
}

#[test]
fn native_sampler_boundary_and_maximum_source_cycles_have_exact_durations() -> Result<()> {
    let (mut trace, _, _) = capture(0, 48_000);
    trace.events.clear();
    trace.end_cycle = 13_125;
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    let mut session = NesTraceSession::new(trace.clone(), options, &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 351);
    assert_eq!(read_all(&mut session, 74).0.len(), 702);
    assert_eq!(session.cycle, 13_125);
    trace.end_cycle = u64::MAX;
    let mut session = NesTraceSession::new(trace, options, &AtomicBool::new(false))?;
    assert_eq!(session.duration_frames(), 48_000);
    assert_eq!(read_all(&mut session, 2048).0.len(), 96_000);
    Ok(())
}

#[test]
fn source_contracts_and_invalid_event_forms_are_rejected() {
    let (trace, _, _) = capture(0, 48_000);
    let reject = |trace| {
        assert!(
            NesTraceSession::new(trace, RenderOptions::default(), &AtomicBool::new(false)).is_err()
        )
    };
    macro_rules! reject_change {
        ($field:ident $(.$nested:ident)*,$value:expr) => {{
            let mut changed = trace.clone();
            changed.$field $(.$nested)* = $value;
            reject(changed);
        }};
    }
    reject_change!(cycle_hz, 1);
    reject_change!(cycle_hz_denominator, 0);
    reject_change!(timing, AudioTraceTiming::InstructionBoundary);
    reject_change!(dropped_events, 1);
    reject_change!(
        invalidated,
        Some(zeff_emu_common::audio_trace::AudioTraceInvalidation::Reset)
    );
    reject_change!(chip.initial_cpu_cycle, 0);
    let mut changed = trace.clone();
    changed.events[0].cycle = changed.end_cycle;
    reject(changed);
    let mut changed = trace.clone();
    changed.events[0].write = NesTraceWrite::Register {
        address: 0x4014,
        value: 0,
        odd_cycle: false,
    };
    reject(changed);
    let mut changed = trace.clone();
    let first = &mut changed.events[0];
    if let NesTraceWrite::Register { odd_cycle, .. } = &mut first.write {
        *odd_cycle = !*odd_cycle;
    }
    reject(changed);
    let mut changed = trace.clone();
    changed.events[0].write = NesTraceWrite::StatusRead {
        value: 0x20,
        origin: NesTraceOrigin::Cpu,
    };
    reject(changed);
    let mut changed = trace.clone();
    changed.events[0].write = NesTraceWrite::StatusRead {
        value: 0,
        origin: NesTraceOrigin::Dma,
    };
    reject(changed);
    let mut changed = trace.clone();
    let fetch = changed
        .events
        .iter_mut()
        .find(|event| matches!(event.write, NesTraceWrite::DmcFetch { .. }))
        .unwrap();
    fetch.write = NesTraceWrite::DmcFetch {
        address: 0x4000,
        value: 0,
        source: AudioTraceSource::Unknown,
    };
    reject(changed);
}

#[test]
fn dmc_payload_sequence_and_native_status_are_checked_during_replay() -> Result<()> {
    let (trace, _, _) = capture(0, 48_000);
    for fetch in [false, true] {
        let mut changed = trace.clone();
        for event in &mut changed.events {
            match &mut event.write {
                NesTraceWrite::DmcFetch { address, .. } if fetch => {
                    *address += 1;
                    break;
                }
                NesTraceWrite::StatusRead { value, .. } if !fetch => {
                    *value ^= 1;
                    break;
                }
                _ => {}
            }
        }
        let mut session =
            NesTraceSession::new(changed, RenderOptions::default(), &AtomicBool::new(false))?;
        assert!(
            session
                .read(&mut [0; 2048], &AtomicBool::new(false))
                .is_err()
        );
        assert_eq!(session.position_frames(), 0);
        assert!(session.read(&mut [0; 2], &AtomicBool::new(false)).is_err());
    }
    Ok(())
}

#[test]
fn interval_fade_mute_and_cancel_controls_preserve_the_replay_clock() -> Result<()> {
    let (trace, expected, _) = capture(0, 48_000);
    let cancel = AtomicBool::new(false);
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    assert!(NesTraceSession::new(trace.clone(), options, &AtomicBool::new(true)).is_err());
    let mut session = NesTraceSession::new(trace.clone(), options, &cancel)?;
    assert!(session.has_source_duration_limit());
    assert!(session.read(&mut [0; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.read(&mut [0; 3], &cancel).is_err());
    assert!(session.set_track_mask(2).is_err());
    session.set_track_mask(0)?;
    let mut muted = [1; 74];
    assert_eq!(session.read(&mut muted, &cancel)?, 74);
    assert_eq!(muted, [0; 74]);
    session.set_track_mask(1)?;
    assert_eq!(read_all(&mut session, 2048).0, expected[74..]);
    session.reset()?;
    assert_eq!(read_all(&mut session, 2048).0, expected);
    let mut session = NesTraceSession::new(
        trace,
        RenderOptions {
            fade_seconds: 1,
            ..options
        },
        &cancel,
    )?;
    let faded = read_all(&mut session, 2048).0;
    assert_ne!(faded, expected);
    assert_eq!(faded[faded.len() - 2..], [0, 0]);
    Ok(())
}
