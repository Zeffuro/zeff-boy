use super::*;

fn source_step(backend: &mut EmuBackend, offset: u64) -> FrameResult {
    let mut actions = DebugUiActions::none();
    actions.add_rom_breakpoints.push(offset);
    let mut input = guest_call_input(0);
    input.debug_actions = actions;
    input.debug_step = true;
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut RewindBuffer::new(0, 1),
        &mut 0,
        &mut WorkerRuntimeFault::default(),
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_guest_entry_fetch_is_not_a_hit_and_real_final_rts_rolls_back_with_prior_undo() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        let mut actions = DebugUiActions::none();
        actions.add_rom_breakpoints.push(u64::from(RETURN_PC));
        configure(&mut owner.backend, &actions);
        let original = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let input = guest_call_input(0);
        let view =
            EmuThread::collect_ui_snapshot(&mut owner.backend, &input.snapshot, input.buffers);
        assert_eq!(view.cpu_debug.unwrap().hit_rom_breakpoint, None);
        let mut actions = DebugUiActions::none();
        actions.add_rom_breakpoints.push(0x1016);
        configure(&mut owner.backend, &actions);
        let before = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let host = session(&mut owner.backend);
        let markers = owner.markers.clone();
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
        assert!(!effects.potentially_dirty);
        let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
            panic!("real RTS breakpoint did not reject the call");
        };
        assert!(error.contains("debugger stop") && error.contains("state restored"));
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            before
        );
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(owner.markers, markers);
        assert!(owner.backend.take_runtime_fault().is_none());
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            original
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_guest_undo_keeps_sampled_queue_and_current_external_code() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        let checkpoint = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let effects = owner.dispatch(EmuCommand::UpdateCheats(
            crate::emu_thread::shared::md_rom_cheat_tests::codes("00021C:7009"),
        ));
        assert!(effects.response.is_none());
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            checkpoint
        );
        assert_eq!(machine(&owner.backend).peek(RETURN_PC), Some(0x70));
        let old = source_step(&mut owner.backend, u64::from(RETURN_PC));
        assert!(old.runtime_fault.is_none());
        let cpu = old.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_rom_breakpoint, Some(u64::from(RETURN_PC)));
        assert_eq!(cpu.recent_opcodes[0].bytes, [0x60, 0xfe]);
        let next = source_step(&mut owner.backend, u64::from(RETURN_PC));
        assert!(next.runtime_fault.is_none());
        let cpu = next.ui_data.cpu_debug.unwrap();
        assert_eq!(cpu.hit_rom_breakpoint, Some(u64::from(RETURN_PC)));
        assert_eq!(cpu.recent_opcodes[0].bytes, [0x70, 9]);
        assert_eq!(machine(&owner.backend).registers().d[0], 9);
    }
}
