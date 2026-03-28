use std::collections::VecDeque;

struct RewindSnapshot {
    compressed: Vec<u8>,
    state_len: u32,
}

impl RewindSnapshot {
    fn compress(state_bytes: &[u8], framebuffer: &[u8], scratch: &mut Vec<u8>) -> Self {
        scratch.clear();
        scratch.reserve(state_bytes.len() + framebuffer.len());
        scratch.extend_from_slice(state_bytes);
        scratch.extend_from_slice(framebuffer);
        Self {
            compressed: lz4_flex::compress_prepend_size(scratch),
            state_len: state_bytes.len() as u32,
        }
    }

    fn decompress(&self) -> Option<RewindFrame> {
        let mut combined = lz4_flex::decompress_size_prepended(&self.compressed).ok()?;
        let split = self.state_len as usize;
        if split > combined.len() {
            return None;
        }
        let framebuffer = combined.split_off(split);
        Some(RewindFrame {
            state_bytes: combined,
            framebuffer,
        })
    }
}

pub struct RewindFrame {
    pub state_bytes: Vec<u8>,
    pub framebuffer: Vec<u8>,
}

pub struct RewindBuffer {
    snapshots: VecDeque<RewindSnapshot>,
    capacity: usize,
    capture_interval: usize,
    frame_counter: usize,
    scratch: Vec<u8>,
}

impl RewindBuffer {
    pub fn new(seconds: usize, capture_interval: usize) -> Self {
        let capacity = (seconds * 60) / capture_interval.max(1);
        Self {
            snapshots: VecDeque::with_capacity(capacity),
            capacity,
            capture_interval: capture_interval.max(1),
            frame_counter: 0,
            scratch: Vec::new(),
        }
    }

    pub fn tick(&mut self) -> bool {
        self.frame_counter += 1;
        if self.frame_counter >= self.capture_interval {
            self.frame_counter = 0;
            true
        } else {
            false
        }
    }

    pub fn push(&mut self, state_bytes: &[u8], framebuffer: &[u8]) {
        if self.snapshots.len() >= self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots
            .push_back(RewindSnapshot::compress(state_bytes, framebuffer, &mut self.scratch));
    }

    pub fn pop(&mut self) -> Option<RewindFrame> {
        self.snapshots.pop_back().and_then(|s| s.decompress())
    }

    pub fn peek(&self) -> Option<RewindFrame> {
        self.snapshots.back().and_then(|s| s.decompress())
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn fill_ratio(&self) -> f32 {
        if self.capacity == 0 {
            return 0.0;
        }
        self.snapshots.len() as f32 / self.capacity as f32
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.frame_counter = 0;
    }
}

#[cfg(test)]
mod tests;
