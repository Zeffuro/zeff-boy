use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::emu_thread::EmuCommand;
use crate::{debug::MenuAction, graphics, platform::Instant};

use super::App;

impl App {
    pub(in crate::app) fn sync_netplay_window(&mut self, event_loop: &ActiveEventLoop) {
        let wants_window = self.debug_windows.netplay.is_open();
        let focus_requested = self.debug_windows.netplay.take_focus_request();
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if wants_window {
            if let Err(error) = gfx.open_netplay_window(event_loop, &self.settings) {
                log::error!("Failed to open Netplay window: {error}");
                self.debug_windows.netplay.close();
                self.focus_state_dirty = true;
                self.toast_manager.error("Failed to open Netplay window");
                return;
            }
            if focus_requested && let Some(window) = gfx.netplay_window() {
                window.set_minimized(false);
                window.focus_window();
                window.request_redraw();
            }
        } else if gfx.netplay_window_id().is_some() {
            gfx.close_netplay_window();
            self.debug_windows.netplay.set_host_window_focused(false);
            self.focus_state_dirty = true;
        }
    }

    pub(in crate::app) fn is_netplay_window(&self, id: WindowId) -> bool {
        self.gfx
            .as_ref()
            .and_then(graphics::Graphics::netplay_window_id)
            == Some(id)
    }

    pub(in crate::app) fn handle_netplay_window_event(&mut self, event: WindowEvent) {
        let interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.netplay_handles_event(&event));
        match event {
            WindowEvent::CloseRequested => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_netplay_window();
                }
                self.debug_windows.netplay.close();
                self.focus_state_dirty = true;
            }
            WindowEvent::Focused(focused) => {
                self.debug_windows.netplay.set_host_window_focused(focused);
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_netplay_window(size.width, size.height);
                    if let Some(window) = gfx.netplay_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_netplay_frame();
            }
            _ if repaint => {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(graphics::Graphics::netplay_window)
                {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        if interaction {
            self.tick_during_window_interaction();
        }
    }

    pub(in crate::app) fn render_netplay_frame(&mut self) {
        self.debug_windows.netplay.mark_host_rendered();
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let result = gfx.render_netplay_window(graphics::NetplayRenderContext {
            settings: &self.settings,
            state: &mut self.debug_windows.netplay,
            system: self.active_system,
        });
        match result {
            Ok(Some(action)) => self.handle_netplay_action(&action),
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.netplay_window().map(|window| window.inner_size()) {
                    gfx.resize_netplay_window(size.width, size.height);
                }
            }
            Ok(None) | Err(graphics::FrameError::Timeout) => {}
        }
    }

    pub(in crate::app) fn redraw_netplay_status(&mut self, now: Instant) {
        if self.debug_windows.netplay.is_open()
            && now.duration_since(self.debug_windows.netplay.last_host_render())
                >= super::super::VIEWER_UPDATE_INTERVAL
            && let Some(window) = self
                .gfx
                .as_ref()
                .and_then(graphics::Graphics::netplay_window)
            && window.is_minimized() != Some(true)
        {
            window.request_redraw();
        }
    }

    pub(in crate::app) fn handle_netplay_action(&mut self, action: &MenuAction) {
        let result = match action {
            MenuAction::HostNesNetplay => self.begin_netplay(None),
            MenuAction::JoinNesNetplay(invitation) => self.begin_netplay(Some(invitation.clone())),
            MenuAction::StopNesNetplay => {
                self.request_netplay_stop();
                Ok(())
            }
            MenuAction::SetNesNetplayPaused(paused) => {
                self.set_netplay_paused(*paused);
                Ok(())
            }
            MenuAction::SendNesNetplayChat(text) => {
                if self.netplay.running() {
                    if let Err(error) =
                        self.send_emu_command_checked(EmuCommand::SendNetplayChat(text.clone()))
                    {
                        self.debug_windows.netplay.chat_failed(error.to_string());
                    }
                } else {
                    self.debug_windows
                        .netplay
                        .chat_failed("Connect before sending a message.".into());
                }
                Ok(())
            }
            _ => return,
        };
        if let Err(error) = result {
            self.debug_windows.netplay.status = error.to_string();
            self.toast_manager.error(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
