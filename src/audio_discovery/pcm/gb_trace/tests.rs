use super::*;
use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn capture(mode: usize, rate: u32) -> (GameBoyAudioTrace, Vec<f32>) {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    rom[0x143] = if mode == 1 { 0x80 } else { 0 };
    let mut code = Vec::new();
    if mode == 1 {
        code.extend([0x3e, 1, 0xe0, 0x4d, 0x10, 0]);
    }
    let repeat = 0x150 + code.len() as u16;
    for (address, value) in [
        (0x26, 0),
        (0x26, 0x80),
        (0x24, 0x77),
        (0x25, 0x11),
        (0x11, 0x80),
        (0x12, 0xf0),
        (0x13, 0x40),
        (0x14, 0x83),
        (0x04, 0),
    ] {
        code.extend([0x3e, value, 0xe0, address]);
    }
    code.extend([0x01, 0, 4, 0x0b, 0x78, 0xb1, 0x20, 0xfb]);
    code.extend([0xc3, repeat as u8, (repeat >> 8) as u8]);
    rom[0x150..0x150 + code.len()].copy_from_slice(&code);
    let preference = if mode == 2 {
        HardwareModePreference::ForceCgb
    } else {
        HardwareModePreference::Auto
    };
    let mut emu = Emulator::from_rom_data(&rom, preference).unwrap();
    emu.set_sample_rate(rate);
    emu.reset_and_begin_native_audio_trace(8192).unwrap();
    let mut pcm = Vec::new();
    for _ in 0..5 {
        emu.step_frame();
        pcm.extend(emu.drain_audio_samples());
    }
    (emu.finish_audio_trace().unwrap(), pcm)
}

fn read_all(session: &mut GbTraceSession, chunk: usize) -> (Vec<i16>, Vec<u32>) {
    let mut pcm = Vec::new();
    let mut bits = Vec::new();
    let mut buffer = vec![0; chunk];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            break;
        }
        pcm.extend_from_slice(&buffer[..count]);
        bits.extend(session.floats.iter().map(|sample| sample.to_bits()));
    }
    (pcm, bits)
}

#[test]
fn native_gb_pcm_matches_models_rates_chunks_resets_and_fresh_sessions() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for mode in 0..3 {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let (trace, samples) = capture(mode, rate);
            let expected: Vec<_> = samples
                .iter()
                .map(|v| (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                .collect();
            let bits: Vec<_> = samples.iter().map(|v| v.to_bits()).collect();
            let options = RenderOptions {
                sample_rate: rate,
                max_seconds: 1,
                ..Default::default()
            };
            let mut session = GbTraceSession::new(trace.clone(), options, &cancel)?;
            assert_eq!(session.duration_frames() * 2, samples.len());
            assert!(expected.iter().any(|sample| *sample != 0));
            for chunk in [2, 74, 2048, 8192] {
                session.reset()?;
                assert_eq!(
                    read_all(&mut session, chunk),
                    (expected.clone(), bits.clone())
                );
            }
            let mut fresh = GbTraceSession::new(trace, options, &cancel)?;
            assert_eq!(read_all(&mut fresh, 190).0, expected);
        }
    }
    Ok(())
}

#[test]
fn caller_controls_preserve_native_timing_and_reset() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let (trace, _) = capture(0, 48_000);
    let options = RenderOptions {
        max_seconds: 1,
        ..Default::default()
    };
    let mut session = GbTraceSession::new(trace.clone(), options, &cancel)?;
    let expected = read_all(&mut session, 2048).0;
    session.reset()?;
    assert!(session.read(&mut [0; 3], &cancel).is_err());
    assert!(session.read(&mut [0; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.set_track_mask(2).is_err());
    session.set_track_mask(0)?;
    let mut muted = [1; 74];
    assert_eq!(session.read(&mut muted, &cancel)?, 74);
    assert_eq!(muted, [0; 74]);
    session.set_track_mask(1)?;
    assert_eq!(read_all(&mut session, 2048).0, expected[74..]);
    session.reset()?;
    assert_eq!(read_all(&mut session, 2048).0, expected);
    let mut faded = GbTraceSession::new(
        trace,
        RenderOptions {
            fade_seconds: 1,
            ..options
        },
        &cancel,
    )?;
    let pcm = read_all(&mut faded, 2048).0;
    assert_ne!(pcm, expected);
    assert_eq!(pcm[pcm.len() - 2..], [0, 0]);
    Ok(())
}

#[test]
fn legacy_contract_and_prescheduled_cancellation_are_refused() {
    let (mut trace, _) = capture(0, 48_000);
    assert!(
        GbTraceSession::new(
            trace.clone(),
            RenderOptions::default(),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    trace.chip.native_replay = None;
    assert!(GbTraceSession::new(trace, RenderOptions::default(), &AtomicBool::new(false)).is_err());
}
