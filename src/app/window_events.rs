use super::App;
use crate::platform::Instant;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::WindowId};

impl App {
    pub(super) fn handle_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.gfx.is_none() {
            return;
        }
        if Some(window_id) != self.window_id {
            return;
        }

        self.update_pointer_and_window_state(&event);

        let keyboard_event = match &event {
            WindowEvent::KeyboardInput { event, .. } => Some(event),
            _ => None,
        };
        let event_consumed_by_egui = self.gfx_handles_event(&event);

        if let Some(key_event) = keyboard_event {
            self.handle_keyboard_input(key_event, event_consumed_by_egui);
        }

        if event_consumed_by_egui {
            return;
        }

        self.dispatch_window_event(event_loop, event);
    }

    fn update_pointer_and_window_state(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some((position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
            }
            WindowEvent::Resized(size) => {
                self.window_size = (size.width as f32, size.height as f32);
            }
            _ => {}
        }
    }

    fn gfx_handles_event(&mut self, event: &WindowEvent) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        gfx.handle_event(event)
    }

    fn dispatch_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.perform_shutdown();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize(size.width, size.height);
                }
            }
            WindowEvent::DroppedFile(path) => self.handle_dropped_file(path),
            WindowEvent::RedrawRequested => self.tick(),
            WindowEvent::Focused(focused) => self.handle_focus_change(focused),
            _ => {}
        }

        if self.exit_requested {
            self.perform_shutdown();
            event_loop.exit();
        }
    }

    pub(super) fn handle_focus_change(&mut self, focused: bool) {
        if focused {
            self.timing.last_frame_time = Instant::now();

            if self.paused_by_unfocus {
                self.paused_by_unfocus = false;
                self.speed.paused = false;
                self.toast_manager.set_paused(false);
            }
        } else if self.settings.emulation.pause_on_unfocus && !self.speed.paused {
            self.paused_by_unfocus = true;
            self.speed.paused = true;
            self.toast_manager.set_paused(true);
        }
    }
}
