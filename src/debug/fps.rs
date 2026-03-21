use std::collections::VecDeque;
use std::time::Instant;

/// FPS tracker
pub(crate) struct FpsTracker {
    timestamps: VecDeque<Instant>,
}

impl FpsTracker {
    pub(crate) fn new() -> Self {
        Self {
            timestamps: VecDeque::with_capacity(120),
        }
    }

    pub(crate) fn tick(&mut self) {
        let now = Instant::now();
        self.timestamps.push_back(now);

        while self.timestamps.len() > 1 {
            if now
                .duration_since(*self.timestamps.front().unwrap())
                .as_secs_f64()
                > 1.0
            {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    pub(crate) fn fps(&self) -> f64 {
        if self.timestamps.len() < 2 {
            return 0.0;
        }
        let first = *self.timestamps.front().unwrap();
        let last = *self.timestamps.back().unwrap();
        let elapsed = last.duration_since(first).as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        (self.timestamps.len() - 1) as f64 / elapsed
    }
}
