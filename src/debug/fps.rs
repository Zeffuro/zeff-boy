use std::collections::VecDeque;

use crate::platform::Instant;

// FPS tracker
pub(crate) struct FpsTracker {
    samples: VecDeque<(Instant, usize)>,
}

impl FpsTracker {
    pub(crate) fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(120),
        }
    }

    pub(crate) fn tick_n(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        let now = Instant::now();
        self.samples.push_back((now, frames));

        while self.samples.len() > 1 {
            if let Some(&(front, _)) = self.samples.front() {
                if now.duration_since(front).as_secs_f64() > 1.0 {
                    self.samples.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    pub(crate) fn fps(&self) -> f64 {
        let (Some(&(first, first_frames)), Some(&(last, _))) =
            (self.samples.front(), self.samples.back())
        else {
            return 0.0;
        };
        let elapsed = last.duration_since(first).as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        let frames: usize = self.samples.iter().map(|&(_, frames)| frames).sum();
        frames.saturating_sub(first_frames) as f64 / elapsed
    }
}
