use super::App;
use crate::platform::Instant;
use winit::{
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    window::{CursorGrabMode, WindowId},
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
                .and_then(crate::graphics::Graphics::mods_window_id)
                == Some(window_id)
            {
                self.handle_mods_window_event(event);
            } else if self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::cheats_window_id)
                == Some(window_id)
            {
                self.handle_cheats_window_event(event);
            } else if self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::printer_window_id)
                == Some(window_id)
            {
                self.handle_printer_window_event(event);
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
                self.focus_settings_window_pending = false;
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
    fn handle_printer_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.printer_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                self.persist_printer_window_geometry();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_printer_window();
                }
                self.debug_windows.printer.clear_textures();
                self.show_printer_window = false;
                self.focus_printer_window_pending = false;
                self.settings.save();
                self.printer_window_focused = false;
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_printer_window(size.width, size.height);
                    if gfx.printer_window().is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_size(
                            window,
                            size,
                            crate::graphics::window_geometry::PRINTER_MIN_SIZE,
                        )
                    }) {
                        self.settings.ui.printer_window_size = [size.width, size.height];
                    }
                    if let Some(window) = gfx.printer_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::printer_window)
                    .is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_position(window, position)
                    })
                {
                    self.settings.ui.printer_window_position = Some([position.x, position.y]);
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_printer_frame();
                self.last_printer_render = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.printer_window_focused = focused;
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now().duration_since(self.last_printer_render)
                    >= std::time::Duration::from_millis(16) =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::printer_window)
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
    fn handle_mods_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.mods_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                self.persist_mods_window_geometry();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_mods_window();
                }
                self.show_mods_window = false;
                self.settings.save();
                self.mods_window_focused = false;
                self.focus_mods_window_pending = false;
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_mods_window(size.width, size.height);
                    if gfx.mods_window().is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_size(
                            window,
                            size,
                            crate::graphics::window_geometry::MODS_MIN_SIZE,
                        )
                    }) {
                        self.settings.ui.mods_window_size = [size.width, size.height];
                    }
                    if let Some(window) = gfx.mods_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::mods_window)
                    .is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_position(window, position)
                    })
                {
                    self.settings.ui.mods_window_position = Some([position.x, position.y]);
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_mods_frame();
                self.last_mods_render = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.mods_window_focused = focused;
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now().duration_since(self.last_mods_render)
                    >= std::time::Duration::from_millis(16) =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::mods_window)
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
    fn handle_cheats_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.cheats_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
                self.persist_cheats_window_geometry();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.close_cheats_window();
                }
                self.show_cheats_window = false;
                self.settings.save();
                self.cheats_window_focused = false;
                self.focus_cheats_window_pending = false;
                self.focus_state_dirty = true;
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize_cheats_window(size.width, size.height);
                    if gfx.cheats_window().is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_size(
                            window,
                            size,
                            crate::graphics::window_geometry::CHEATS_MIN_SIZE,
                        )
                    }) {
                        self.settings.ui.cheats_window_size = [size.width, size.height];
                    }
                    if let Some(window) = gfx.cheats_window() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::cheats_window)
                    .is_some_and(|window| {
                        crate::graphics::window_geometry::can_persist_position(window, position)
                    })
                {
                    self.settings.ui.cheats_window_position = Some([position.x, position.y]);
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_cheats_frame();
                self.last_cheats_render = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.cheats_window_focused = focused;
                self.focus_state_dirty = true;
            }
            _ if needs_repaint
                && Instant::now().duration_since(self.last_cheats_render)
                    >= std::time::Duration::from_millis(16) =>
            {
                if let Some(window) = self
                    .gfx
                    .as_ref()
                    .and_then(crate::graphics::Graphics::cheats_window)
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn persist_printer_window_geometry(&mut self) {
        let Some(window) = self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::printer_window)
        else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        self.settings.ui.printer_window_maximized = window.is_maximized();
        let size = window.inner_size();
        if crate::graphics::window_geometry::can_persist_size(
            window,
            size,
            crate::graphics::window_geometry::PRINTER_MIN_SIZE,
        ) {
            self.settings.ui.printer_window_size = [size.width, size.height];
            if let Ok(position) = window.outer_position()
                && crate::graphics::window_geometry::can_persist_position(window, position)
            {
                self.settings.ui.printer_window_position = Some([position.x, position.y]);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn persist_mods_window_geometry(&mut self) {
        let Some(window) = self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::mods_window)
        else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        self.settings.ui.mods_window_maximized = window.is_maximized();
        let size = window.inner_size();
        if crate::graphics::window_geometry::can_persist_size(
            window,
            size,
            crate::graphics::window_geometry::MODS_MIN_SIZE,
        ) {
            self.settings.ui.mods_window_size = [size.width, size.height];
            if let Ok(position) = window.outer_position()
                && crate::graphics::window_geometry::can_persist_position(window, position)
            {
                self.settings.ui.mods_window_position = Some([position.x, position.y]);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn persist_cheats_window_geometry(&mut self) {
        let Some(window) = self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::cheats_window)
        else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        self.settings.ui.cheats_window_maximized = window.is_maximized();
        let size = window.inner_size();
        if crate::graphics::window_geometry::can_persist_size(
            window,
            size,
            crate::graphics::window_geometry::CHEATS_MIN_SIZE,
        ) {
            self.settings.ui.cheats_window_size = [size.width, size.height];
            if let Ok(position) = window.outer_position()
                && crate::graphics::window_geometry::can_persist_position(window, position)
            {
                self.settings.ui.cheats_window_position = Some([position.x, position.y]);
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
                let next = (position.x as f32, position.y as f32);
                if self.active_system == crate::emu_backend::ActiveSystem::Pce
                    && !self.pce_mouse_captured
                    && self.game_view_focused
                    && let Some(previous) = self.cursor_pos
                    && let Some(gfx) = self.gfx.as_ref()
                    && let (Some(_), Some(_)) = (
                        gfx.game_pixel_at_window_pos(previous.0, previous.1),
                        gfx.game_pixel_at_window_pos(next.0, next.1),
                    )
                {
                    self.pce_mouse_motion.0 += f64::from(next.0 - previous.0);
                    self.pce_mouse_motion.1 += f64::from(next.1 - previous.1);
                }
                self.cursor_pos = Some(next);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
                if !self.pce_mouse_captured {
                    self.mouse_left_pressed = false;
                    self.mouse_right_pressed = false;
                }
            }
            WindowEvent::Resized(size) => {
                self.window_size = (size.width as f32, size.height as f32);
            }
            _ => {}
        }
    }

    fn handle_mouse_input_for_zapper(&mut self, event: &WindowEvent, event_consumed_by_egui: bool) {
        let WindowEvent::MouseInput { state, button, .. } = event else {
            return;
        };

        if !matches!(button, MouseButton::Left | MouseButton::Right) {
            return;
        }

        match state {
            ElementState::Pressed => {
                let pointer_over_direct_game = self.pointer_over_direct_game_view();
                let captured = self.pce_mouse_captured;
                #[cfg(not(target_arch = "wasm32"))]
                if *button == MouseButton::Left && pointer_over_direct_game {
                    self.capture_pce_mouse();
                }
                let pressed = game_mouse_press_reaches_emulator(
                    self.game_view_focused,
                    captured,
                    pointer_over_direct_game,
                    event_consumed_by_egui,
                );
                match button {
                    MouseButton::Left => self.mouse_left_pressed = pressed,
                    MouseButton::Right => self.mouse_right_pressed = pressed,
                    _ => unreachable!(),
                }
            }
            ElementState::Released => match button {
                MouseButton::Left => self.mouse_left_pressed = false,
                MouseButton::Right => self.mouse_right_pressed = false,
                _ => unreachable!(),
            },
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
        #[cfg(not(target_arch = "wasm32"))]
        if !focused {
            self.release_pce_mouse(false);
        }
        self.game_window_focused = focused;
        self.focus_state_dirty = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_device_event(&mut self, event: DeviceEvent) {
        if !self.pce_mouse_captured
            || !self.game_window_focused
            || self.active_system != crate::emu_backend::ActiveSystem::Pce
        {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.pce_mouse_motion.0 += delta.0;
            self.pce_mouse_motion.1 += delta.1;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn capture_pce_mouse(&mut self) {
        if self.pce_mouse_captured
            || !pce_mouse_capture_allowed(
                self.active_system,
                self.settings.emulation.pce_controller,
                self.rom_info.pce_controller_profile_hash,
            )
            || self.settings.emulation.pce_mouse_cursor_mode
                != crate::settings::PceMouseCursorMode::Captured
        {
            return;
        }
        let Some(window) = self.gfx.as_ref().map(crate::graphics::Graphics::window) else {
            return;
        };
        let grabbed = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        if let Err(error) = grabbed {
            log::warn!("Failed to capture the mouse cursor: {error}");
            self.toast_manager
                .error("Couldn't capture the mouse cursor");
            return;
        }
        window.set_cursor_visible(false);
        self.pce_mouse_captured = true;
        self.cursor_pos = None;
        self.pce_mouse_motion = (0.0, 0.0);
        self.toast_manager
            .info("Mouse captured. Press Escape or Alt to release");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn release_pce_mouse(&mut self, notify: bool) {
        if !self.pce_mouse_captured {
            return;
        }
        if let Some(window) = self.gfx.as_ref().map(crate::graphics::Graphics::window) {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.pce_mouse_captured = false;
        self.pce_mouse_motion = (0.0, 0.0);
        self.mouse_left_pressed = false;
        self.mouse_right_pressed = false;
        if notify {
            self.toast_manager.info("Mouse released");
        }
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
        #[cfg(not(target_arch = "wasm32"))]
        let mods_window_focused = self.mods_window_focused;
        #[cfg(target_arch = "wasm32")]
        let mods_window_focused = false;
        #[cfg(not(target_arch = "wasm32"))]
        let cheats_window_focused = self.cheats_window_focused;
        #[cfg(target_arch = "wasm32")]
        let cheats_window_focused = false;
        #[cfg(not(target_arch = "wasm32"))]
        let printer_window_focused = self.printer_window_focused;
        #[cfg(target_arch = "wasm32")]
        let printer_window_focused = false;
        let focused = any_app_window_focused(
            self.game_window_focused,
            debugger_window_focused,
            settings_window_focused,
            mods_window_focused,
            cheats_window_focused,
            printer_window_focused,
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
    mods_window_focused: bool,
    cheats_window_focused: bool,
    printer_window_focused: bool,
) -> bool {
    game_window_focused
        || debugger_window_focused
        || settings_window_focused
        || mods_window_focused
        || cheats_window_focused
        || printer_window_focused
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

fn game_mouse_press_reaches_emulator(
    game_view_focused: bool,
    pce_mouse_captured: bool,
    pointer_over_direct_game: bool,
    event_consumed_by_egui: bool,
) -> bool {
    game_view_focused && (pce_mouse_captured || pointer_over_direct_game || !event_consumed_by_egui)
}

pub(super) fn pce_mouse_capture_allowed(
    active_system: crate::emu_backend::ActiveSystem,
    preference: crate::settings::PceControllerPreference,
    content_hash: Option<[u8; 32]>,
) -> bool {
    if active_system != crate::emu_backend::ActiveSystem::Pce {
        return false;
    }
    match preference {
        crate::settings::PceControllerPreference::Mouse => true,
        crate::settings::PceControllerPreference::Auto => content_hash.is_some_and(|hash| {
            crate::emu_backend::pce_profiles::automatic_controller_mode(hash)
                == zeff_pce_core::hardware::PceControllerMode::Mouse
        }),
        crate::settings::PceControllerPreference::TwoButton
        | crate::settings::PceControllerPreference::SixButton
        | crate::settings::PceControllerPreference::Multitap => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        any_app_window_focused, game_mouse_press_reaches_emulator, pce_mouse_capture_allowed,
        should_pause_on_unfocus,
    };

    #[test]
    fn any_native_window_keeps_the_app_focused() {
        assert!(any_app_window_focused(
            true, false, false, false, false, false
        ));
        assert!(any_app_window_focused(
            false, true, false, false, false, false
        ));
        assert!(any_app_window_focused(
            false, false, true, false, false, false
        ));
        assert!(any_app_window_focused(
            false, false, false, true, false, false
        ));
        assert!(any_app_window_focused(
            false, false, false, false, true, false
        ));
        assert!(any_app_window_focused(
            false, false, false, false, false, true
        ));
        assert!(!any_app_window_focused(
            false, false, false, false, false, false
        ));
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

    #[test]
    fn captured_mouse_clicks_reach_the_emulator_without_a_cursor_position() {
        assert!(game_mouse_press_reaches_emulator(true, true, false, true));
        assert!(!game_mouse_press_reaches_emulator(false, true, false, true));
    }

    #[test]
    fn uncaptured_ui_clicks_do_not_leak_into_the_emulator() {
        assert!(!game_mouse_press_reaches_emulator(true, false, false, true));
        assert!(game_mouse_press_reaches_emulator(true, false, true, true));
    }

    #[test]
    fn pce_mouse_capture_requires_a_mouse_profile_or_explicit_override() {
        use crate::emu_backend::ActiveSystem;
        use crate::settings::PceControllerPreference;

        assert!(pce_mouse_capture_allowed(
            ActiveSystem::Pce,
            PceControllerPreference::Auto,
            Some(crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256),
        ));
        assert!(!pce_mouse_capture_allowed(
            ActiveSystem::Pce,
            PceControllerPreference::Auto,
            Some([0; 32]),
        ));
        assert!(pce_mouse_capture_allowed(
            ActiveSystem::Pce,
            PceControllerPreference::Mouse,
            Some([0; 32]),
        ));
        assert!(!pce_mouse_capture_allowed(
            ActiveSystem::Pce,
            PceControllerPreference::Multitap,
            Some(crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256),
        ));
        assert!(!pce_mouse_capture_allowed(
            ActiveSystem::Pce,
            PceControllerPreference::Auto,
            Some(
                crate::emu_backend::pce_profiles::TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256,
            ),
        ));
        assert!(!pce_mouse_capture_allowed(
            ActiveSystem::Nes,
            PceControllerPreference::Mouse,
            None,
        ));
    }
}
