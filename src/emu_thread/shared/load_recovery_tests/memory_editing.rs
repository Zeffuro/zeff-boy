use super::*;

fn deliver(thread: &EmuThread, request: FrameInput) -> FrameResult {
    thread.send(EmuCommand::StepFrames(Box::new(request)));
    receive_frame(thread)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_memory_edit_normal_worker_break_edit_capture_load_and_continue() {
    let subject = EmuThread::spawn(backend(), false);
    let mut request = input(4, 0, 0);
    request.snapshot.want_debug_info = true;
    let running = deliver(&subject, request);
    assert_eq!(running.advanced_frames, 4);
    assert!(pcm(&running).iter().any(|&bits| bits != 0));
    let pc = running.ui_data.cpu_debug.unwrap().pc;
    assert!(
        !running
            .ui_data
            .core_features
            .unwrap()
            .memory_editing
            .unwrap()
            .enabled
    );
    let mut request = input(1, 0, 0);
    request.snapshot.want_debug_info = true;
    request.debug_actions.add_breakpoint = Some(pc);
    let stopped = deliver(&subject, request);
    assert_eq!(stopped.advanced_frames, 0);
    assert_eq!(stopped.ui_data.cpu_debug.unwrap().hit_breakpoint, Some(pc));
    let features = stopped.ui_data.core_features.unwrap();
    assert!(features.memory_editing.unwrap().enabled);
    assert!(features.supports_debugger && features.supports_guest_calls);
    let before = capture(&subject);
    let mut request = input(0, 0, 0);
    request.debug_actions.memory_writes = vec![(0xff0041, 0x77), (0xa00000, 0x12)];
    assert!(deliver(&subject, request).runtime_fault.is_none());
    assert!(matches!(
        receive_response(&subject),
        EmuResponse::MemoryWriteRejected(_)
    ));
    assert_eq!(capture(&subject), before);
    let writes = [
        (0xff0000, 0x12),
        (0xff0041, 0x55),
        (0xffffff, 0x34),
        (0xff0041, 0xa5),
    ];
    let mut request = input(0, 0, 0);
    request.snapshot.want_debug_info = true;
    request.snapshot.show_memory_viewer = true;
    request.snapshot.memory_view_start = 0xff0000;
    request.debug_actions.memory_writes = writes.to_vec();
    request.debug_actions.add_watchpoint =
        Some((0xff0041, 0xff0041, zeff_emu_common::debug::WatchType::Write));
    let edited = deliver(&subject, request);
    assert_eq!(edited.advanced_frames, 0);
    assert!(edited.runtime_fault.is_none());
    let cpu = edited.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(cpu.pc, pc);
    assert_eq!(cpu.hit_breakpoint, Some(pc));
    assert!(cpu.hit_watchpoint.is_none());
    assert!(
        edited
            .ui_data
            .memory_page
            .unwrap()
            .contains(&(0xff0041, 0xa5))
    );
    let saved = capture(&subject);
    assert_eq!(capture(&subject), saved);
    let mut expected = backend();
    expected.load_state_from_bytes(before).unwrap();
    let EmuBackend::Md(md) = &mut expected else {
        unreachable!()
    };
    md.test_normal_core_mut()
        .unwrap()
        .debug_write_ram(&writes)
        .unwrap();
    assert_eq!(expected.encode_state_bytes().unwrap(), saved);
    let control = EmuThread::spawn(backend(), false);
    assert!(matches!(
        load(&control, saved.clone()),
        EmuResponse::LoadStateOk { .. }
    ));
    assert!(matches!(
        load(&subject, saved),
        EmuResponse::LoadStateOk { .. }
    ));
    let mut request = input(0, 0, 0);
    request.debug_actions.remove_breakpoints.push(pc);
    assert_eq!(deliver(&subject, request).advanced_frames, 0);
    for _ in 0..3 {
        let actual = step(&subject, 1, 0, 0);
        let expected = step(&control, 1, 0, 0);
        assert_eq!(actual.advanced_frames, 1);
        assert!(actual.runtime_fault.is_none());
        assert_eq!(pcm(&actual), pcm(&expected));
        assert_eq!(capture(&subject), capture(&control));
        assert_eq!(
            subject.shared_framebuffer().load_full(),
            control.shared_framebuffer().load_full()
        );
    }
}
