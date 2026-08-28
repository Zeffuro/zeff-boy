use super::super::{App, MAX_FRAMES_PER_TICK, SpeedMode};
use crate::emu_thread::EmuCommand;
use crate::platform::Instant;

impl App {
    pub(super) fn sync_speed_setting(&mut self) {
        let uncapped_batch_size =
            uncapped_batch_size(self.settings.emulation.uncapped_frames_per_tick);
        if self.timing.last_uncapped_frames_per_tick != uncapped_batch_size {
            self.timing.last_uncapped_frames_per_tick = uncapped_batch_size;
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SetUncappedBatchSize(uncapped_batch_size));
            }
        }
        if self.timing.uncapped_speed != self.settings.emulation.uncapped_speed {
            self.timing.uncapped_speed = self.settings.emulation.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SetUncapped(
                    self.timing.uncapped_speed && self.recording.allows_uncapped_worker(),
                ));
            }
        }
    }

    pub(super) fn compute_frames_to_step(&mut self, now: Instant) -> usize {
        let speed_mode = self.speed_mode();
        rebase_on_speed_mode_change(
            &mut self.timing.last_speed_mode,
            &mut self.timing.last_frame_time,
            speed_mode,
            now,
        );

        match speed_mode {
            SpeedMode::Uncapped => {
                self.timing.last_frame_time = now;
                let batch_size =
                    uncapped_batch_size(self.settings.emulation.uncapped_frames_per_tick);
                #[cfg(target_arch = "wasm32")]
                {
                    batch_size
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.recording.is_replay_active() {
                        batch_size
                    } else {
                        1
                    }
                }
            }
            SpeedMode::Normal | SpeedMode::SlowMotion | SpeedMode::FastForward => {
                let effective_duration = self.effective_frame_duration();
                let max_frames = if speed_mode == SpeedMode::FastForward {
                    self.settings
                        .emulation
                        .fast_forward_multiplier
                        .clamp(1, MAX_FRAMES_PER_TICK)
                } else {
                    MAX_FRAMES_PER_TICK
                };
                let discard_timing_debt =
                    speed_mode == SpeedMode::FastForward || self.settings.emulation.frame_skip;
                let frames = frames_due(
                    &mut self.timing.last_frame_time,
                    now,
                    effective_duration,
                    max_frames,
                    discard_timing_debt,
                );

                #[cfg(target_arch = "wasm32")]
                if matches!(speed_mode, SpeedMode::Normal | SpeedMode::SlowMotion) && frames > 3 {
                    self.timing.last_frame_time = now;
                    return 3;
                }

                frames
            }
        }
    }
}

fn uncapped_batch_size(configured: usize) -> usize {
    configured.clamp(1, crate::emu_thread::MAX_UNCAPPED_BATCH_SIZE)
}

fn rebase_on_speed_mode_change(
    last_speed_mode: &mut SpeedMode,
    last_frame_time: &mut Instant,
    speed_mode: SpeedMode,
    now: Instant,
) {
    if speed_mode != *last_speed_mode {
        *last_speed_mode = speed_mode;
        *last_frame_time = now;
    }
}

fn frames_due(
    last_frame_time: &mut Instant,
    now: Instant,
    frame_duration: std::time::Duration,
    max_frames: usize,
    discard_timing_debt: bool,
) -> usize {
    let mut frames = 0;
    while *last_frame_time + frame_duration <= now {
        frames += 1;
        *last_frame_time += frame_duration;
        if frames >= max_frames {
            if discard_timing_debt {
                *last_frame_time = now;
            }
            break;
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncapped_batch_setting_is_bounded_for_worker_and_foreground_paths() {
        assert_eq!(uncapped_batch_size(0), 1);
        assert_eq!(uncapped_batch_size(17), 17);
        assert_eq!(
            uncapped_batch_size(usize::MAX),
            crate::emu_thread::MAX_UNCAPPED_BATCH_SIZE
        );
    }

    #[test]
    fn capped_fast_forward_rebases_timing_debt() {
        let start = Instant::now();
        let now = start + std::time::Duration::from_millis(100);
        let mut last_frame_time = start;

        assert_eq!(
            frames_due(
                &mut last_frame_time,
                now,
                std::time::Duration::from_millis(1),
                4,
                true,
            ),
            4
        );
        assert_eq!(last_frame_time, now);
    }

    #[test]
    fn normal_mode_keeps_timing_debt_without_frame_skip() {
        let start = Instant::now();
        let now = start + std::time::Duration::from_millis(100);
        let mut last_frame_time = start;

        assert_eq!(
            frames_due(
                &mut last_frame_time,
                now,
                std::time::Duration::from_millis(1),
                4,
                false,
            ),
            4
        );
        assert_eq!(last_frame_time, start + std::time::Duration::from_millis(4));
    }

    #[test]
    fn normal_mode_recovers_all_frames_after_a_long_host_stall() {
        let start = Instant::now();
        let now = start + std::time::Duration::from_millis(100);
        let mut last_frame_time = start;
        let mut frames = 0;
        let mut batches = 0;

        loop {
            let due = frames_due(
                &mut last_frame_time,
                now,
                std::time::Duration::from_millis(1),
                4,
                false,
            );
            if due == 0 {
                break;
            }
            frames += due;
            batches += 1;
        }

        assert_eq!(frames, 100);
        assert_eq!(batches, 25);
        assert_eq!(last_frame_time, now);
    }

    #[test]
    fn speed_transitions_do_not_carry_fast_forward_debt_into_normal_mode() {
        let start = Instant::now();
        let entered_fast_forward = start + std::time::Duration::from_millis(100);
        let overloaded = entered_fast_forward + std::time::Duration::from_millis(100);
        let returned_to_normal = overloaded + std::time::Duration::from_millis(1);
        let frame_duration = std::time::Duration::from_millis(1);
        let mut last_speed_mode = SpeedMode::Normal;
        let mut last_frame_time = start;

        rebase_on_speed_mode_change(
            &mut last_speed_mode,
            &mut last_frame_time,
            SpeedMode::FastForward,
            entered_fast_forward,
        );
        assert_eq!(last_frame_time, entered_fast_forward);
        assert_eq!(
            frames_due(&mut last_frame_time, overloaded, frame_duration, 4, true,),
            4
        );
        assert_eq!(last_frame_time, overloaded);

        rebase_on_speed_mode_change(
            &mut last_speed_mode,
            &mut last_frame_time,
            SpeedMode::Normal,
            returned_to_normal,
        );
        assert_eq!(last_frame_time, returned_to_normal);
        assert_eq!(
            frames_due(
                &mut last_frame_time,
                returned_to_normal,
                frame_duration,
                MAX_FRAMES_PER_TICK,
                false,
            ),
            0
        );
    }
}
