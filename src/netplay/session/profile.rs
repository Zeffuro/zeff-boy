use std::time::{Duration, Instant};

use anyhow::{Result, ensure};

pub(super) const MAX_RECORDS: usize = 8192;

#[derive(Default)]
pub(super) struct ExecutionProfile {
    pub frames: Vec<FrameCost>,
}

pub(super) struct FrameCost {
    pub frame: u64,
    pub advance: Duration,
    pub capture: Duration,
    pub checkpoint: Option<Duration>,
    pub total: Duration,
}

pub(super) struct FrameTimer {
    started: Instant,
    stage: Instant,
    advance: Duration,
    capture: Duration,
}

impl FrameTimer {
    pub fn begin(profile: Option<&ExecutionProfile>) -> Result<Option<Self>> {
        let Some(profile) = profile else {
            return Ok(None);
        };
        ensure!(
            profile.frames.len() < MAX_RECORDS,
            "execution profile is full"
        );
        let started = Instant::now();
        Ok(Some(Self {
            started,
            stage: started,
            advance: Duration::ZERO,
            capture: Duration::ZERO,
        }))
    }

    pub fn advanced(&mut self) {
        let now = Instant::now();
        self.advance = now.duration_since(self.stage);
        self.stage = now;
    }

    pub fn captured(&mut self) {
        let now = Instant::now();
        self.capture = now.duration_since(self.stage);
        self.stage = now;
    }

    pub fn checkpointed(&self, present: bool) -> Option<Duration> {
        present.then(|| self.stage.elapsed())
    }

    pub fn finish(self, frame: u64, checkpoint: Option<Duration>) -> FrameCost {
        FrameCost {
            frame,
            advance: self.advance,
            capture: self.capture,
            checkpoint,
            total: self.started.elapsed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_profile_and_full_profile_do_not_start_timers() {
        assert!(FrameTimer::begin(None).unwrap().is_none());
        let mut profile = ExecutionProfile::default();
        for frame in 0..MAX_RECORDS {
            profile.frames.push(FrameCost {
                frame: frame as u64,
                advance: Duration::ZERO,
                capture: Duration::ZERO,
                checkpoint: None,
                total: Duration::ZERO,
            });
        }
        assert!(FrameTimer::begin(Some(&profile)).is_err());
    }
}
