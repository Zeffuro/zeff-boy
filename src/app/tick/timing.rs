use super::super::{App, MAX_FRAMES_PER_TICK, SpeedMode};
use crate::emu_thread::EmuCommand;
use crate::platform::Instant;

impl App {
    pub(super) fn sync_speed_setting(&mut self) {
        if self.timing.uncapped_speed != self.settings.emulation.uncapped_speed {
            self.timing.uncapped_speed = self.settings.emulation.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SetUncapped(self.timing.uncapped_speed));
            }
        }
    }

    pub(super) fn compute_frames_to_step(&mut self, now: Instant) -> usize {
        match self.speed_mode() {
            SpeedMode::Uncapped => {
                self.timing.last_frame_time = now;
                #[cfg(target_arch = "wasm32")]
                {
                    MAX_FRAMES_PER_TICK
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    1
                }
            }
            SpeedMode::Normal | SpeedMode::SlowMotion | SpeedMode::FastForward => {
                let effective_duration = self.effective_frame_duration();

                let mut frames = 0usize;
                while self.timing.last_frame_time + effective_duration <= now {
                    frames += 1;
                    self.timing.last_frame_time += effective_duration;
                    if frames >= MAX_FRAMES_PER_TICK {
                        if self.settings.emulation.frame_skip {
                            self.timing.last_frame_time = now;
                        }
                        break;
                    }
                }

                #[cfg(target_arch = "wasm32")]
                if matches!(self.speed_mode(), SpeedMode::Normal | SpeedMode::SlowMotion)
                    && frames > 3
                {
                    self.timing.last_frame_time = now;
                    frames = 3;
                }

                frames
            }
        }
    }
}
