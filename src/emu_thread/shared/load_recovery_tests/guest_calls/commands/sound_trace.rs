use super::*;
use crate::ui::{InstructionTraceBatch, TraceCursor, TraceTarget};

fn owner(header: u8) -> Owner {
    let mut setup = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
    let program = [
        0x3e, 0x5a, 0x32, 0, 0x10, 0, 0x3e, 0x80, 0x32, 0x11, 0x7f, 0x3e, 0x10, 0x32, 0x11, 0x7f,
        0x3e, 0x90, 0x32, 0x11, 0x7f, 0x18, 0xfe,
    ];
    for (address, byte) in program.into_iter().enumerate() {
        setup.extend([0x13fc, byte, 0xa0, address as u16]);
    }
    setup.extend([
        0x33fc, 0x100, 0xa1, 0x1200, 0x33fc, 0, 0xa1, 0x1100, 0x4ef9, 0, 0x200,
    ]);
    Owner::new(fixture(
        header,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
        &[(4, &[0, 0x1900]), (0x1900, &setup)],
    ))
}

fn enable(owner: &mut Owner, value: bool) {
    let mut actions = DebugUiActions::none();
    actions.sound_trace.enabled = Some(value);
    actions.sound_trace.capacity = Some(64);
    configure(&mut owner.backend, &actions);
}

fn snapshot(owner: &mut Owner) -> InstructionTraceBatch {
    let mut input = guest_call_input(0);
    input.snapshot.trace_cursor = TraceCursor {
        target: TraceTarget::Sound,
        ..Default::default()
    };
    EmuThread::collect_ui_snapshot(&mut owner.backend, &input.snapshot, input.buffers)
        .instruction_trace
        .unwrap()
}

fn unchanged(owner: &mut Owner, before: &InstructionTraceBatch) {
    let after = snapshot(owner);
    assert_eq!(after.stream, before.stream);
    assert_eq!(after.entries, before.entries);
    assert_eq!(
        (after.enabled, after.capacity),
        (before.enabled, before.capacity)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_guest_call_failures_restore_pair_and_undo_keeps_current_mask_with_honest_gap() {
    for header in *b"UJE" {
        for mask in [Some("001800:1234"), Some("001800:ABCD"), None] {
            let mut subject = owner(header);
            let mut control = owner(header);
            let original = crate::cheats::parse_cheat_for_system("001800:1234", ActiveSystem::Md)
                .unwrap()
                .0;
            subject.backend.try_install_rom_patches(&original).unwrap();
            control.backend.try_install_rom_patches(&original).unwrap();
            enable(&mut subject, true);
            let token = subject.call();
            let control_token = control.call();
            trace_stream::compare_machine(&subject, &control);
            let before = snapshot(&mut subject);
            assert!(!before.entries.is_empty());
            for (target, budget) in [(0x1200, 4), (0x1300, 8), (0x1000, 8)] {
                let mut watch = DebugUiActions::none();
                if target == 0x1000 {
                    watch.add_watchpoint = Some((0xff0040, 0xff0040, WatchType::Write));
                }
                configure(&mut subject.backend, &watch);
                configure(&mut control.backend, &watch);
                let effects =
                    subject.dispatch(EmuCommand::ExecuteGuestCall(request(target, budget)));
                assert!(matches!(
                    effects.response,
                    Some(EmuResponse::GuestCallFailed { .. })
                ));
                assert!(!effects.potentially_dirty);
                unchanged(&mut subject, &before);
                trace_stream::compare_machine(&subject, &control);
            }
            let mut clear_watch = DebugUiActions::none();
            clear_watch
                .remove_watchpoints
                .push((0xff0040, 0xff0040, WatchType::Write));
            configure(&mut subject.backend, &clear_watch);
            configure(&mut control.backend, &clear_watch);
            subject.refused_undo(vec![0; 16]);
            unchanged(&mut subject, &before);
            let next = mask
                .map(|code| {
                    crate::cheats::parse_cheat_for_system(code, ActiveSystem::Md)
                        .unwrap()
                        .0
                })
                .unwrap_or_default();
            subject.backend.try_install_rom_patches(&next).unwrap();
            control.backend.try_install_rom_patches(&next).unwrap();
            unchanged(&mut subject, &before);
            subject.undo(token);
            control.undo(control_token);
            trace_stream::compare_machine(&subject, &control);
            assert_eq!(
                machine(&subject.backend).peek(0x1800),
                mask.map(|code| if code.ends_with("ABCD") { 0xab } else { 0x12 })
                    .or(Some(0))
            );
            let restored = snapshot(&mut subject);
            assert!(restored.enabled && restored.entries.is_empty());
            assert!(restored.stream > before.stream);
            assert!(
                restored
                    .sound
                    .unwrap()
                    .gap
                    .unwrap()
                    .contains("not restored")
            );
            subject.call();
            control.call();
            trace_stream::compare_machine(&subject, &control);
            assert!(!snapshot(&mut subject).entries.is_empty());
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_policy_load_reset_and_metadata_routes_preserve_only_authorized_history() {
    let mut unsupported = Owner::new(fixture(
        b'U',
        TimingModel::Precise,
        ControllerKind::ThreeButton,
        &[],
    ));
    enable(&mut unsupported, true);
    let refused = snapshot(&mut unsupported);
    assert!(!refused.enabled && refused.entries.is_empty());
    assert!(
        refused
            .sound
            .unwrap()
            .rejection
            .unwrap()
            .contains("Standard")
    );
    let mut subject = owner(b'U');
    enable(&mut subject, true);
    subject.call();
    let before = snapshot(&mut subject);
    subject.backend.set_input(1, 0);
    subject.backend.set_apu_sample_generation_enabled(true);
    subject.backend.set_apu_channel_mutes(&[false; 10]);
    let mut samples = Vec::new();
    subject.backend.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().any(|sample| *sample != 0.0));
    subject.backend.set_sample_rate(48_000);
    unchanged(&mut subject, &before);
    assert!(subject.backend.load_state_from_bytes(vec![1]).is_err());
    assert!(subject.backend.restore_rewind_checkpoint(vec![1]).is_err());
    unchanged(&mut subject, &before);
    let state = subject.backend.encode_state_bytes().unwrap();
    enable(&mut subject, false);
    let disabled = snapshot(&mut subject);
    assert!(!disabled.enabled && !disabled.entries.is_empty());
    assert!(disabled.stream > before.stream);
    subject.call();
    unchanged(&mut subject, &disabled);
    enable(&mut subject, true);
    let resumed = snapshot(&mut subject);
    assert!(resumed.entries.is_empty() && resumed.stream > disabled.stream);
    subject.call();
    let called = snapshot(&mut subject);
    assert!(!called.entries.is_empty());
    let mut actions = DebugUiActions::none();
    actions.sound_trace.clear = true;
    configure(&mut subject.backend, &actions);
    assert!(snapshot(&mut subject).entries.is_empty());
    subject.call();
    let cleared = snapshot(&mut subject);
    assert!(cleared.stream > called.stream && !cleared.entries.is_empty());
    subject.backend.load_state_from_bytes(state).unwrap();
    let loaded = snapshot(&mut subject);
    assert!(loaded.enabled && loaded.entries.is_empty() && loaded.stream > cleared.stream);
    subject.backend.reset();
    assert!(snapshot(&mut subject).stream > loaded.stream);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_timed_ports_fault_discards_real_candidate_progress_and_keeps_previous_undo() {
    use zeff_emu_common::time::MachineTiming;
    use zeff_md_core::emulator::DebugPortWrite;
    let mut subject = owner(b'U');
    enable(&mut subject, true);
    let token = subject.call();
    let mut ready = false;
    for _ in 0..128 {
        let core = machine(&subject.backend);
        if core.sound_deadline().is_some_and(|deadline| {
            deadline
                .get()
                .saturating_sub(core.timing_snapshot().now().get())
                <= 28
        }) {
            ready = true;
            break;
        }
        let actions = DebugUiActions::none();
        let mut config = BackendRuntimeConfig::new(&actions);
        config.opcode_log_enabled = true;
        config.debug_step = true;
        subject.backend.apply_runtime_config(config);
    }
    assert!(ready);
    let first = DebugPortWrite::Byte {
        address: 0xa11100,
        value: 1,
    };
    let mut prefix = machine(&subject.backend).clone();
    prefix.debug_write_ports(&[first]).unwrap();
    assert!(prefix.sound_cpu().cycles() > machine(&subject.backend).sound_cpu().cycles());
    let before = machine(&subject.backend).clone();
    let rows = snapshot(&mut subject);
    let failure = subject
        .backend
        .execute_md_timed_ports(&[
            first,
            DebugPortWrite::Word {
                address: 0xc00004,
                value: 0x8c80,
            },
        ])
        .unwrap_err();
    assert_eq!(
        failure.kind,
        crate::emu_thread::MdTimedPortFailureKind::Execution
    );
    assert_eq!(failure.index, Some(1));
    assert_eq!(machine(&subject.backend), &before);
    unchanged(&mut subject, &rows);
    subject.undo(token);
    assert!(
        snapshot(&mut subject)
            .sound
            .unwrap()
            .gap
            .unwrap()
            .contains("not restored")
    );
}
