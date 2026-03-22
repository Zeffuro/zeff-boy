use super::{App, GB_FRAME_DURATION, SpeedMode};
use crate::{audio::AudioOutput, graphics::Graphics};
use winit::event_loop::{ActiveEventLoop, ControlFlow};

impl App {
    pub(super) fn ensure_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        if let Some(emu) = self.emulator.as_ref() {
            self.emu_thread = Some(crate::emu_thread::EmuThread::spawn(std::sync::Arc::clone(
                emu,
            )));
        }
    }

    pub(super) fn handle_resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }

        if self.audio.is_none() {
            self.audio = AudioOutput::new();
            if let (Some(audio), Some(emu)) = (self.audio.as_ref(), self.emulator.as_ref()) {
                let mut emu = emu.lock().expect("emulator mutex poisoned");
                emu.bus.io.apu.set_sample_rate(audio.sample_rate());
            }
        }

        self.ensure_emu_thread();

        let mut gfx =
            pollster::block_on(Graphics::new(event_loop)).expect("failed to initialize graphics");
        gfx.set_uncapped_present_mode(self.uncapped_speed);
        let size = gfx.window().inner_size();
        self.window_size = (size.width as f32, size.height as f32);
        self.window_id = Some(gfx.window().id());
        self.gfx = Some(gfx);
    }

    pub(super) fn schedule_next_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = &self.gfx else {
            return;
        };

        match self.speed_mode() {
            SpeedMode::Normal => {
                let now = std::time::Instant::now();
                let next_frame_time = self.last_frame_time + GB_FRAME_DURATION;
                if now >= next_frame_time {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    gfx.window().request_redraw();
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
                }
            }
            SpeedMode::Uncapped | SpeedMode::FastForward => {
                event_loop.set_control_flow(ControlFlow::Poll);
                gfx.window().request_redraw();
            }
        }
    }
}
