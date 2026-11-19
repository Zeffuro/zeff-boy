use super::*;
use crate::debug::types::{ExecutionCoverage, TraceViewerState};
use crate::symbols::SymbolSession;
use crate::ui::{InstructionTraceBatch, TraceCursor};

fn snapshot(thread: &EmuThread, cursor: TraceCursor) -> InstructionTraceBatch {
    let mut input = guest_call_input(0);
    input.snapshot.trace_cursor = cursor;
    guest_call_deliver(thread, input)
        .ui_data
        .instruction_trace
        .unwrap()
}

fn call(thread: &EmuThread, target: u32, count: u64) -> Vec<u8> {
    thread.send(EmuCommand::ExecuteGuestCall(GuestCallRequest {
        name: "stream".into(),
        target,
        storage_offset: Some(u64::from(target)),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: count,
    }));
    let EmuResponse::GuestCallCompleted {
        instructions,
        undo_state,
        ..
    } = receive_response(thread)
    else {
        panic!("guest call failed")
    };
    assert_eq!(instructions, count);
    undo_state
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_trace_stream_real_worker_undo_replay_replaces_equal_lower_and_higher_histories() {
    for count in [8, 4, 10] {
        let mut words = vec![0x705a];
        words.extend(std::iter::repeat_n(0x4e71, count - 2));
        words.push(0x4e75);
        let backend = guest_call_backend_with(
            b'U',
            zeff_md_core::TimingModel::Standard,
            zeff_md_core::ControllerKind::ThreeButton,
            &[(0x1800, &words)],
            false,
        );
        let mut thread = EmuThread::spawn(backend, false);
        assert_eq!(step(&thread, 2, 0, 0).advanced_frames, 2);
        let mut stop = guest_call_input(1);
        stop.debug_actions.add_breakpoint = Some(RETURN_PC);
        stop.debug_actions.trace_enabled = Some(true);
        stop.debug_actions.trace_capacity = Some(64);
        paused(&guest_call_deliver(&thread, stop));
        let mut step_one = guest_call_input(1);
        step_one.debug_step = true;
        paused(&guest_call_deliver(&thread, step_one));
        let before = capture(&thread);
        let token = call(&thread, 0x1000, 8);
        let old = snapshot(&thread, TraceCursor::default());
        let delayed = snapshot(&thread, TraceCursor::default());
        assert_eq!(old.entries.len(), 9);
        let old_cursor = TraceCursor {
            target: crate::ui::TraceTarget::Primary,
            stream: old.stream,
            after_sequence: old.newest_sequence,
        };
        let mut viewer = TraceViewerState::new();
        let mut coverage = ExecutionCoverage::default();
        coverage.merge(&old, &SymbolSession::default());
        viewer.merge(old);
        thread.send(EmuCommand::UndoGuestCall(token));
        assert!(matches!(
            receive_response(&thread),
            EmuResponse::GuestCallUndone
        ));
        assert_eq!(capture(&thread), before);
        call(&thread, 0x1800, count as u64);
        let replay = snapshot(&thread, old_cursor);
        assert!(replay.stream > old_cursor.stream);
        assert_eq!(replay.entries.len(), count + 1);
        assert_eq!(
            replay
                .newest_sequence
                .unwrap()
                .cmp(&old_cursor.after_sequence.unwrap()),
            count.cmp(&8)
        );
        let expected = replay.entries.clone();
        coverage.merge(&replay, &SymbolSession::default());
        viewer.merge(replay);
        assert_eq!(viewer.entries.iter().copied().collect::<Vec<_>>(), expected);
        assert_eq!(coverage.total(), (count + 1) as u64);
        assert!(
            viewer
                .entries
                .iter()
                .all(|row| !(0x1000..0x1200).contains(&row.pc))
        );
        let revision = coverage.revision();
        coverage.merge(&delayed, &SymbolSession::default());
        viewer.merge(delayed);
        assert_eq!(viewer.entries.iter().copied().collect::<Vec<_>>(), expected);
        assert_eq!(coverage.revision(), revision);
        thread.shutdown();
    }
}
