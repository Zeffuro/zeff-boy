use super::*;
use crate::debug::types::{ExecutionCoverage, TraceViewerState};
use crate::symbols::SymbolSession;
use crate::ui::{InstructionTraceBatch, TraceCursor, TraceStreamId};
use zeff_emu_common::debug::{InstructionTraceRecord, InstructionTraceStore, TraceExecMode};

fn push(store: &mut InstructionTraceStore, pc: u32, count: usize) {
    for _ in 0..count {
        store.push(InstructionTraceRecord::new(
            TraceExecMode::M68000,
            pc,
            None,
            0,
            0,
            &[0x4e, 0x71],
        ));
    }
}

fn merge(
    viewer: &mut TraceViewerState,
    coverage: &mut ExecutionCoverage,
    batch: InstructionTraceBatch,
) {
    coverage.merge(&batch, &SymbolSession::default());
    viewer.merge(batch);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_stream_restored_store_and_clear_replace_equal_lower_and_higher_cursors() {
    for count in [2, 4, 6] {
        let mut store = InstructionTraceStore::default();
        store.set_enabled(true);
        push(&mut store, 0x200, 1);
        let checkpoint = store.clone();
        let old_stream = Some(TraceStreamId::allocate());
        push(&mut store, 0x1000, 4);
        let old = collect_instruction_trace_stream(&store, old_stream, TraceCursor::default());
        let delayed = collect_instruction_trace_stream(&store, old_stream, TraceCursor::default());
        let old_cursor = TraceCursor {
            target: crate::ui::TraceTarget::Primary,
            stream: old.stream,
            after_sequence: old.newest_sequence,
        };
        let mut viewer = TraceViewerState::new();
        let mut coverage = ExecutionCoverage::default();
        merge(&mut viewer, &mut coverage, old);
        viewer.filtered_indices.push(999);
        viewer.cached_sequence = old_cursor.after_sequence;
        viewer.cached_filter = "old".into();
        store = checkpoint;
        push(&mut store, 0x1800, count);
        let new_stream = Some(TraceStreamId::allocate());
        let replay = collect_instruction_trace_stream(&store, new_stream, old_cursor);
        assert_eq!(replay.entries.len(), count + 1);
        merge(&mut viewer, &mut coverage, replay);
        assert_eq!(coverage.total(), (count + 1) as u64);
        assert_eq!(viewer.entries.len(), count + 1);
        assert!(viewer.entries.iter().all(|row| row.pc != 0x1000));
        assert!(viewer.filtered_indices.is_empty() && viewer.cached_filter.is_empty());
        assert_eq!(viewer.cached_sequence, None);
        assert_eq!(viewer.missed, 0);
        let revision = coverage.revision();
        merge(&mut viewer, &mut coverage, delayed);
        assert_eq!(viewer.cursor().stream, new_stream);
        assert_eq!(coverage.revision(), revision);
        let cursor = viewer.cursor();
        let next = store.newest_sequence().unwrap().wrapping_add(1);
        store.clear();
        push(&mut store, 0x1900, 1);
        assert_eq!(store.newest_sequence(), Some(next));
        let cleared_stream = Some(TraceStreamId::allocate());
        merge(
            &mut viewer,
            &mut coverage,
            collect_instruction_trace_stream(&store, cleared_stream, cursor),
        );
        assert_eq!(viewer.entries.len(), 1);
        assert_eq!(viewer.entries[0].pc, 0x1900);
        assert_eq!(coverage.total(), 1);
        assert_eq!(viewer.missed, 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_stream_mismatch_returns_whole_retained_ring_and_matching_cursor_is_bounded() {
    let mut store = InstructionTraceStore::default();
    store.set_capacity(4096);
    store.set_enabled(true);
    push(&mut store, 0x200, 5000);
    let stream = Some(TraceStreamId::allocate());
    let first = collect_instruction_trace_stream(&store, stream, TraceCursor::default());
    assert_eq!(first.entries.len(), store.len());
    assert!(first.entries.len() > 2048);
    assert_eq!(first.entries, store.iter().copied().collect::<Vec<_>>());
    let cursor = TraceCursor {
        target: crate::ui::TraceTarget::Primary,
        stream,
        after_sequence: Some(first.entries[0].sequence),
    };
    let incremental = collect_instruction_trace_stream(&store, stream, cursor);
    assert_eq!(incremental.entries.len(), 2048);
    assert_eq!(incremental.entries[0], first.entries[1]);
    let duplicate = collect_instruction_trace_stream(
        &store,
        stream,
        TraceCursor {
            target: crate::ui::TraceTarget::Primary,
            stream,
            after_sequence: store.newest_sequence(),
        },
    );
    assert!(duplicate.entries.is_empty());
    let legacy = collect_instruction_trace(&store, None);
    assert_eq!(legacy.stream, None);
    assert_eq!(legacy.entries.len(), 2048);
    let switched = collect_instruction_trace_stream(&store, None, cursor);
    assert_eq!(switched.entries.len(), store.len());
}

fn batch(stream: Option<TraceStreamId>, sequences: &[u64]) -> InstructionTraceBatch {
    InstructionTraceBatch {
        target: crate::ui::TraceTarget::Primary,
        sound: None,
        stream,
        enabled: true,
        capacity: 64,
        retained: sequences.len(),
        oldest_sequence: sequences.first().copied(),
        newest_sequence: sequences.last().copied(),
        entries: sequences
            .iter()
            .map(|&sequence| InstructionTraceRecord {
                sequence,
                pc: sequence as u32,
                ..Default::default()
            })
            .collect(),
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_stream_consumers_keep_modular_wrap_duplicates_gaps_and_empty_fences() {
    for stream in [None, Some(TraceStreamId::allocate())] {
        let mut viewer = TraceViewerState::new();
        let mut coverage = ExecutionCoverage::default();
        merge(
            &mut viewer,
            &mut coverage,
            batch(stream, &[u64::MAX - 1, u64::MAX]),
        );
        merge(&mut viewer, &mut coverage, batch(stream, &[u64::MAX, 0, 1]));
        merge(&mut viewer, &mut coverage, batch(stream, &[0, 1]));
        assert_eq!(
            viewer
                .entries
                .iter()
                .map(|row| row.sequence)
                .collect::<Vec<_>>(),
            [u64::MAX - 1, u64::MAX, 0, 1]
        );
        assert_eq!(viewer.missed, 0);
        assert_eq!(coverage.total(), 4);
        if stream.is_some() {
            let revision = coverage.revision();
            let mut stale = batch(stream, &[u64::MAX]);
            stale.capacity = 500;
            merge(&mut viewer, &mut coverage, stale);
            merge(&mut viewer, &mut coverage, batch(stream, &[]));
            assert_eq!(viewer.last_sequence, Some(1));
            assert_eq!(viewer.capacity, 64);
            assert_eq!(coverage.revision(), revision);
        }
        for (oldest, missed) in [(u64::MAX, 0), (0, 1)] {
            viewer.reset_backend();
            coverage.reset_backend();
            merge(&mut viewer, &mut coverage, batch(stream, &[u64::MAX - 1]));
            merge(&mut viewer, &mut coverage, batch(stream, &[oldest]));
            assert_eq!(viewer.missed, missed);
        }
        let fresh = Some(TraceStreamId::allocate());
        merge(&mut viewer, &mut coverage, batch(fresh, &[]));
        assert!(viewer.entries.is_empty());
        assert_eq!(viewer.cursor().stream, fresh);
        assert_eq!(coverage.total(), 0);
        merge(&mut viewer, &mut coverage, batch(stream, &[50]));
        if stream.is_some() {
            assert!(viewer.entries.is_empty());
            assert_eq!(coverage.total(), 0);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_target_switch_rejects_foreign_batches_and_sound_never_changes_main_coverage() {
    let primary_stream = Some(TraceStreamId::allocate());
    let sound_stream = Some(TraceStreamId::allocate());
    let mut viewer = TraceViewerState::new();
    let mut coverage = ExecutionCoverage::default();
    merge(&mut viewer, &mut coverage, batch(primary_stream, &[1, 2]));
    let revision = coverage.revision();
    viewer.select_target(crate::ui::TraceTarget::Sound);
    assert!(viewer.entries.is_empty());
    let mut sound = batch(sound_stream, &[1, 2]);
    sound.target = crate::ui::TraceTarget::Sound;
    sound.sound = Some(crate::ui::SoundTraceInfo {
        rejection: None,
        gap: Some("unknown history"),
    });
    merge(&mut viewer, &mut coverage, sound);
    assert_eq!(coverage.total(), 2);
    assert_eq!(coverage.revision(), revision);
    assert_eq!(viewer.entries.len(), 2);
    merge(&mut viewer, &mut coverage, batch(primary_stream, &[3]));
    assert_eq!(viewer.entries.len(), 2);
    assert_eq!(viewer.cursor().stream, sound_stream);
    assert_eq!(viewer.sound.unwrap().gap, Some("unknown history"));
    viewer.select_target(crate::ui::TraceTarget::Primary);
    merge(
        &mut viewer,
        &mut coverage,
        batch(primary_stream, &[1, 2, 3]),
    );
    assert_eq!(viewer.entries.len(), 3);
    assert_eq!(viewer.cursor().stream, primary_stream);
    assert_eq!(coverage.total(), 3);
}
