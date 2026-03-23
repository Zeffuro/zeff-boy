use std::collections::VecDeque;

struct RewindSnapshot {
    compressed: Vec<u8>,
}

impl RewindSnapshot {
    fn compress(state_bytes: &[u8]) -> Self {
        Self {
            compressed: lz4_flex::compress_prepend_size(state_bytes),
        }
    }

    fn decompress(&self) -> Vec<u8> {
        lz4_flex::decompress_size_prepended(&self.compressed)
            .unwrap_or_default()
    }
}

pub(crate) struct RewindBuffer {
    snapshots: VecDeque<RewindSnapshot>,
    capacity: usize,
    capture_interval: usize,
    frame_counter: usize,
}

impl RewindBuffer {
    pub(crate) fn new(seconds: usize, capture_interval: usize) -> Self {
        let capacity = (seconds * 60) / capture_interval.max(1);
        Self {
            snapshots: VecDeque::with_capacity(capacity),
            capacity,
            capture_interval: capture_interval.max(1),
            frame_counter: 0,
        }
    }

    pub(crate) fn tick(&mut self) -> bool {
        self.frame_counter += 1;
        if self.frame_counter >= self.capture_interval {
            self.frame_counter = 0;
            true
        } else {
            false
        }
    }

    pub(crate) fn push(&mut self, state_bytes: &[u8]) {
        if self.snapshots.len() >= self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(RewindSnapshot::compress(state_bytes));
    }

    pub(crate) fn pop(&mut self) -> Option<Vec<u8>> {
        self.snapshots.pop_back().map(|s| s.decompress())
    }

    pub(crate) fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.snapshots.clear();
        self.frame_counter = 0;
    }
}

