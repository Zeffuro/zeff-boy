use super::{App, SpeedMode};
use crate::{audio::AudioOutput, emu_thread::EmuThread, graphics::Graphics};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Fullscreen;

impl App {
    pub(super) fn ensure_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        if let Some(backend) = self.initial_backend.take() {
            self.emu_thread = Some(EmuThread::spawn(backend));
            if self.timing.uncapped_speed
                && let Some(thread) = &self.emu_thread {
                    thread.send(crate::emu_thread::EmuCommand::SetUncapped(true));
                }
        }
    }

    pub(super) fn handle_resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }

        if self.audio.is_none() {
            self.audio = AudioOutput::new();
            if let (Some(audio), Some(thread)) = (self.audio.as_ref(), &self.emu_thread) {
                thread.send(crate::emu_thread::EmuCommand::SetSampleRate(
                    audio.sample_rate(),
                ));
            }
        }

        self.ensure_emu_thread();

        if let (Some(audio), Some(thread)) = (self.audio.as_ref(), &self.emu_thread) {
            thread.send(crate::emu_thread::EmuCommand::SetSampleRate(
                audio.sample_rate(),
            ));
        }

        let gfx = pollster::block_on(Graphics::new(event_loop, self.settings.vsync_mode))
            .expect("failed to initialize graphics");
        let size = gfx.window().inner_size();
        self.window_size = (size.width as f32, size.height as f32);
        self.window_id = Some(gfx.window().id());

        if self.settings.ui_scale_needs_auto {
            let monitor_height = gfx
                .window()
                .current_monitor()
                .map(|m| m.size().height)
                .unwrap_or(1080);
            let scale_factor = gfx.window().scale_factor();
            self.settings.auto_detect_ui_scale(monitor_height, scale_factor);
        }

        self.gfx = Some(gfx);
    }

    pub(super) fn toggle_fullscreen(&mut self) {
        let Some(gfx) = &self.gfx else {
            return;
        };
        let window = gfx.window();
        if window.fullscreen().is_some() {
            window.set_fullscreen(None);
        } else {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
    }

    pub(super) fn schedule_next_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = &self.gfx else {
            return;
        };

        match self.speed_mode() {
            SpeedMode::Normal => {
                let effective = self.effective_frame_duration();
                let now = std::time::Instant::now();
                let next_frame_time = self.timing.last_frame_time + effective;
                if now >= next_frame_time {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    gfx.window().request_redraw();
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
                }
            }
            SpeedMode::FastForward => {
                event_loop.set_control_flow(ControlFlow::Poll);
                gfx.window().request_redraw();
            }
            SpeedMode::Uncapped => {
                event_loop.set_control_flow(ControlFlow::Poll);
                gfx.window().request_redraw();
            }
        }
    }
}
