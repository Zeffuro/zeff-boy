use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn external_input_edits_preserve_enabled_and_disabled_tracker_until_actual_returns() {
    for (timing, concurrent) in PROFILES {
        for enabled in [false, true] {
            let mut subject = fixture(timing, concurrent);
            let mut control = fixture(timing, concurrent);
            if enabled {
                enter(&mut subject);
                enter(&mut control);
            } else {
                for _ in 0..5 {
                    subject.step_instruction().unwrap();
                    control.step_instruction().unwrap();
                }
            }
            let expected_frames = frames(&subject);
            let status = subject.call_stack_status();
            assert_eq!(expected_frames.len(), if enabled { 2 } else { 0 });
            let before = subject.clone();
            let writes = [(0xa10009, 0x40), (0xa10003, 0), (0xff0041, 0xa5)];
            subject.debug_write_memory(&writes).unwrap();
            assert_eq!(frames(&subject), expected_frames);
            assert_eq!(subject.call_stack_status(), status);
            let mut normalized = subject.clone();
            normalized.bus.input = before.bus.input.clone();
            normalized.bus.ram = before.bus.ram.clone();
            assert_eq!(
                normalized.encode_debugger_checkpoint().unwrap(),
                before.encode_debugger_checkpoint().unwrap()
            );
            let owned = subject.encode_debugger_checkpoint().unwrap();
            let frontend = subject.encode_frontend_state().unwrap();
            assert_eq!(
                subject.debug_write_memory(&[(0xa10003, 0x40), (0xff0041, 0x55), (0xa10002, 0)]),
                Err(0xa10002)
            );
            assert_eq!(subject.encode_debugger_checkpoint().unwrap(), owned);
            assert_eq!(subject.encode_frontend_state().unwrap(), frontend);
            assert_eq!(frames(&subject), expected_frames);
            control.debug_write_memory(&writes).unwrap();
            for _ in 0..3 {
                assert_eq!(
                    subject.step_instruction().unwrap(),
                    control.step_instruction().unwrap()
                );
                assert_eq!(frames(&subject), frames(&control));
                assert_eq!(subject.call_stack_status(), control.call_stack_status());
                assert_eq!(
                    subject.encode_debugger_checkpoint().unwrap(),
                    control.encode_debugger_checkpoint().unwrap()
                );
            }
            assert!(frames(&subject).is_empty());
            assert_eq!(
                subject.call_stack_status(),
                if enabled {
                    CallStackStatus::Observed { truncated: false }
                } else {
                    CallStackStatus::Disabled
                }
            );
        }
    }
}
