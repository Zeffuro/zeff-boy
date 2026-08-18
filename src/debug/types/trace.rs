use std::collections::VecDeque;

use zeff_emu_common::debug::InstructionTraceRecord;

use crate::ui::InstructionTraceBatch;

pub(crate) struct TraceViewerState {
    pub(crate) entries: VecDeque<InstructionTraceRecord>,
    pub(crate) last_sequence: Option<u64>,
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
        self.enabled = batch.enabled;
        self.capacity = batch.capacity;
        self.retained = batch.retained;

        if batch.retained == 0 && batch.newest_sequence.is_none() {
            self.clear();
            self.enabled = batch.enabled;
            self.capacity = batch.capacity;
            return;
        }
        if let (Some(last), Some(newest)) = (self.last_sequence, batch.newest_sequence)
            && newest < last
        {
            self.clear();
        }

        if let (Some(last), Some(oldest)) = (self.last_sequence, batch.oldest_sequence)
            && oldest > last.wrapping_add(1)
        {
            self.missed = self.missed.saturating_add(oldest - last - 1);
        }

        for entry in batch.entries {
            if self
                .last_sequence
                .is_some_and(|sequence| entry.sequence <= sequence)
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
