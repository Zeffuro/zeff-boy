use super::*;

fn active_rom(color: bool, halt: bool) -> Vec<u8> {
    let mut code = Vec::new();
    for (port, value) in [
        (0x40, 0),
        (0x41, 0x10),
        (0x42, 3),
        (0x44, 0xc0),
        (0x45, 0x3f),
        (0x46, 64),
        (0x47, 0),
        (0x48, 0x80),
        (0x8f, 0xff),
        (0x80, 0),
        (0x81, 7),
        (0x88, 0xf1),
        (0x90, 0x23),
        (0x94, 5),
        (0x91, 0x0f),
        (0x4a, 0),
        (0x4b, 0x10),
        (0x4c, 3),
        (0x4e, 64),
        (0x4f, 0),
        (0x50, 0),
        (0x52, 0x8b),
    ] {
        out(&mut code, port, value);
    }
    if halt {
        code.push(0xf4);
    } else {
        let start = code.len();
        code.extend_from_slice(&[0x26, 0xa2, 0xc0, 0x3f, 0x40]);
        code.extend_from_slice(&[0x90; 20]);
        let displacement = -((code.len() - start + 2) as i8);
        code.extend_from_slice(&[0xeb, displacement as u8]);
    }
    let mut bytes = rom(&code, color);
    for (index, value) in bytes[0x1000..0x1040].iter_mut().enumerate() {
        *value = (index as u8).wrapping_mul(29).wrapping_add(7);
    }
    checksum(&mut bytes);
    bytes
}

fn assert_native_equal(plain: &mut Emulator, captured: &mut Emulator) -> Vec<f32> {
    assert_eq!(
        captured.encode_state().unwrap(),
        plain.encode_state().unwrap()
    );
    assert_eq!(captured.cpu_cycles(), plain.cpu_cycles());
    assert_eq!(captured.bus.cycles, plain.bus.cycles);
    assert_eq!(captured.last_fetch(), plain.last_fetch());
    assert_eq!(captured.last_trap(), plain.last_trap());
    assert_eq!(captured.frame_count(), plain.frame_count());
    assert_eq!(captured.framebuffer(), plain.framebuffer());
    assert_eq!(captured.bus.apu.save_state(), plain.bus.apu.save_state());
    assert_eq!(captured.apu_debug_snapshot(), plain.apu_debug_snapshot());
    assert_eq!(
        captured.apu_master_debug_samples_ordered(),
        plain.apu_master_debug_samples_ordered()
    );
    for channel in 0..4 {
        assert_eq!(
            captured.apu_channel_debug_samples_ordered(channel),
            plain.apu_channel_debug_samples_ordered(channel)
        );
    }
    assert_eq!(
        captured.hlt_fast_forward_calls,
        plain.hlt_fast_forward_calls
    );
    assert_eq!(
        captured.bus.frame_service_state_for_test(),
        plain.bus.frame_service_state_for_test()
    );
    #[cfg(feature = "profiling")]
    assert_eq!(captured.profiling_snapshot(), plain.profiling_snapshot());
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    plain.drain_audio_samples_into(&mut expected);
    captured.drain_audio_samples_into(&mut actual);
    assert_eq!(
        actual
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    actual
}

#[test]
fn enabled_trace_preserves_full_frames_pcm_state_and_service_strategy() {
    for color in [false, true] {
        for halt in [false, true] {
            for generation in [false, true] {
                let bytes = active_rom(color, halt);
                let mut plain = Emulator::new(&bytes, 48_000).unwrap();
                plain.set_apu_sample_generation_enabled(generation);
                let mut captured = plain.clone();
                captured.reset_and_begin_audio_trace(20_000).unwrap();
                let mut nonzero = false;
                for frame in 0..3 {
                    plain.set_input(frame, frame << 1);
                    captured.set_input(frame, frame << 1);
                    plain.step_frame();
                    captured.step_frame();
                    let pcm = assert_native_equal(&mut plain, &mut captured);
                    nonzero |= pcm.iter().any(|&sample| sample != 0.0);
                    assert_eq!(captured.bus.frame_service_state_for_test(), (false, 0, 0));
                }
                assert_eq!(nonzero, generation);
                assert_eq!(captured.hlt_fast_forward_calls > 0, halt);
                if color {
                    assert_eq!(captured.bus.cycles - captured.cpu_cycles(), 69);
                }
                #[cfg(feature = "profiling")]
                assert!(captured.profiling_snapshot().frame_service_deferred_calls > 0);
                let trace = captured.finish_audio_trace().unwrap();
                trace.validate_complete().unwrap();
                assert_eq!(trace.end_cycle, captured.bus.cycles);
                assert!(trace.events.iter().any(|event| matches!(
                    event.write,
                    Write::Register {
                        origin: Origin::Cpu,
                        ..
                    }
                )));
                assert_eq!(!dma_events(&trace).is_empty(), color);
                assert_eq!(
                    trace.events.iter().any(|event| matches!(
                        event.write,
                        Write::WaveRam {
                            origin: Origin::GeneralDma,
                            ..
                        }
                    )),
                    color
                );
            }
        }
    }
}

#[test]
fn deferred_wave_mutation_does_not_add_a_trace_only_fence() {
    let mut plain = emulator(&[0xf4], true);
    let mut captured = plain.clone();
    captured.reset_and_begin_audio_trace(10).unwrap();
    for emu in [&mut plain, &mut captured] {
        emu.bus.io_write8(0x88, 0xff);
        emu.bus.io_write8(0x90, 1);
        emu.bus.begin_frame_service();
        emu.bus.step_cycles(31);
        emu.bus.write8(0, 0xff);
        assert_eq!(emu.bus.frame_service_state_for_test().1, 31);
        emu.bus.step_cycles(32);
        emu.bus.write8(0, 0x0f);
        assert_eq!(emu.bus.frame_service_state_for_test().1, 63);
        emu.bus.step_cycles(1);
        emu.bus.end_frame_service();
    }
    let pcm = assert_native_equal(&mut plain, &mut captured);
    assert_eq!(pcm.len(), 2);
    let trace = captured.finish_audio_trace().unwrap();
    let writes = trace
        .events
        .iter()
        .filter_map(|event| match event.write {
            Write::WaveRam {
                address: 0, value, ..
            } => Some((event.cycle, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes, vec![(31, 0xff), (63, 0x0f)]);
    assert_eq!(trace.end_cycle, 64);
}
