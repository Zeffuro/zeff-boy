use super::*;
use crate::emulator::Emulator;
use crate::hardware::types::hardware_mode::HardwareModePreference;
use zeff_emu_common::audio_trace::{AudioTraceSource, GameBoyTraceWrite as Write};

fn store(code: &mut Vec<u8>, address: u16, value: u8) {
    code.extend([0x3e, value, 0xea, address as u8, (address >> 8) as u8]);
}

fn fixture(mode: usize, rate: u32, firmware: bool) -> Emulator {
    let mut rom = vec![0; 0x8000];
    rom[4..7].copy_from_slice(&[0xc3, 0x50, 0x01]);
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 0x01]);
    rom[0x143] = if mode == 1 { 0x80 } else { 0 };
    let mut code = Vec::new();
    if mode == 1 {
        store(&mut code, 0xff4d, 1);
        code.extend([0x10, 0]);
    }
    let restart = 0x150 + code.len() as u16;
    for (address, value) in [
        (0xff26, 0),
        (0xff26, 0x80),
        (0xff24, 0x77),
        (0xff25, 0xff),
        (0xff10, 0x11),
        (0xff11, 0x80),
        (0xff12, 0xf3),
        (0xff13, 0x40),
        (0xff14, 0x87),
        (0xff16, 0x40),
        (0xff17, 0xa2),
        (0xff18, 0x80),
        (0xff19, 0x83),
        (0xff1a, 0x80),
        (0xff1c, 0x20),
        (0xff1d, 0xf0),
        (0xff1e, 0x87),
        (0xff21, 0x93),
        (0xff22, 0x35),
        (0xff23, 0x80),
        (0xff30, 0xa5),
        (0xff3f, 0x3c),
        (0xff04, 0),
    ] {
        store(&mut code, address, value);
    }
    code.extend([0x01, 0, 4, 0x0b, 0x78, 0xb1, 0x20, 0xfb]);
    code.extend([0xc3, restart as u8, (restart >> 8) as u8]);
    rom[0x150..0x150 + code.len()].copy_from_slice(&code);
    let preference = if mode == 2 {
        HardwareModePreference::ForceCgb
    } else {
        HardwareModePreference::Auto
    };
    let mut emu = if firmware {
        let mut boot = vec![0; if mode == 0 { 0x100 } else { 0x900 }];
        boot[..4].copy_from_slice(&[0x3e, 1, 0xe0, 0x50]);
        Emulator::from_rom_data_with_boot_rom(&rom, preference, &boot).unwrap()
    } else {
        Emulator::from_rom_data(&rom, preference).unwrap()
    };
    emu.set_sample_rate(rate);
    emu
}

fn state(apu: &Apu) -> Vec<u8> {
    let mut writer = crate::save_state::StateWriter::new();
    apu.write_state(&mut writer);
    writer.into_bytes()
}

fn source(
    mode: usize,
    rate: u32,
    path: usize,
    firmware: bool,
) -> (GameBoyAudioTrace, Vec<Vec<f32>>, Vec<u8>) {
    let mut plain = fixture(mode, rate, firmware);
    let mut captured = fixture(mode, rate, firmware);
    captured.reset_and_begin_native_audio_trace(20_000).unwrap();
    plain.set_instruction_trace_enabled(path == 1);
    captured.set_instruction_trace_enabled(path == 1);
    let mut pcm = Vec::new();
    for _ in 0..6 {
        for emu in [&mut plain, &mut captured] {
            if path < 2 {
                emu.step_frame();
            } else {
                let end = emu.cycle_count + 22_000;
                while emu.cycle_count < end {
                    emu.step_instruction();
                }
            }
        }
        assert_eq!(
            captured.encode_state_bytes().unwrap(),
            plain.encode_state_bytes().unwrap()
        );
        let expected = plain.drain_audio_samples();
        let actual = captured.drain_audio_samples();
        assert_eq!(
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        pcm.push(expected);
    }
    let trace = captured.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    (trace, pcm, plain.bus.native_audio_state())
}

#[test]
fn native_replay_matches_source_pcm_state_and_drains_for_every_model_rate_seed_and_path()
-> Result<()> {
    let cancel = AtomicBool::new(false);
    for mode in 0..3 {
        for rate in [44_100, 48_000, 63_072, 96_000] {
            for path in 0..3 {
                for firmware in [false, true] {
                    let (trace, expected, expected_state) = source(mode, rate, path, firmware);
                    let mut replay = GameBoyTraceReplayer::new(trace.clone(), rate)
                        .unwrap_or_else(|error| panic!("mode {mode} rate {rate} path {path} firmware {firmware}: {error}; {:?}", trace.chip));
                    assert_eq!(
                        replay.duration_frames(),
                        expected.iter().map(|v| v.len() as u64 / 2).sum()
                    );
                    for _ in 0..2 {
                        for samples in &expected {
                            let actual = replay.read_next_drain(&cancel)?.unwrap();
                            assert_eq!(
                                actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                samples.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                "mode {mode} rate {rate} path {path} firmware {firmware}"
                            );
                        }
                        assert!(replay.read_next_drain(&cancel)?.is_none());
                        assert_eq!(state(&replay.apu), expected_state);
                        replay.reset()?;
                    }
                    assert!(trace.events.iter().any(|event| matches!(
                        event.write,
                        Write::NativeBatch {
                            repetitions: 10..,
                            ..
                        }
                    )));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn legacy_missing_timing_lost_events_and_unbounded_batches_are_refused() {
    let (trace, _, _) = source(0, 48_000, 2, false);
    let reject = |trace| assert!(GameBoyTraceReplayer::new(trace, 48_000).is_err());
    let mut bad = trace.clone();
    bad.chip.native_replay = None;
    reject(bad);
    let mut bad = trace.clone();
    bad.dropped_events = 1;
    reject(bad);
    let mut bad = trace.clone();
    bad.end_cycle = 4_194_304 * 7200 + 1;
    reject(bad);
    let mut bad = trace.clone();
    bad.events.pop();
    reject(bad);
    let mut bad = trace.clone();
    bad.chip.reset.nr52 = 0;
    reject(bad);
    for write in [
        Write::NativeBatch {
            cycles: 0,
            repetitions: u32::MAX,
        },
        Write::NativeBatch {
            cycles: u32::MAX,
            repetitions: u32::MAX,
        },
        Write::NativeBatch {
            cycles: 4,
            repetitions: u32::MAX,
        },
        Write::NativeBatch {
            cycles: 0,
            repetitions: 0,
        },
        Write::PcmDrain { frames: u32::MAX },
    ] {
        let mut bad = trace.clone();
        bad.events[0].write = write;
        reject(bad);
    }
    let mut bad = trace.clone();
    bad.events
        .retain(|event| !matches!(event.write, Write::NativeDividerPhase { .. }));
    reject(bad);
    let mut bad = trace.clone();
    bad.events[0].instruction_source = AudioTraceSource::Unmapped;
    reject(bad);
}

#[test]
fn output_changes_remain_recorded_but_do_not_claim_exact_replay() {
    for change in [
        |emu: &mut Emulator| emu.set_sample_rate(96_000),
        |emu: &mut Emulator| emu.set_apu_sample_generation_enabled(false),
        |emu: &mut Emulator| emu.set_apu_channel_mutes([true; 4]),
    ] {
        let mut emu = fixture(0, 48_000, false);
        emu.reset_and_begin_native_audio_trace(8192).unwrap();
        emu.step_frame();
        change(&mut emu);
        emu.drain_audio_samples();
        let trace = emu.finish_audio_trace().unwrap();
        trace.validate_complete().unwrap();
        assert!(GameBoyTraceReplayer::new(trace, 48_000).is_err());
    }
}

#[test]
fn requested_rates_match_independent_native_sources_with_the_same_guest_timeline() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for mode in 0..3 {
        let (trace, _, _) = source(mode, 48_000, 2, false);
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let (_, expected, expected_state) = source(mode, rate, 2, false);
            let mut replay = GameBoyTraceReplayer::new(trace.clone(), rate)?;
            for samples in expected {
                let actual = replay.read_next_drain(&cancel)?.unwrap();
                assert_eq!(
                    actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    samples.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
            }
            assert!(replay.read_next_drain(&cancel)?.is_none());
            assert_eq!(state(&replay.apu), expected_state);
        }
    }
    Ok(())
}

#[test]
fn stop_wake_preserves_dmg_frozen_output_and_cgb_running_output() -> Result<()> {
    use crate::hardware::types::CpuState;

    let cancel = AtomicBool::new(false);
    for mode in 0..3 {
        let mut rom = vec![0; 0x8000];
        rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 0x01]);
        rom[0x143] = if mode == 1 { 0x80 } else { 0 };
        let mut code = Vec::new();
        for (address, value) in [
            (0xff26, 0x80),
            (0xff24, 0x77),
            (0xff25, 0xff),
            (0xff12, 0xf0),
            (0xff14, 0x80),
            (0xff00, 0x10),
        ] {
            store(&mut code, address, value);
        }
        code.extend([0x10, 0, 0x18, 0xfe]);
        rom[0x150..0x150 + code.len()].copy_from_slice(&code);
        let preference = if mode == 2 {
            HardwareModePreference::ForceCgb
        } else {
            HardwareModePreference::Auto
        };
        let mut plain = Emulator::from_rom_data(&rom, preference)?;
        let mut captured = Emulator::from_rom_data(&rom, preference)?;
        plain.set_sample_rate(48_000);
        captured.set_sample_rate(48_000);
        captured.reset_and_begin_native_audio_trace(8192)?;
        let mut expected = Vec::new();
        for phase in 0..3 {
            for emu in [&mut plain, &mut captured] {
                if phase == 0 {
                    while emu.cpu.running != CpuState::Stopped {
                        emu.step_instruction();
                    }
                } else {
                    if phase == 2 {
                        emu.set_input(1, 0);
                    }
                    for _ in 0..1024 {
                        emu.step_instruction();
                    }
                }
            }
            let samples = plain.drain_audio_samples();
            assert_eq!(captured.drain_audio_samples(), samples);
            if phase == 1 {
                assert_eq!(samples.is_empty(), mode == 0);
            }
            expected.push(samples);
            assert_eq!(captured.encode_state_bytes()?, plain.encode_state_bytes()?);
        }
        let trace = captured.finish_audio_trace().unwrap();
        let mut replay = GameBoyTraceReplayer::new(trace, 48_000)?;
        for samples in expected {
            assert_eq!(replay.read_next_drain(&cancel)?.unwrap(), samples);
        }
        assert!(replay.read_next_drain(&cancel)?.is_none());
        assert_eq!(state(&replay.apu), plain.bus.native_audio_state());
    }
    Ok(())
}

#[test]
fn native_output_frame_clock_matches_the_existing_resampler() {
    for rate in [44_100, 48_000, 63_072, 96_000] {
        let mut apu = Apu::new();
        apu.set_sample_rate(rate);
        apu.write(0xff26, 0x80);
        let mut cycles = 0;
        let mut frames = 0;
        for advance in [0, 1, 4, 8192, 65_544, 100_003] {
            apu.step(advance);
            cycles += advance;
            frames += apu.drain_samples().len() / 2;
            let expected = ((cycles * 2 + 1) * u64::from(rate)) / (2 * 4_194_304);
            assert_eq!(frames as u64, expected);
        }
    }
}

#[test]
fn both_cgb_speed_switch_directions_replay_the_native_batches_and_output() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 0x01]);
    rom[0x143] = 0x80;
    let mut code = Vec::new();
    for (address, value) in [
        (0xff26, 0x80),
        (0xff24, 0x77),
        (0xff25, 0x11),
        (0xff12, 0xf0),
        (0xff14, 0x83),
    ] {
        store(&mut code, address, value);
    }
    let repeat = 0x150 + code.len() as u16;
    store(&mut code, 0xff4d, 1);
    code.extend([0x10, 0, 0x01, 0, 4, 0x0b, 0x78, 0xb1, 0x20, 0xfb]);
    code.extend([0xc3, repeat as u8, (repeat >> 8) as u8]);
    rom[0x150..0x150 + code.len()].copy_from_slice(&code);
    for rate in [44_100, 48_000, 63_072, 96_000] {
        let mut plain = Emulator::from_rom_data(&rom, HardwareModePreference::Auto)?;
        let mut captured = Emulator::from_rom_data(&rom, HardwareModePreference::Auto)?;
        plain.set_sample_rate(rate);
        captured.set_sample_rate(rate);
        captured.reset_and_begin_native_audio_trace(8192)?;
        let mut expected = Vec::new();
        for _ in 0..6 {
            plain.step_frame();
            captured.step_frame();
            let samples = plain.drain_audio_samples();
            assert_eq!(captured.drain_audio_samples(), samples);
            assert_eq!(captured.encode_state_bytes()?, plain.encode_state_bytes()?);
            expected.push(samples);
        }
        let trace = captured.finish_audio_trace().unwrap();
        for double_speed in [false, true] {
            assert!(
                trace
                    .events
                    .iter()
                    .any(|event| { event.write == Write::SpeedSwitch { double_speed } })
            );
        }
        let mut replay = GameBoyTraceReplayer::new(trace, rate)?;
        for samples in expected {
            assert_eq!(replay.read_next_drain(&cancel)?.unwrap(), samples);
        }
        assert!(replay.read_next_drain(&cancel)?.is_none());
        assert_eq!(state(&replay.apu), plain.bus.native_audio_state());
    }
    Ok(())
}

#[test]
fn runtime_wave_access_mismatch_requires_reset_before_retry() -> Result<()> {
    let (mut trace, _, _) = source(0, 48_000, 2, false);
    let write = trace
        .events
        .iter_mut()
        .find_map(|event| match &mut event.write {
            Write::WaveRam { applied_index, .. } => Some(applied_index),
            _ => None,
        })
        .unwrap();
    *write = Some((write.unwrap_or(0) + 1) % 16);
    let mut replay = GameBoyTraceReplayer::new(trace, 48_000)?;
    let cancel = AtomicBool::new(false);
    for _ in 0..2 {
        let error = replay.read_next_drain(&cancel).unwrap_err().to_string();
        assert!(error.contains("wave-RAM access differs"), "{error}");
        let error = replay.read_next_drain(&cancel).unwrap_err().to_string();
        assert!(error.contains("reset Game Boy replay"), "{error}");
        replay.reset()?;
    }
    Ok(())
}
