use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn receive_uart_safe_same_value_and_electrical_rejection_remain_recoverable() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for (port, data, ctrl, serial) in [
                (ControllerPort::One, 0xa10003, 0xa10009, 0x0013),
                (ControllerPort::Two, 0xa10005, 0xa1000b, 0x0019),
            ] {
                let words = [
                    0x13fc,
                    0x40,
                    0x00a1,
                    ctrl as u16,
                    0x13fc,
                    0x20,
                    0x00a1,
                    serial,
                    0x60fe,
                    0x4e71,
                ];
                let mut emu = program(region, timing, ControllerKind::SixButton, &words);
                emu.input_mut().set_controller_raw(port, 0x7f);
                emu.step_instruction().unwrap();
                emu.step_instruction().unwrap();
                assert_eq!(emu.peek(0xa10000 + u32::from(serial)), Some(0x20));
                let states = versions(&emu);
                emu.debug_write_memory(&[(ctrl, 0x40), (data, 0x7f)])
                    .unwrap();
                assert_eq!(versions(&emu), states);
                for writes in [
                    vec![(0xff0041, 0x55), (0xa10007, 0x22), (data, 0)],
                    vec![(0xff0041, 0x55), (ctrl, 0x40), (data, 0), (data, 0x7f)],
                ] {
                    let before = emu.clone();
                    assert_eq!(emu.validate_debug_memory_writes(&writes), Err(data));
                    assert_eq!(emu.debug_write_memory(&writes), Err(data));
                    assert_eq!(emu, before);
                    assert_eq!(versions(&emu), states);
                    assert_eq!(emu.input_fault(), None);
                }
                emu.step_instruction().unwrap();
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn active_real_uart_tx_keeps_same_value_edits_and_rejects_mixed_changes_atomically() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for (data, ctrl, status, transmit) in [
                (0xa10003, 0xa10009, 0x13, 0x0f),
                (0xa10005, 0xa1000b, 0x19, 0x15),
            ] {
                let mut words = vec![0x4e71; 1000];
                words.extend([
                    0x13fc, 0x38, 0x00a1, status, 0x13fc, 0x53, 0x00a1, transmit, 0x60fe, 0x4e71,
                ]);
                let mut emu = program(region, timing, ControllerKind::ThreeButton, &words);
                for _ in 0..1002 {
                    emu.step_instruction().unwrap();
                }
                assert_eq!(emu.peek(0xa10000 + u32::from(status)), None);
                assert_eq!(emu.peek(data), None);
                let states = versions(&emu);
                emu.debug_write_memory(&[(data, 0x7f), (ctrl, 0)]).unwrap();
                assert_eq!(versions(&emu), states);
                let writes = [
                    (0xff0041, 0xa5),
                    (0xa1000d, 0x7f),
                    (0xa10007, 0x33),
                    (data, 0x7e),
                ];
                let before = emu.clone();
                assert_eq!(emu.validate_debug_memory_writes(&writes), Err(data));
                assert_eq!(emu.debug_write_memory(&writes), Err(data));
                assert_eq!(emu, before);
                assert_eq!(versions(&emu), states);
                assert_eq!(emu.input_fault(), None);
                emu.step_instruction().unwrap();
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn only_accepted_nonempty_external_edits_invalidate_audio_capture() {
    let mut emu = machine(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    );
    emu.reset_and_begin_audio_trace(16).unwrap();
    let before = emu.clone();
    emu.debug_write_memory(&[]).unwrap();
    assert_eq!(
        emu.debug_write_memory(&[(0xa10009, 0x40), (0xa10002, 0)]),
        Err(0xa10002)
    );
    assert_eq!(emu, before);
    assert!(
        emu.clone()
            .finish_audio_trace()
            .unwrap()
            .trace
            .invalidated
            .is_none()
    );
    emu.debug_write_memory(&[(0xa10009, 0)]).unwrap();
    let mut normalized = emu.clone();
    normalized.bus.audio.trace = before.bus.audio.trace.clone();
    assert_eq!(normalized, before);
    assert_eq!(
        emu.finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
}
