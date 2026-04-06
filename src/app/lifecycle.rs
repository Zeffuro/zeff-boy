use super::{App, SpeedMode};
use crate::{
    audio::AudioOutput,
    emu_thread::{EmuCommand, EmuThread},
    graphics::Graphics,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Fullscreen;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

impl App {
    pub(super) fn reset_audio_output(&mut self) {
        let preferred = self.settings.audio.output_sample_rate;
        self.audio = AudioOutput::new(Some(preferred))
            .map_err(|e| log::warn!("Audio init failed: {e}"))
            .ok();
        if let (Some(audio), Some(thread)) = (self.audio.as_ref(), &self.emu_thread) {
            thread.send(EmuCommand::SetSampleRate(audio.sample_rate()));
        }
    }

    pub(super) fn ensure_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        if let Some(backend) = self.initial_backend.take() {
            self.emu_thread = Some(EmuThread::spawn(backend));
            if self.timing.uncapped_speed
                && let Some(thread) = &self.emu_thread
            {
                thread.send(EmuCommand::SetUncapped(true));
            }
        }
    }

    pub(super) fn handle_resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }

        if self.audio.is_none() {
            self.reset_audio_output();
        }

        self.ensure_emu_thread();

        if let (Some(audio), Some(thread)) = (self.audio.as_ref(), &self.emu_thread) {
            thread.send(EmuCommand::SetSampleRate(audio.sample_rate()));
        }

        #[cfg(not(target_arch = "wasm32"))]
        let gfx = pollster::block_on(Graphics::new(event_loop, self.settings.video.vsync_mode))
            .expect("failed to initialize graphics");

        #[cfg(target_arch = "wasm32")]
        let gfx = pollster_lite_block(Graphics::new(event_loop, self.settings.video.vsync_mode))
            .expect("failed to initialize graphics");

        let size = gfx.window().inner_size();
        self.window_size = (size.width as f32, size.height as f32);
        self.window_id = Some(gfx.window().id());

        if self.settings.ui.ui_scale_needs_auto {
            let monitor_height = gfx
                .window()
                .current_monitor()
                .map(|m| m.size().height)
                .unwrap_or(1080);
            let scale_factor = gfx.window().scale_factor();
            self.settings
                .auto_detect_ui_scale(monitor_height, scale_factor);
        }

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = gfx.window().canvas() {
                let window = web_sys::window().unwrap();
                let document = window.document().unwrap();
                let body = document.body().unwrap();
                let _ = body.append_child(&canvas);
                canvas.set_attribute("style", "width:100%;height:100%").ok();
            }
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
                let now = Instant::now();
                let next_frame_time = self.timing.last_frame_time + effective;
                if now >= next_frame_time {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    gfx.window().request_redraw();
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time.into()));
                }
            }
            SpeedMode::FastForward | SpeedMode::Uncapped => {
                event_loop.set_control_flow(ControlFlow::Poll);
                gfx.window().request_redraw();
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn pollster_lite_block<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Wake, Waker};
    use std::pin::pin;
    struct NoopWake;
    impl Wake for NoopWake { fn wake(self: std::sync::Arc<Self>) {} }
    let waker = Waker::from(std::sync::Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(val) => val,
        Poll::Pending => panic!("GPU init returned Pending on WASM"),
    }
}
