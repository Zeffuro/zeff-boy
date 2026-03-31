use std::collections::VecDeque;
use std::time::Instant;

// FPS tracker
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
            if let Some(&front) = self.timestamps.front() {
                if now.duration_since(front).as_secs_f64() > 1.0 {
                    self.timestamps.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    pub(crate) fn fps(&self) -> f64 {
        let (Some(&first), Some(&last)) = (self.timestamps.front(), self.timestamps.back()) else {
            return 0.0;
        };
        let elapsed = last.duration_since(first).as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        (self.timestamps.len() - 1) as f64 / elapsed
    }
}
