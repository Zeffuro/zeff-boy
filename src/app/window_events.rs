use super::App;
use crate::platform::Instant;
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
};

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
        if Some(window_id) == self.window_id {
            self.handle_game_window_event(event_loop, event);
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            if self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::settings_window_id)
                == Some(window_id)
            {
                self.handle_settings_window_event(event);
            } else if self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::debugger_window_id)
                == Some(window_id)
            {
                self.handle_debugger_window_event(event);
            }
        }
    }

    fn handle_game_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        self.update_pointer_and_window_state(&event);

        let keyboard_event = match &event {
            WindowEvent::KeyboardInput { event, .. } => Some(event),
            _ => None,
        };
        let event_consumed_by_egui = self.gfx_handles_event(&event);
        self.handle_mouse_input_for_zapper(&event, event_consumed_by_egui);

        if let Some(key_event) = keyboard_event {
            self.handle_keyboard_input(key_event, event_consumed_by_egui);
        }

        if event_consumed_by_egui {
            return;
        }

        self.dispatch_window_event(event_loop, event);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_debugger_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.debugger_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                self.persist_debugger_window_geometry();
                self.persist_current_dock_layout();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_debugger_window();
                }
                self.settings.ui.debugger_window_open = false;
                self.settings.save();
                self.debugger_window_focused = false;
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_debugger(size.width, size.height);
                    if gfx.debugger_window().is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_size(
                            window,
                            size,
                            crate::graphics::window_geometry::DEBUGGER_MIN_SIZE,
                        )
                    }) {
                        self.settings.ui.debugger_window_size = [size.width, size.height];
                    }
                    if let Some(window) = gfx.debugger_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::debugger_window)
                    .is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_position(window, position)
                    })
                {
                    self.settings.ui.debugger_window_position = Some([position.x, position.y]);
                }
            }
            WindowEvent::RedrawRequested => {
                let data = self.cached_ui_data.take();
                self.render_debugger_frame(data.as_ref());
                self.cached_ui_data = data;
                self.last_debugger_render = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.debugger_window_focused = focused;
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now().duration_since(self.last_debugger_render)
                    >= std::time::Duration::from_millis(16) =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::debugger_window)
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

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_settings_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.settings_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                self.persist_settings_window_geometry();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_settings_window();
                }
                self.show_settings_window = false;
                self.clear_rebinding_state();
                self.settings.save();
                self.settings_window_focused = false;
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_settings_window(size.width, size.height);
                    if gfx.settings_window().is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_size(
                            window,
                            size,
                            crate::graphics::window_geometry::SETTINGS_MIN_SIZE,
                        )
                    }) {
                        self.settings.ui.settings_window_size = [size.width, size.height];
                    }
                    if let Some(window) = gfx.settings_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::settings_window)
                    .is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_position(window, position)
                    })
                {
                    self.settings.ui.settings_window_position = Some([position.x, position.y]);
                }
            }
            WindowEvent::RedrawRequested => {
                let before = self.settings.clone();
                let data = self.cached_ui_data.take();
                self.render_settings_frame(data.as_ref());
                self.cached_ui_data = data;
                if self.settings != before {
                    self.settings.save();
                    if let Some(gfx) = self.gfx.as_ref() {
                        gfx.window().request_redraw();
                        if let Some(window) = gfx.settings_window() {
                            window.request_redraw();
                        }
                    }
                }
                self.last_settings_render = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.settings_window_focused = focused;
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now().duration_since(self.last_settings_render)
                    >= std::time::Duration::from_millis(16) =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::settings_window)
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

    #[cfg(not(target_arch = "wasm32"))]
    fn tick_during_window_interaction(&mut self) {
        self.apply_focus_state();
        if Instant::now().duration_since(self.timing.last_render_time) >= super::UI_RENDER_INTERVAL
        {
            self.tick();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn persist_debugger_window_geometry(&mut self) {
        let Some(window) = self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::debugger_window)
        else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        self.settings.ui.debugger_window_maximized = window.is_maximized();
        let size = window.inner_size();
        if crate::graphics::window_geometry::can_persist_size(
            window,
            size,
            crate::graphics::window_geometry::DEBUGGER_MIN_SIZE,
        ) {
            self.settings.ui.debugger_window_size = [size.width, size.height];
            if let Ok(position) = window.outer_position()
                && crate::graphics::window_geometry::can_persist_position(window, position)
            {
                self.settings.ui.debugger_window_position = Some([position.x, position.y]);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn persist_settings_window_geometry(&mut self) {
        let Some(window) = self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::settings_window)
        else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        self.settings.ui.settings_window_maximized = window.is_maximized();
        let size = window.inner_size();
        if crate::graphics::window_geometry::can_persist_size(
            window,
            size,
            crate::graphics::window_geometry::SETTINGS_MIN_SIZE,
        ) {
            self.settings.ui.settings_window_size = [size.width, size.height];
            if let Ok(position) = window.outer_position()
                && crate::graphics::window_geometry::can_persist_position(window, position)
            {
                self.settings.ui.settings_window_position = Some([position.x, position.y]);
            }
        }
    }

    pub(super) fn persist_current_dock_layout(&mut self) {
        if let Some(layout) = crate::debug::serialize_dock_layout(&self.debug_dock) {
            self.settings
                .ui
                .set_dock_layout(self.active_debug_presentation, layout);
        }
    }

    fn update_pointer_and_window_state(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some((position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
                self.mouse_left_pressed = false;
            }
            WindowEvent::Resized(size) => {
                self.window_size = (size.width as f32, size.height as f32);
            }
            _ => {}
        }
    }

    fn handle_mouse_input_for_zapper(&mut self, event: &WindowEvent, event_consumed_by_egui: bool) {
        let WindowEvent::MouseInput {
            state,
            button: MouseButton::Left,
            ..
        } = event
        else {
            return;
        };

        match state {
            ElementState::Pressed => {
                let pointer_over_direct_game = self.pointer_over_direct_game_view();
                self.mouse_left_pressed =
                    self.game_view_focused && (!event_consumed_by_egui || pointer_over_direct_game);
            }
            ElementState::Released => {
                self.mouse_left_pressed = false;
            }
        }
    }

    fn pointer_over_direct_game_view(&self) -> bool {
        let Some((x, y)) = self.cursor_pos else {
            return false;
        };
        self.gfx
            .as_ref()
            .and_then(|gfx| gfx.game_pixel_at_window_pos(x, y))
            .is_some()
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
                #[cfg(not(target_arch = "wasm32"))]
                self.tick_during_window_interaction();
            }
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::Moved(_) => self.tick_during_window_interaction(),
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
        self.game_window_focused = focused;
        self.focus_state_dirty = true;
    }

    pub(super) fn apply_focus_state(&mut self) {
        if !self.focus_state_dirty {
            return;
        }
        self.focus_state_dirty = false;
        #[cfg(not(target_arch = "wasm32"))]
        let debugger_window_focused = self.debugger_window_focused;
        #[cfg(target_arch = "wasm32")]
        let debugger_window_focused = false;
        #[cfg(not(target_arch = "wasm32"))]
        let settings_window_focused = self.settings_window_focused;
        #[cfg(target_arch = "wasm32")]
        let settings_window_focused = false;
        let focused = any_app_window_focused(
            self.game_window_focused,
            debugger_window_focused,
            settings_window_focused,
        );

        self.window_focused = focused;
        if focused {
            self.timing.last_frame_time = Instant::now();
            self.suppress_unfocus_pause_until_focus = false;

            if self.paused_by_unfocus {
                self.paused_by_unfocus = false;
                self.speed.paused = false;
                self.toast_manager.set_paused(false);
            }
        } else {
            let link_keeps_running = self.link_keeps_running();
            if should_pause_on_unfocus(
                self.suppress_unfocus_pause_until_focus,
                self.settings.emulation.pause_on_unfocus,
                link_keeps_running,
                self.speed.paused,
            ) {
                self.paused_by_unfocus = true;
                self.speed.paused = true;
                self.toast_manager.set_paused(true);
            }
        }
    }
}

fn any_app_window_focused(
    game_window_focused: bool,
    debugger_window_focused: bool,
    settings_window_focused: bool,
) -> bool {
    game_window_focused || debugger_window_focused || settings_window_focused
}

fn should_pause_on_unfocus(
    suppress_unfocus_pause_until_focus: bool,
    pause_on_unfocus: bool,
    link_keeps_running: bool,
    paused: bool,
) -> bool {
    if suppress_unfocus_pause_until_focus {
        return false;
    }
    pause_on_unfocus && !link_keeps_running && !paused
}

#[cfg(test)]
mod tests {
    use super::{any_app_window_focused, should_pause_on_unfocus};

    #[test]
    fn either_native_window_keeps_the_app_focused() {
        assert!(any_app_window_focused(true, false, false));
        assert!(any_app_window_focused(false, true, false));
        assert!(any_app_window_focused(false, false, true));
        assert!(!any_app_window_focused(false, false, false));
    }

    #[test]
    fn dialog_unfocus_suppression_blocks_until_focus_returns() {
        let suppress = true;

        assert!(!should_pause_on_unfocus(suppress, true, false, false));
        assert!(!should_pause_on_unfocus(suppress, true, false, false));
        assert!(should_pause_on_unfocus(false, true, false, false));
    }

    #[test]
    fn unfocus_does_not_pause_while_link_keeps_running_or_already_paused() {
        assert!(!should_pause_on_unfocus(false, true, true, false));
        assert!(!should_pause_on_unfocus(false, true, false, true));
        assert!(!should_pause_on_unfocus(false, false, false, false));
    }
}
