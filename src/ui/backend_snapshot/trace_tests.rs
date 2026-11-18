use super::collect_instruction_trace;
use crate::debug::types::TraceViewerState;
use zeff_emu_common::debug::{
    InstructionTraceRecord, InstructionTraceStore, MIN_TRACE_CAPACITY, TraceBoundary,
    TraceBoundaryKind, TraceExecMode, TraceObservation, TraceObservationEnd, TraceObservationStart,
    TraceWrite, TraceWriteKind, TraceWriteWidth,
};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_frame_unknown_zero_survives_real_store_batch_viewer_and_keeps_exact_zero_visible() {
    use zeff_emu_common::debug::TraceFrameKnowledge;
    let mut store = InstructionTraceStore::default();
    store.set_enabled(true);
    let mut unknown = InstructionTraceRecord::new(TraceExecMode::Z80, 0, None, 0, 7, &[0]);
    unknown.observation.frame_knowledge = TraceFrameKnowledge::Unknown;
    store.push(unknown);
    let exact =
        InstructionTraceRecord::new(TraceExecMode::M68000, 0x200, None, 0, 24, &[0x4e, 0x71]);
    assert_eq!(
        exact.observation.frame_knowledge,
        TraceFrameKnowledge::Exact
    );
    store.push(exact);
    let cloned = store.clone();
    let batch = collect_instruction_trace(&cloned, None);
    assert_eq!(batch.entries, cloned.entries_after(None, 2));
    let mut viewer = TraceViewerState::new();
    viewer.merge(batch);
    let entries = viewer.entries.iter().collect::<Vec<_>>();
    assert_eq!(
        entries.iter().map(|entry| entry.frame).collect::<Vec<_>>(),
        [0, 0]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| crate::debug::trace_frame_label(entry))
            .collect::<Vec<_>>(),
        ["Unknown", "0"]
    );
    assert_eq!(
        entries.iter().map(|entry| entry.cycle).collect::<Vec<_>>(),
        [7, 24]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_observation_survives_store_clone_cursor_batch_and_viewer_transport() {
    let mut store = InstructionTraceStore::new(MIN_TRACE_CAPACITY);
    store.set_enabled(true);
    for _ in 0..MIN_TRACE_CAPACITY {
        store.push(InstructionTraceRecord::default());
    }
    let mut expected = Vec::new();
    for start in [
        TraceObservationStart::Instruction,
        TraceObservationStart::Suffix,
    ] {
        for end in [
            TraceObservationEnd::Retired,
            TraceObservationEnd::Fault,
            TraceObservationEnd::Reset,
            TraceObservationEnd::Disabled,
        ] {
            let mut entry =
                InstructionTraceRecord::new(TraceExecMode::Z80, 0xffff, None, 3, 41, &[0x3e, 0x5a]);
            entry.observation = TraceObservation {
                start,
                end,
                ..Default::default()
            };
            if end == TraceObservationEnd::Reset {
                entry.set_instruction(&[0xdd; 17]);
            }
            entry.boundary = Some(TraceBoundary {
                kind: TraceBoundaryKind::Instruction,
                cpu_clocks: 7,
                final_exception: None,
                machine_fault: false,
            });
            entry.push_write(TraceWrite {
                address: 0x100,
                old_value: None,
                new_value: 0x5a,
                width: TraceWriteWidth::Byte,
                kind: TraceWriteKind::Memory,
            });
            entry.sequence = store.push(entry).unwrap();
            expected.push(entry);
        }
    }
    let cursor = Some(MIN_TRACE_CAPACITY as u64 - 1);
    let cloned = store.clone();
    let read = cloned.entries_after(cursor, 64);
    assert_eq!(read, expected);
    let batch = collect_instruction_trace(&cloned, cursor);
    assert_eq!(batch.entries, expected);
    assert_eq!(batch.oldest_sequence, Some(8));
    assert_eq!(batch.newest_sequence, Some(MIN_TRACE_CAPACITY as u64 + 7));
    let mut viewer = TraceViewerState::new();
    viewer.merge(batch);
    let copied: Vec<_> = viewer.entries.iter().copied().collect();
    assert_eq!(copied, expected);
    assert_eq!(
        viewer.entries.clone().into_iter().collect::<Vec<_>>(),
        expected
    );
    viewer.merge(collect_instruction_trace(&store, viewer.last_sequence));
    assert_eq!(viewer.entries.into_iter().collect::<Vec<_>>(), expected);
}
