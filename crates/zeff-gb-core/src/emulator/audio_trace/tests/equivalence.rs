use super::*;

fn sound_program(cgb: bool) -> Vec<u8> {
    let mut code = Vec::new();
    store(&mut code, 0xff26, 0);
    store(&mut code, 0xff26, 0x80);
    for (address, value) in [
        (0xff24, 0x77),
        (0xff25, 0xff),
        (0xff11, 0x80),
        (0xff12, 0xf3),
        (0xff13, 0x40),
        (0xff14, 0x87),
        (0xff21, 0x93),
        (0xff22, 0x35),
        (0xff23, 0x80),
    ] {
        store(&mut code, address, value);
    }
    if cgb {
        store(&mut code, 0xff4d, 1);
        code.extend_from_slice(&[0x10, 0]);
    }
    store(&mut code, 0xff04, 0);
    code.extend_from_slice(&[0x18, 0xfe]);
    code
}

#[test]
fn capture_preserves_native_state_pcm_and_frames_on_fast_and_debug_paths() {
    for cgb in [false, true] {
        for debug in [false, true] {
            let code = sound_program(cgb);
            let mut plain = emulator(&code, cgb);
            let mut captured = emulator(&code, cgb);
            captured.reset_and_begin_audio_trace(20_000).unwrap();
            plain.set_instruction_trace_enabled(debug);
            captured.set_instruction_trace_enabled(debug);
            for _ in 0..8 {
                plain.step_frame();
                captured.step_frame();
                assert_eq!(
                    plain.encode_state_bytes().unwrap(),
                    captured.encode_state_bytes().unwrap()
                );
                assert_eq!(plain.drain_audio_samples(), captured.drain_audio_samples());
                assert_eq!(plain.framebuffer(), captured.framebuffer());
                assert_eq!(plain.frame_count, captured.frame_count);
            }
            let trace = captured.finish_audio_trace().unwrap();
            assert_eq!(trace.end_cycle, captured.cycle_count);
            assert!(!trace.events.is_empty());
            trace.validate_complete().unwrap();
        }
    }
}

#[test]
fn frame_slices_and_instruction_steps_capture_the_same_events() {
    let code = sound_program(false);
    let mut sliced = traced(&code, false);
    let mut stepped = traced(&code, false);
    let mut cursor = sliced.begin_frame_slice();
    let _ = sliced.step_frame_slice_until(&mut cursor, |emu| emu.cycle_count >= 20_000);
    while stepped.cycle_count < sliced.cycle_count {
        stepped.step_instruction();
    }
    assert_eq!(
        sliced.finish_audio_trace().unwrap(),
        stepped.finish_audio_trace().unwrap()
    );
}

#[test]
fn output_collection_and_sample_generation_do_not_change_chip_events() {
    let code = sound_program(true);
    let mut enabled = traced(&code, true);
    let mut disabled = traced(&code, true);
    disabled.set_apu_sample_generation_enabled(false);
    disabled.set_sample_rate(96_000);
    disabled.set_apu_debug_capture_enabled(true);
    for _ in 0..4 {
        enabled.step_frame();
        disabled.step_frame();
        enabled.drain_audio_samples();
    }
    assert_eq!(
        enabled.finish_audio_trace().unwrap(),
        disabled.finish_audio_trace().unwrap()
    );
}
