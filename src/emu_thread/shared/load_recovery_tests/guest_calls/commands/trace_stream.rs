use super::*;
use crate::debug::types::{ExecutionCoverage, TraceViewerState};
use crate::symbols::SymbolSession;
use crate::ui::{InstructionTraceBatch, TraceCursor, TraceStreamId};

fn stream(owner: &Owner) -> TraceStreamId {
    let EmuBackend::Md(md) = &owner.backend else {
        panic!("expected MD")
    };
    md.instruction_trace_stream()
}

fn snapshot(owner: &mut Owner, cursor: TraceCursor) -> InstructionTraceBatch {
    let mut input = guest_call_input(0);
    input.snapshot.trace_cursor = cursor;
    EmuThread::collect_ui_snapshot(&mut owner.backend, &input.snapshot, input.buffers)
        .instruction_trace
        .unwrap()
}

pub(super) fn compare_machine(actual: &Owner, expected: &Owner) {
    assert_eq!(machine(&actual.backend), machine(&expected.backend));
    assert_eq!(frontend(&actual.backend), frontend(&expected.backend));
    assert_eq!(
        actual.backend.encode_state_bytes().unwrap(),
        expected.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(
        machine(&actual.backend)
            .encode_debugger_checkpoint()
            .unwrap(),
        machine(&expected.backend)
            .encode_debugger_checkpoint()
            .unwrap()
    );
    let mut left = Vec::new();
    let mut right = Vec::new();
    machine(&actual.backend)
        .clone()
        .drain_audio_samples_into(&mut left);
    machine(&expected.backend)
        .clone()
        .drain_audio_samples_into(&mut right);
    assert_eq!(
        left.iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        right
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_trace_stream_failed_commands_preserve_publication_previous_undo_and_machine() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for mask in [None, Some("001800:1234"), Some("001800:ABCD")] {
            let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::SixButton, &[]));
            let mut control = Owner::new(fixture(b'U', timing, ControllerKind::SixButton, &[]));
            let mut disabled = DebugUiActions::none();
            disabled.trace_enabled = Some(false);
            configure(&mut control.backend, &disabled);
            let original_mask =
                crate::cheats::parse_cheat_for_system("001800:1234", ActiveSystem::Md)
                    .unwrap()
                    .0;
            machine_mut(&mut owner.backend)
                .install_rom_patches(&original_mask)
                .unwrap();
            machine_mut(&mut control.backend)
                .install_rom_patches(&original_mask)
                .unwrap();
            let initial = frontend(&owner.backend);
            let first_stream = stream(&owner);
            let token = owner.call();
            let control_token = control.call();
            assert_eq!(stream(&owner), first_stream);
            if let Some(code) = mask {
                let patches = crate::cheats::parse_cheat_for_system(code, ActiveSystem::Md)
                    .unwrap()
                    .0;
                machine_mut(&mut owner.backend)
                    .install_rom_patches(&patches)
                    .unwrap();
                machine_mut(&mut control.backend)
                    .install_rom_patches(&patches)
                    .unwrap();
            } else {
                machine_mut(&mut owner.backend)
                    .install_rom_patches(&[])
                    .unwrap();
                machine_mut(&mut control.backend)
                    .install_rom_patches(&[])
                    .unwrap();
            }
            compare_machine(&owner, &control);
            let batch = snapshot(&mut owner, TraceCursor::default());
            let cursor = TraceCursor {
                target: crate::ui::TraceTarget::Primary,
                stream: batch.stream,
                after_sequence: batch.newest_sequence,
            };
            let mut viewer = TraceViewerState::new();
            let mut coverage = ExecutionCoverage::default();
            coverage.merge(&batch, &SymbolSession::default());
            viewer.merge(batch);
            let rows = viewer.entries.clone();
            let revision = coverage.revision();
            let before = frontend(&owner.backend);
            for (target, budget, watch) in
                [(0x1200, 4, false), (0x1300, 8, false), (0x1000, 8, true)]
            {
                let mut actions = DebugUiActions::none();
                if watch {
                    actions.add_watchpoint = Some((0xff0040, 0xff0040, WatchType::Write));
                }
                configure(&mut owner.backend, &actions);
                configure(&mut control.backend, &actions);
                for subject in [&mut owner, &mut control] {
                    let effect =
                        subject.dispatch(EmuCommand::ExecuteGuestCall(request(target, budget)));
                    assert!(!effect.potentially_dirty);
                    assert!(matches!(
                        effect.response,
                        Some(EmuResponse::GuestCallFailed { .. })
                    ));
                }
                assert_eq!(frontend(&owner.backend), before);
                assert_eq!(stream(&owner), first_stream);
                compare_machine(&owner, &control);
                let batch = snapshot(&mut owner, cursor);
                assert!(batch.entries.is_empty());
                coverage.merge(&batch, &SymbolSession::default());
                viewer.merge(batch);
                assert_eq!(viewer.entries, rows);
                assert_eq!(coverage.revision(), revision);
            }
            owner.refused_undo(vec![0; 16]);
            assert_eq!(stream(&owner), first_stream);
            owner.undo(token);
            control.undo(control_token);
            assert!(stream(&owner) > first_stream);
            assert_eq!(frontend(&owner.backend), initial);
            compare_machine(&owner, &control);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_trace_stream_clear_reexecute_and_load_reset_fences_are_acceptance_bound() {
    let mut owner = Owner::new(guest_call_backend());
    owner.call();
    let old = snapshot(&mut owner, TraceCursor::default());
    let cursor = TraceCursor {
        target: crate::ui::TraceTarget::Primary,
        stream: old.stream,
        after_sequence: old.newest_sequence,
    };
    let mut viewer = TraceViewerState::new();
    let mut coverage = ExecutionCoverage::default();
    coverage.merge(&old, &SymbolSession::default());
    viewer.merge(old);
    let mut actions = DebugUiActions::none();
    actions.trace_clear = true;
    configure(&mut owner.backend, &actions);
    owner.call();
    let replay = snapshot(&mut owner, cursor);
    assert!(replay.stream > cursor.stream);
    assert_eq!(replay.entries.len(), 8);
    coverage.merge(&replay, &SymbolSession::default());
    viewer.merge(replay);
    assert_eq!(viewer.entries.len(), 8);
    assert_eq!(coverage.total(), 8);
    assert_eq!(viewer.missed, 0);
    let accepted = stream(&owner);
    let mut policy = DebugUiActions::none();
    policy.trace_enabled = Some(false);
    policy.trace_capacity = Some(128);
    configure(&mut owner.backend, &policy);
    policy.trace_enabled = Some(true);
    configure(&mut owner.backend, &policy);
    assert_eq!(stream(&owner), accepted);
    let state = owner.backend.encode_state_bytes().unwrap();
    assert_eq!(stream(&owner), accepted);
    assert!(owner.backend.load_state_from_bytes(vec![1, 2, 3]).is_err());
    assert_eq!(stream(&owner), accepted);
    assert!(
        owner
            .backend
            .restore_rewind_checkpoint(vec![1, 2, 3])
            .is_err()
    );
    assert_eq!(stream(&owner), accepted);
    owner.backend.load_state_from_bytes(state).unwrap();
    assert!(stream(&owner) > accepted);
    let loaded = stream(&owner);
    let checkpoint = machine(&owner.backend)
        .encode_debugger_checkpoint()
        .unwrap();
    owner.backend.restore_rewind_checkpoint(checkpoint).unwrap();
    assert!(stream(&owner) > loaded);
    let rewind = stream(&owner);
    owner.backend.reset();
    assert!(stream(&owner) > rewind);
    let reset = stream(&owner);
    let EmuBackend::Md(md) = &mut owner.backend else {
        panic!("expected MD")
    };
    let patches = crate::cheats::parse_cheat_for_system("001800:1234", ActiveSystem::Md)
        .unwrap()
        .0;
    md.test_normal_core_mut()
        .unwrap()
        .install_rom_patches(&patches)
        .unwrap();
    let before = md.core().clone();
    assert!(md.reset_and_begin_audio_trace(8).is_err());
    assert_eq!(md.instruction_trace_stream(), reset);
    assert_eq!(md.core(), &before);
    md.test_normal_core_mut()
        .unwrap()
        .install_rom_patches(&[])
        .unwrap();
    md.reset_and_begin_audio_trace(8).unwrap();
    assert!(md.instruction_trace_stream() > reset);
    assert!(md.instruction_trace().is_empty());
    let audio_reset = md.instruction_trace_stream();
    md.finish_audio_trace().unwrap();
    assert_eq!(md.instruction_trace_stream(), audio_reset);
    md.try_warm_reset().unwrap();
    assert!(md.instruction_trace_stream() > audio_reset);
    let previous = viewer.cursor();
    let mut replacement = Owner::new(standard());
    replacement.call();
    let replacement = snapshot(&mut replacement, previous);
    assert_eq!(replacement.entries.len(), 8);
    assert!(replacement.stream > previous.stream);
    coverage.merge(&replacement, &SymbolSession::default());
    viewer.merge(replacement);
    assert_eq!(viewer.entries.len(), 8);
    assert_eq!(coverage.total(), 8);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_trace_stream_exhaustion_disables_capture_without_blocking_execution_or_undo() {
    let mut owner = Owner::new(standard());
    let mut control = Owner::new(standard());
    owner.call();
    let original = frontend(&owner.backend);
    let EmuBackend::Md(md) = &mut owner.backend else {
        panic!("expected MD")
    };
    md.exhaust_trace_stream_for_test();
    let terminal = md.instruction_trace_stream();
    assert!(!terminal.is_live());
    assert!(!md.instruction_trace().is_enabled());
    assert!(md.instruction_trace().is_empty());
    assert_eq!(frontend(&owner.backend), original);
    control.call();
    let mut actions = DebugUiActions::none();
    actions.trace_enabled = Some(true);
    configure(&mut owner.backend, &actions);
    let token = owner.call();
    let control_token = control.call();
    compare_machine(&owner, &control);
    owner.undo(token);
    control.undo(control_token);
    compare_machine(&owner, &control);
    let batch = snapshot(&mut owner, TraceCursor::default());
    assert_eq!(batch.stream, Some(terminal));
    assert!(!batch.enabled && batch.entries.is_empty());
}
