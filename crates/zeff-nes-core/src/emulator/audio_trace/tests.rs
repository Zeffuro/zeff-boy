use super::*;
use crate::hardware::apu::Apu;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, MAX_AUDIO_TRACE_EVENTS, NesTraceOrigin, NesTraceWrite,
};

fn rom(mapper: u8, region: u8, program: &[u8], trainer: bool) -> Vec<u8> {
    let bias = 16 + if trainer { 512 } else { 0 };
    let mut rom = vec![0; bias + 0x8000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 2;
    rom[5] = 1;
    rom[6] = (mapper << 4) | if trainer { 4 } else { 0 };
    rom[7] = (mapper & 0xf0) | 8;
    rom[10] = 7;
    rom[12] = region;
    rom[bias..bias + program.len()].copy_from_slice(program);
    for (index, value) in rom[bias + 0x4000..bias + 0x7ffa].iter_mut().enumerate() {
        *value = (index as u8).wrapping_mul(17) ^ 0xa5;
    }
    rom[bias + 0x7ffa..bias + 0x8000].copy_from_slice(&[0, 0x80, 0, 0x80, 0, 0x80]);
    rom
}

fn program() -> Vec<u8> {
    let mut bytes = vec![0x78];
    for (address, value) in [
        (0x4015u16, 0x0f),
        (0x4000, 0xbf),
        (0x4002, 0x40),
        (0x4003, 8),
        (0x4004, 0x7a),
        (0x4006, 0x80),
        (0x4007, 8),
        (0x4008, 0xff),
        (0x400a, 0x20),
        (0x400b, 8),
        (0x400c, 0x37),
        (0x400e, 4),
        (0x400f, 8),
        (0x4017, 0x80),
        (0x4010, 0x4f),
        (0x4011, 0x40),
        (0x4012, 0),
        (0x4013, 1),
        (0x4015, 0x1f),
        (0x4014, 0x40),
    ] {
        bytes.extend([0xa9, value, 0x8d, address as u8, (address >> 8) as u8]);
    }
    let loop_address = 0x8000u16 + bytes.len() as u16;
    bytes.extend([0xa2, 64, 0xca, 0xd0, 0xfd, 0xad, 0x15, 0x40]);
    bytes.extend([0xa9, 0, 0x8d, 0x17, 0x40, 0x24, 0, 0x8d, 0x17, 0x40]);
    bytes.extend([0x4c, loop_address as u8, (loop_address >> 8) as u8]);
    bytes
}

fn run(emu: &mut Emulator, cycles: u64) -> Vec<f32> {
    let target = emu.cpu_cycles() + cycles;
    while emu.cpu_cycles() < target {
        emu.step_instruction();
    }
    emu.drain_audio_samples()
}

fn apu_state(apu: &Apu) -> Vec<u8> {
    let mut writer = crate::save_state::StateWriter::new();
    apu.write_state(&mut writer);
    apu.write_frame_counter_runtime_state(&mut writer);
    writer.into_bytes()
}

fn replay(trace: &NesAudioTrace, rate: f64) -> (Vec<f32>, Vec<u8>) {
    let mut apu = Apu::new_for_audio_trace(&trace.chip, rate).unwrap();
    let mut events = trace.events.iter().peekable();
    for cycle in 0..trace.end_cycle {
        while events.peek().is_some_and(|event| event.cycle == cycle) {
            match events.next().unwrap().write {
                NesTraceWrite::Register {
                    address,
                    value,
                    odd_cycle,
                } => {
                    assert_eq!(odd_cycle, (cycle + 7) & 1 != 0);
                    apu.write_register(address, value, odd_cycle);
                }
                NesTraceWrite::StatusRead { value, .. } => assert_eq!(apu.read_status(), value),
                NesTraceWrite::DmcFetch { address, value, .. } => {
                    assert!(apu.dmc.needs_dma());
                    assert_eq!(apu.dmc.dma_address(), address);
                    apu.dmc.fill_sample_buffer(value);
                }
            }
        }
        apu.tick();
    }
    assert!(events.next().is_none());
    (apu.drain_samples(), apu_state(&apu))
}

#[test]
fn native_capture_preserves_state_pcm_and_replays_every_region_rate_and_admitted_board() {
    for mapper in [0, 1, 2, 3, 4, 7] {
        for region in [0, 1, 3] {
            for rate in [44_100.0, 48_000.0, 63_072.0, 96_000.0] {
                let rom = rom(mapper, region, &program(), false);
                let mut baseline = Emulator::new(&rom, rate).unwrap();
                let mut capture = Emulator::new_with_audio_trace(&rom, rate, 8192).unwrap();
                let before = baseline.encode_state().unwrap();
                assert_eq!(capture.encode_state().unwrap(), before);
                let expected = run(&mut baseline, 40_000);
                let actual = run(&mut capture, 40_000);
                assert_eq!(
                    capture.encode_state().unwrap(),
                    baseline.encode_state().unwrap()
                );
                assert_eq!(
                    actual.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
                );
                assert!(actual.iter().any(|sample| sample.abs() > 0.001));
                let trace = capture.finish_audio_trace().unwrap();
                trace.validate_complete().unwrap();
                assert_eq!(trace.end_cycle, capture.cpu_cycles() - 7);
                let (rendered, state) = replay(&trace, rate);
                assert_eq!(state, apu_state(&baseline.bus.apu));
                assert_eq!(
                    rendered.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                    "{mapper} {region} {rate}"
                );
                assert!(trace.events.iter().any(|event| matches!(
                    event.write,
                    NesTraceWrite::StatusRead {
                        origin: NesTraceOrigin::Dma,
                        ..
                    }
                )));
                let parity: Vec<_> = trace
                    .events
                    .iter()
                    .filter_map(|event| match event.write {
                        NesTraceWrite::Register {
                            address: 0x4017,
                            odd_cycle,
                            ..
                        } => Some(odd_cycle),
                        _ => None,
                    })
                    .collect();
                assert!(parity.contains(&true) && parity.contains(&false));
                for event in &trace.events {
                    if let NesTraceWrite::DmcFetch {
                        value,
                        source,
                        address,
                    } = event.write
                    {
                        assert_eq!(event.pc, 0);
                        assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
                        let AudioTraceSource::CartridgeRom {
                            offset,
                            bit_reversed: false,
                        } = source
                        else {
                            panic!("{source:?}")
                        };
                        assert_eq!(rom[offset as usize], value, "{address:x}");
                    }
                }
            }
        }
    }
}

#[test]
fn frame_execution_captures_the_same_native_state_and_pcm_as_the_fast_path() {
    for region in [0, 1, 3] {
        for rate in [44_100.0, 48_000.0, 63_072.0, 96_000.0] {
            let rom = rom(0, region, &program(), false);
            let mut baseline = Emulator::new(&rom, rate).unwrap();
            let mut capture = Emulator::new_with_audio_trace(&rom, rate, 16_384).unwrap();
            for _ in 0..3 {
                baseline.step_frame();
                capture.step_frame();
                assert_eq!(
                    capture.encode_state().unwrap(),
                    baseline.encode_state().unwrap()
                );
            }
            let expected = baseline.drain_audio_samples();
            assert_eq!(capture.drain_audio_samples(), expected);
            let trace = capture.finish_audio_trace().unwrap();
            trace.validate_complete().unwrap();
            for kind in [0, 1, 2] {
                assert!(trace.events.iter().any(|event| match event.write {
                    NesTraceWrite::Register { .. } => kind == 0,
                    NesTraceWrite::StatusRead { .. } => kind == 1,
                    NesTraceWrite::DmcFetch { .. } => kind == 2,
                }));
            }
            let (actual, state) = replay(&trace, rate);
            assert_eq!(
                actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(state, apu_state(&baseline.bus.apu));
        }
    }
}

#[test]
fn capture_checks_board_capacity_and_invalidates_discontinuities() {
    let rom = rom(0, 0, &program(), false);
    let fresh = || Emulator::new_with_audio_trace(&rom, 48_000.0, 8192).unwrap();
    for capacity in [0, MAX_AUDIO_TRACE_EVENTS + 1] {
        assert!(Emulator::new_with_audio_trace(&rom, 48_000.0, capacity).is_err());
    }
    for rate in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e30] {
        assert!(Emulator::new_with_audio_trace(&rom, rate, 32).is_err());
        assert!(
            Apu::new_for_audio_trace(
                &native_trace_chip(crate::hardware::timing::NesTiming::Ntsc),
                rate
            )
            .is_err()
        );
    }
    for mapper in [5, 19, 24, 26, 69, 85] {
        assert!(
            Emulator::new_with_audio_trace(&self::rom(mapper, 0, &program(), false), 48_000.0, 32)
                .is_err()
        );
    }
    for mutation in [
        |emu: &mut Emulator| emu.reset(),
        |emu: &mut Emulator| {
            emu.cpu_write(0x4011, 0x40);
        },
        |emu: &mut Emulator| {
            emu.set_cpu_pc(0x8000);
        },
        |emu: &mut Emulator| {
            emu.bus_mut().apu.write_register(0x4011, 0x40, false);
        },
    ] {
        let mut emu = fresh();
        run(&mut emu, 1000);
        mutation(&mut emu);
        assert!(
            emu.finish_audio_trace()
                .unwrap()
                .validate_complete()
                .is_err()
        );
    }
    let mut emu = fresh();
    let state = emu.encode_state().unwrap();
    run(&mut emu, 1000);
    emu.load_state(&state).unwrap();
    assert_eq!(
        emu.finish_audio_trace().unwrap().invalidated,
        Some(AudioTraceInvalidation::StateRestore)
    );
    let mut emu = fresh();
    assert!(emu.load_state(b"invalid").is_err());
    run(&mut emu, 1000);
    emu.finish_audio_trace()
        .unwrap()
        .validate_complete()
        .unwrap();
    let mut emu = Emulator::new_with_audio_trace(&rom, 48_000.0, 1).unwrap();
    run(&mut emu, 1000);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert!(trace.dropped_events > 0);
    assert!(trace.validate_complete().is_err());
}

#[test]
fn trainer_bias_banked_dmc_wrap_and_ram_writer_provenance_are_retained() {
    let mut rom = rom(2, 0, &[0x4c, 0, 0xc0], true);
    let bias = 16 + 512;
    let mut fixed = Vec::new();
    for (address, value) in [
        (0x4010u16, 15),
        (0x4012, 255),
        (0x4013, 8),
        (0x4015, 16),
        (0x8000, 1),
    ] {
        fixed.extend([0xa9, value, 0x8d, address as u8, (address >> 8) as u8]);
    }
    let loop_address = 0xc000u16 + fixed.len() as u16;
    fixed.extend([0x4c, loop_address as u8, (loop_address >> 8) as u8]);
    rom[bias + 0x4000..bias + 0x4000 + fixed.len()].copy_from_slice(&fixed);
    let mut emu = Emulator::new_with_audio_trace(&rom, 48_000.0, 8192).unwrap();
    let expected = run(&mut emu, 70_000);
    let trace = emu.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    let mut wrapped = false;
    for event in &trace.events {
        match event.write {
            NesTraceWrite::Register { .. } => {
                assert_eq!(
                    event.instruction_source,
                    AudioTraceSource::CartridgeRom {
                        offset: u64::from(event.pc as u16 - 0xc000) + bias as u64 + 0x4000,
                        bit_reversed: false,
                    }
                );
            }
            NesTraceWrite::DmcFetch {
                address,
                value,
                source,
            } => {
                let offset = bias + 0x4000 + usize::from(address & 0x3fff);
                assert_eq!(
                    source,
                    AudioTraceSource::CartridgeRom {
                        offset: offset as u64,
                        bit_reversed: false
                    }
                );
                assert_eq!(value, rom[offset]);
                wrapped |= address == 0x8000;
            }
            _ => {}
        }
    }
    assert!(wrapped);
    let (actual, state) = replay(&trace, 48_000.0);
    assert_eq!(actual, expected);
    assert_eq!(state, apu_state(&emu.bus.apu));

    let routine = [0xa9, 0x40, 0x8d, 0x11, 0x40, 0xad, 0x15, 0x40, 0x4c, 0, 2];
    let mut program = vec![
        0xa2,
        routine.len() as u8 - 1,
        0xbd,
        0,
        0x81,
        0x9d,
        0,
        2,
        0xca,
        0x10,
        0xf7,
        0x4c,
        0,
        2,
    ];
    program.resize(0x100, 0);
    program.extend(routine);
    let rom = self::rom(0, 0, &program, true);
    let mut emu = Emulator::new_with_audio_trace(&rom, 48_000.0, 8192).unwrap();
    run(&mut emu, 1000);
    let trace = emu.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    assert!(!trace.events.is_empty());
    assert!(trace.events.iter().all(|event| event.instruction_source
        == AudioTraceSource::WorkRam {
            offset: event.pc & 0x7ff
        }));
}

#[test]
fn apu_factory_rejects_changed_clock_reset_and_initial_phase_fields() {
    let chip = native_trace_chip(crate::hardware::timing::NesTiming::Ntsc);
    macro_rules! reject {
        ($field:ident,$value:expr) => {{
            let mut altered = chip;
            altered.$field = $value;
            assert!(Apu::new_for_audio_trace(&altered, 48_000.0).is_err());
        }};
    }
    reject!(clock_hz_numerator, 1);
    reject!(clock_hz_denominator, 0);
    reject!(initial_cpu_cycle, 0);
    reject!(initial_cpu_cycle_odd, false);
    reject!(initial_apu_frame_cycle, 0);
    reject!(initial_half_rate_timer_clock, true);
}

#[test]
fn interrupt_status_reads_do_not_claim_instruction_provenance() {
    use crate::hardware::cpu::StatusFlags;

    let rom = rom(0, 0, &program(), false);
    for nmi in [false, true] {
        let mut baseline = Emulator::new(&rom, 48_000.0).unwrap();
        let mut capture = Emulator::new_with_audio_trace(&rom, 48_000.0, 8192).unwrap();
        for emu in [&mut baseline, &mut capture] {
            emu.cpu.pc = 0x4015;
            emu.cpu.nmi_pending = nmi;
            emu.cpu.irq_line = !nmi;
            emu.cpu.regs.set_flag(StatusFlags::INTERRUPT, false);
            emu.step_instruction();
        }
        assert_eq!(
            capture.encode_state().unwrap(),
            baseline.encode_state().unwrap()
        );
        let trace = capture.finish_audio_trace().unwrap();
        trace.validate_complete().unwrap();
        assert_eq!(trace.events.len(), 2);
        for event in &trace.events {
            assert_eq!(event.pc, 0);
            assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
            assert!(matches!(
                event.write,
                NesTraceWrite::StatusRead {
                    origin: NesTraceOrigin::CpuNonInstruction,
                    ..
                }
            ));
        }
        let (samples, state) = replay(&trace, 48_000.0);
        assert_eq!(samples, baseline.drain_audio_samples());
        assert_eq!(state, apu_state(&baseline.bus.apu));
    }
}

#[test]
fn failed_partial_state_load_restores_the_host_trace_and_direct_decode_invalidates_it() {
    let rom = rom(0, 0, &program(), false);
    let mut emu = Emulator::new_with_audio_trace(&rom, 48_000.0, 8192).unwrap();
    let state = emu.encode_state().unwrap();
    let mut payload = lz4_flex::decompress_size_prepended(&state[12..]).unwrap();
    payload.push(0);
    let mut invalid = state[..12].to_vec();
    invalid.extend(lz4_flex::compress_prepend_size(&payload));
    run(&mut emu, 1000);
    let before_state = emu.encode_state().unwrap();
    let before_trace = emu.bus.audio_trace.clone().finish(emu.cpu_cycles() - 7);
    assert!(emu.load_state(&invalid).is_err());
    assert_eq!(emu.encode_state().unwrap(), before_state);
    assert_eq!(
        emu.bus.audio_trace.clone().finish(emu.cpu_cycles() - 7),
        before_trace
    );
    crate::save_state::decode_state(&mut emu, &state).unwrap();
    assert_eq!(
        emu.finish_audio_trace().unwrap().invalidated,
        Some(AudioTraceInvalidation::StateRestore)
    );
}
