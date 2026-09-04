use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use super::App;
use crate::debug::TasEditorPresentation;
use crate::graphics;
use crate::platform::Instant;

impl App {
    pub(in crate::app) fn sync_tas_editor(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_verified_replay_export();
        self.debug_windows.tas_editor.tick_periodic_autosave();
        let wants_window = self.debug_windows.tas_editor.open
            && self.debug_windows.tas_editor.presentation()
                == TasEditorPresentation::SeparateWindow;
        let focus_requested = self.debug_windows.tas_editor.take_separate_focus_request();
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let mut close_request = None;
        if wants_window {
            if gfx.tas_editor_window_id().is_none()
                && let Err(error) = gfx.open_tas_editor_window(event_loop)
            {
                log::error!("Failed to open TAS Editor window: {error}");
                self.debug_windows.tas_editor.close();
                close_request = self.debug_windows.tas_editor.take_pending_host_request();
                self.toast_manager.error("Failed to open TAS Editor window");
            }
            if focus_requested && let Some(window) = gfx.tas_editor_window() {
                window.focus_window();
                window.request_redraw();
            }
        } else if gfx.tas_editor_window_id().is_some() {
            gfx.close_tas_editor_window();
            self.debug_windows.tas_editor.set_host_window_focused(false);
            self.focus_state_dirty = true;
        }
        if let Some(request) = close_request {
            self.handle_tas_editor_host_request(request);
        }
    }

    pub(in crate::app) fn is_tas_editor_window(&self, window_id: winit::window::WindowId) -> bool {
        self.gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::tas_editor_window_id)
            == Some(window_id)
    }

    pub(in crate::app) fn handle_tas_editor_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.tas_editor_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                if let Some(export) = self.tas_verified_replay_export.as_ref() {
                    export.request_cancel();
                    self.toast_manager.info(
                        "Canceling verified replay export; the TAS Editor remains open until it finishes",
                    );
                    return;
                }
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_tas_editor_window();
                }
                self.debug_windows.tas_editor.close();
                if let Some(request) = self.debug_windows.tas_editor.take_pending_host_request() {
                    self.handle_tas_editor_host_request(request);
                }
                self.debug_windows.tas_editor.set_host_window_focused(false);
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_tas_editor_window(size.width, size.height);
                    if let Some(window) = gfx.tas_editor_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_tas_editor_frame();
                self.debug_windows.tas_editor.mark_host_rendered();
            }
            WindowEvent::Focused(focused) => {
                self.debug_windows
                    .tas_editor
                    .set_host_window_focused(focused);
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now()
                    .duration_since(self.debug_windows.tas_editor.last_host_render())
                    >= super::super::VIEWER_UPDATE_INTERVAL =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::tas_editor_window)
                {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        if window_interaction {
            self.tick_during_window_interaction();
        }
    }

    pub(in crate::app) fn render_tas_editor_frame(&mut self) -> bool {
        self.refresh_tas_editor_live_status();
        let result = {
            let Some(gfx) = self.gfx.as_mut() else {
                return false;
            };
            gfx.render_tas_editor_window(graphics::TasEditorRenderContext {
                settings: &self.settings,
                state: &mut self.debug_windows.tas_editor,
            })
        };
        match result {
            Ok(result) => {
                if let Some(request) = result.host_request {
                    self.handle_tas_editor_host_request(request);
                }
                true
            }
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(gfx) = self.gfx.as_mut()
                    && let Some(size) = gfx.tas_editor_window().map(|window| window.inner_size())
                {
                    gfx.resize_tas_editor_window(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }
}
