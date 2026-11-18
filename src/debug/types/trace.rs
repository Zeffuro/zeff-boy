use std::collections::VecDeque;

use zeff_emu_common::debug::{InstructionTraceRecord, sequence_is_after};

use crate::ui::{InstructionTraceBatch, TraceCursor, TraceStreamId, TraceUpdate};

pub(crate) struct TraceViewerState {
    pub(crate) entries: VecDeque<InstructionTraceRecord>,
    pub(crate) last_sequence: Option<u64>,
    stream: Option<TraceStreamId>,
    pub(crate) target: crate::ui::TraceTarget,
    pub(crate) sound: Option<crate::ui::SoundTraceInfo>,
    pub(crate) enabled: bool,
    pub(crate) capacity: usize,
    pub(crate) retained: usize,
    pub(crate) missed: u64,
    pub(crate) filter: String,
    pub(crate) auto_scroll: bool,
    pub(crate) filtered_indices: Vec<usize>,
    pub(crate) cached_filter: String,
    pub(crate) cached_sequence: Option<u64>,
}

impl TraceViewerState {
    pub(crate) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            last_sequence: None,
            stream: None,
            target: crate::ui::TraceTarget::Primary,
            sound: None,
            enabled: false,
            capacity: zeff_emu_common::debug::MIN_TRACE_CAPACITY,
            retained: 0,
            missed: 0,
            filter: String::new(),
            auto_scroll: true,
            filtered_indices: Vec::new(),
            cached_filter: String::new(),
            cached_sequence: None,
        }
    }

    pub(crate) fn merge(&mut self, batch: InstructionTraceBatch) {
        if batch.target != self.target {
            return;
        }
        match self.cursor().update(batch.stream, batch.newest_sequence) {
            TraceUpdate::Ignore => return,
            TraceUpdate::Replace => {
                self.clear();
                self.stream = batch.stream;
            }
            TraceUpdate::Same => {}
        }
        self.sound = batch.sound;
        self.enabled = batch.enabled;
        self.capacity = batch.capacity;
        self.retained = batch.retained;

        if batch.retained == 0 && batch.newest_sequence.is_none() {
            self.clear();
            self.enabled = batch.enabled;
            self.capacity = batch.capacity;
            return;
        }

        if let (Some(last), Some(oldest)) = (self.last_sequence, batch.oldest_sequence)
            && sequence_is_after(oldest, last)
        {
            self.missed = self
                .missed
                .saturating_add(oldest.wrapping_sub(last).saturating_sub(1));
        }

        for entry in batch.entries {
            if self
                .last_sequence
                .is_some_and(|sequence| !sequence_is_after(entry.sequence, sequence))
            {
                continue;
            }
            self.last_sequence = Some(entry.sequence);
            self.entries.push_back(entry);
        }

        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub(crate) fn cursor(&self) -> TraceCursor {
        TraceCursor {
            target: self.target,
            stream: self.stream,
            after_sequence: self.last_sequence,
        }
    }

    pub(crate) fn select_target(&mut self, target: crate::ui::TraceTarget) {
        if self.target == target {
            return;
        }
        self.target = target;
        self.clear();
        self.stream = None;
        self.enabled = false;
    }

    pub(crate) fn reset_backend(&mut self) {
        self.target = crate::ui::TraceTarget::Primary;
        self.sound = None;
        self.clear();
        self.stream = None;
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.last_sequence = None;
        self.retained = 0;
        self.missed = 0;
        self.filtered_indices.clear();
        self.cached_filter.clear();
        self.cached_sequence = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(sequences: &[u64], retained: usize) -> InstructionTraceBatch {
        let entries = sequences
            .iter()
            .map(|&sequence| InstructionTraceRecord {
                sequence,
                ..Default::default()
            })
            .collect();
        InstructionTraceBatch {
            target: crate::ui::TraceTarget::Primary,
            sound: None,
            stream: None,
            enabled: true,
            capacity: 1_000,
            retained,
            oldest_sequence: sequences.first().copied(),
            newest_sequence: sequences.last().copied(),
            entries,
        }
    }

    #[test]
    fn merge_is_incremental_and_reports_gaps() {
        let mut state = TraceViewerState::new();
        state.merge(batch(&[3, 4], 2));
        state.merge(batch(&[8], 1));

        assert_eq!(state.entries.len(), 3);
        assert_eq!(state.last_sequence, Some(8));
        assert_eq!(state.missed, 3);
    }

    #[test]
    fn empty_store_clears_stale_rows() {
        let mut state = TraceViewerState::new();
        state.merge(batch(&[1], 1));
        state.merge(batch(&[], 0));

        assert!(state.entries.is_empty());
        assert_eq!(state.last_sequence, None);
    }
}
