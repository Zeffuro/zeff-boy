use super::*;

fn actions() -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.memory_writes = vec![
        (0xff0041, 0xa5),
        (0xa10009, 0xc0),
        (0xa10003, 0x80),
        (0xa1000b, 0x81),
        (0xa10005, 0x80),
        (0xa1000d, 0xff),
        (0xa10007, 0xd5),
    ];
    actions
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_editor_keeps_guest_undo_and_rewind_restores_prior_full_input_with_current_overlay() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::SixButton, &[]));
        let original = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let original_host = session(&mut owner.backend);
        let token = owner.call();
        let called = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let called_host = session(&mut owner.backend);
        let history_len = owner.rewind.len();
        let actions = actions();
        assert_eq!(
            owner.backend.debugger_memory_mutation(&actions, true),
            Ok(true)
        );
        configure(&mut owner.backend, &actions);
        assert_eq!(session(&mut owner.backend), called_host);
        assert_eq!(owner.rewind.len(), history_len);
        assert_ne!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            called
        );
        let overlay = crate::cheats::parse_cheat_for_system("001800:1234", ActiveSystem::Md)
            .unwrap()
            .0;
        machine_mut(&mut owner.backend)
            .install_rom_patches(&overlay)
            .unwrap();
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            original
        );
        assert_eq!(session(&mut owner.backend), original_host);
        assert_eq!(machine(&owner.backend).peek(0x1800), Some(0x12));
        assert!(owner.backend.is_suspended());
        assert!(owner.backend.encode_rewind_checkpoint().is_err());
        let clock = machine(&owner.backend).cpu_clock();
        let mut resume_actions = DebugUiActions::none();
        resume_actions
            .remove_breakpoints
            .push(machine(&owner.backend).registers().pc & 0xffffff);
        let mut config = BackendRuntimeConfig::new(&resume_actions);
        config.opcode_log_enabled = true;
        config.debug_continue = true;
        owner.backend.apply_runtime_config(config);
        assert!(!owner.backend.is_suspended());
        assert_eq!(machine(&owner.backend).cpu_clock(), clock);
        let checkpoint = owner.backend.encode_rewind_checkpoint().unwrap();
        let input = machine(&owner.backend).input().clone();
        owner.backend.debug_suspend();
        configure(&mut owner.backend, &actions);
        assert_ne!(machine(&owner.backend).input(), &input);
        owner
            .backend
            .restore_rewind_checkpoint(checkpoint.clone())
            .unwrap();
        assert_eq!(
            owner.backend.encode_rewind_checkpoint().unwrap(),
            checkpoint
        );
        assert_eq!(machine(&owner.backend).input(), &input);
        assert_eq!(machine(&owner.backend).peek(0x1800), Some(0x12));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_editor_preflight_and_commit_keep_flags_then_continue_order_and_exact_future() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut subject = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        let mut control = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        assert_eq!(frontend(&subject.backend), frontend(&control.backend));
        let mut actions = actions();
        actions.trace_enabled = Some(false);
        actions.apu_channel_mutes = Some(vec![false; 10]);
        actions
            .remove_breakpoints
            .push(machine(&subject.backend).registers().pc);
        assert_eq!(
            subject.backend.debugger_memory_mutation(&actions, true),
            Ok(true)
        );
        let clock = machine(&subject.backend).cpu_clock();
        let queue = machine(&subject.backend).queued_words();
        let mut config = BackendRuntimeConfig::new(&actions);
        config.opcode_log_enabled = true;
        config.debug_continue = true;
        subject.backend.apply_runtime_config(config);
        machine_mut(&mut control.backend)
            .debug_write_memory(&actions.memory_writes)
            .unwrap();
        actions.memory_writes.clear();
        let mut config = BackendRuntimeConfig::new(&actions);
        config.opcode_log_enabled = true;
        config.debug_continue = true;
        control.backend.apply_runtime_config(config);
        assert!(!subject.backend.is_suspended() && !control.backend.is_suspended());
        assert_eq!(machine(&subject.backend).cpu_clock(), clock);
        assert_eq!(machine(&subject.backend).queued_words(), queue);
        assert_eq!(frontend(&subject.backend), frontend(&control.backend));
        for _ in 0..2 {
            subject.backend.step_frame();
            control.backend.step_frame();
            assert!(!subject.backend.is_suspended() && !control.backend.is_suspended());
            assert_eq!(frontend(&subject.backend), frontend(&control.backend));
            assert_eq!(
                machine(&subject.backend).encode_state().unwrap(),
                machine(&control.backend).encode_state().unwrap()
            );
            assert_eq!(subject.backend.framebuffer(), control.backend.framebuffer());
            let left = machine_mut(&mut subject.backend).drain_audio_samples();
            let right = machine_mut(&mut control.backend).drain_audio_samples();
            assert!(left.iter().any(|sample| *sample != 0.0));
            assert!(
                left.iter()
                    .map(|sample| sample.to_bits())
                    .eq(right.iter().map(|sample| sample.to_bits()))
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_editor_refuses_running_faulted_and_invalid_mixed_requests_without_mutation() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture(b'U', timing, ControllerKind::ThreeButton, &[]);
        let actions = actions();
        let before = machine(&backend).clone();
        assert!(backend.debugger_memory_mutation(&actions, true).is_err());
        configure(&mut backend, &actions);
        assert_eq!(machine(&backend), &before);
        backend.debug_suspend();
        let before = machine(&backend).clone();
        assert!(backend.debugger_memory_mutation(&actions, false).is_err());
        assert_eq!(machine(&backend), &before);
        let mut actions = actions;
        actions.memory_writes.push((0xa10001, 0));
        assert!(backend.debugger_memory_mutation(&actions, true).is_err());
        configure(&mut backend, &actions);
        assert_eq!(machine(&backend), &before);
        actions.memory_writes.clear();
        assert_eq!(backend.debugger_memory_mutation(&actions, false), Ok(false));
        assert_eq!(machine(&backend), &before);
    }
}
