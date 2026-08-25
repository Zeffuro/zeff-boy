use std::collections::VecDeque;

struct RewindSnapshot {
    compressed: Vec<u8>,
    state_len: u32,
    emulated_frame: u64,
    capture_phase: usize,
}

impl RewindSnapshot {
    fn compress(
        state_bytes: &[u8],
        framebuffer: &[u8],
        emulated_frame: u64,
        capture_phase: usize,
        scratch: &mut Vec<u8>,
    ) -> Self {
        scratch.clear();
        scratch.reserve(state_bytes.len() + framebuffer.len());
        scratch.extend_from_slice(state_bytes);
        scratch.extend_from_slice(framebuffer);
        Self {
            compressed: lz4_flex::compress_prepend_size(scratch),
            state_len: state_bytes.len() as u32,
            emulated_frame,
            capture_phase,
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
            rewound_frames: 0,
        })
    }
}

pub struct RewindFrame {
    pub state_bytes: Vec<u8>,
    pub framebuffer: Vec<u8>,
    pub rewound_frames: u64,
}

pub struct RewindBuffer {
    snapshots: VecDeque<RewindSnapshot>,
    capacity: usize,
    capture_interval: usize,
    frame_counter: usize,
    emulated_frame: u64,
    scratch: Vec<u8>,
}

impl RewindBuffer {
    pub fn new(seconds: usize, capture_interval: usize) -> Self {
        Self::new_with_frame_duration(seconds, capture_interval, 16_666_667)
    }

    pub fn new_with_frame_duration(
        seconds: usize,
        capture_interval: usize,
        frame_duration_ns: u64,
    ) -> Self {
        let capture_interval = capture_interval.max(1);
        let history_ns = (seconds as u128).saturating_mul(1_000_000_000);
        let snapshot_ns =
            (frame_duration_ns.max(1) as u128).saturating_mul(capture_interval as u128);
        let capacity = usize::try_from(history_ns.div_ceil(snapshot_ns)).unwrap_or(usize::MAX);
        Self {
            snapshots: VecDeque::with_capacity(capacity),
            capacity,
            capture_interval,
            frame_counter: 0,
            emulated_frame: 0,
            scratch: Vec::new(),
        }
    }

    pub fn advance_frames(&mut self, frames: usize) -> bool {
        self.emulated_frame = self.emulated_frame.saturating_add(frames as u64);
        self.frame_counter = self.frame_counter.saturating_add(frames);
        if self.frame_counter >= self.capture_interval {
            self.frame_counter %= self.capture_interval;
            true
        } else {
            false
        }
    }

    pub fn push(&mut self, state_bytes: &[u8], framebuffer: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        if self.snapshots.len() >= self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(RewindSnapshot::compress(
            state_bytes,
            framebuffer,
            self.emulated_frame,
            self.frame_counter,
            &mut self.scratch,
        ));
    }

    pub fn pop(&mut self) -> Option<RewindFrame> {
        self.pop_steps(1)
    }

    pub fn pop_steps(&mut self, steps: usize) -> Option<RewindFrame> {
        let mut snapshot = None;
        for _ in 0..steps.max(1) {
            let Some(next) = self.snapshots.pop_back() else {
                break;
            };
            snapshot = Some(next);
        }
        snapshot.and_then(|snapshot| {
            let rewound_frames = self.emulated_frame.saturating_sub(snapshot.emulated_frame);
            self.emulated_frame = snapshot.emulated_frame;
            self.frame_counter = snapshot.capture_phase;
            let mut frame = snapshot.decompress()?;
            frame.rewound_frames = rewound_frames;
            Some(frame)
        })
    }

    pub fn discard_latest(&mut self) {
        self.snapshots.pop_back();
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
        self.emulated_frame = 0;
    }
}

#[cfg(test)]
mod tests;
