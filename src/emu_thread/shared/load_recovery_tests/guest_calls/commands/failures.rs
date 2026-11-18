use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_common_failures_roll_back_real_effects_and_preserve_previous_undo() {
    for case in 0..16 {
        let mut owner = Owner::new(standard());
        let original = frontend(&owner.backend);
        let original_host = session(&mut owner.backend);
        let token = owner.call();
        let mut actions = DebugUiActions::none();
        let mut call = request(0x1000, 8);
        let reason = match case {
            0 => {
                call = request(0x1200, 4);
                "budget"
            }
            1 => {
                call = request(0x1300, 8);
                "STOP"
            }
            2 => {
                call = request(0x1400, 8);
                "machine fault"
            }
            3 => {
                call = request(0x1200, 8);
                actions.add_breakpoint = Some(0x1208);
                "debugger stop"
            }
            4 => {
                call = request(0x1200, 8);
                actions.add_one_shot_breakpoint = Some(0x1210);
                "debugger stop"
            }
            5 => {
                call = request(0x1200, 8);
                actions.add_watchpoint = Some((0xff0042, 0xff0042, WatchType::Write));
                "debugger stop"
            }
            6 => {
                call = request(0x1500, 8);
                actions
                    .event_breakpoint_changes
                    .push((DebugEvent::Exception, true));
                "debugger stop"
            }
            7 => {
                call.target = 0x1001;
                call.storage_offset = None;
                "rejected"
            }
            8 => {
                call.target = 0x100_0000;
                call.storage_offset = None;
                "rejected"
            }
            9 => {
                call.target = 0xc00000;
                call.storage_offset = None;
                "rejected"
            }
            10 => {
                call.exec_mode = crate::symbols::ExecMode::Z80;
                "M68000"
            }
            11 => {
                call.storage_offset = Some(0x1002);
                "no longer maps"
            }
            12 => {
                call.instruction_budget = 0;
                "positive"
            }
            13 => {
                call.target = machine(&owner.backend).registers().pc;
                call.storage_offset = None;
                "current PC"
            }
            14 => {
                assert!(machine_mut(&mut owner.backend).set_interrupt_level(7));
                "rejected"
            }
            _ => {
                let mut config = BackendRuntimeConfig::new(&actions);
                config.debug_continue = true;
                owner.backend.apply_runtime_config(config);
                "suspended"
            }
        };
        configure(&mut owner.backend, &actions);
        let before = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let machine = machine(&owner.backend).clone();
        let markers = owner.markers.clone();
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(call));
        assert!(!effects.potentially_dirty, "case {case}");
        let Some(EmuResponse::GuestCallFailed { name, error }) = effects.response else {
            panic!("failure case {case} completed")
        };
        assert_eq!(name, "nested");
        assert!(error.contains(reason), "case {case}: {error}");
        assert_eq!(frontend(&owner.backend), before, "case {case}");
        assert_eq!(super::machine(&owner.backend), &machine, "case {case}");
        assert_eq!(session(&mut owner.backend), host, "case {case}");
        assert_eq!(owner.markers, markers);
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        assert!(owner.fault.can_step());
        assert!(owner.backend.take_runtime_fault().is_none());
        owner.backend.debug_suspend();
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), original, "case {case}");
        assert_eq!(session(&mut owner.backend), original_host, "case {case}");
    }
}
