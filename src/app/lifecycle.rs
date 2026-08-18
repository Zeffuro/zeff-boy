use super::{App, SpeedMode, UI_RENDER_INTERVAL};
use crate::{
    audio::AudioOutput,
    emu_thread::{EmuCommand, EmuThread},
    graphics::Graphics,
    platform::Instant,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Fullscreen;

impl App {
    pub(super) fn reset_audio_output(&mut self) {
        if std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1") {
            self.audio = None;
            return;
        }
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

        #[cfg(target_arch = "wasm32")]
        if self.pending_gfx.is_some() {
            return;
        }

        if self.audio.is_none() {
            self.reset_audio_output();
        }

        self.ensure_emu_thread();

        if let (Some(audio), Some(thread)) = (self.audio.as_ref(), &self.emu_thread) {
            thread.send(EmuCommand::SetSampleRate(audio.sample_rate()));
        }

        let window = match Graphics::create_window(event_loop) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create window: {e}");
                #[cfg(target_arch = "wasm32")]
                crate::platform::show_boot_error(
                    "Failed to create the emulator window.",
                    "The browser did not allow zeff-boy to create its rendering canvas.",
                    &e.to_string(),
                );
                return;
            }
        };

        let size = window.inner_size();
        self.window_size = (size.width as f32, size.height as f32);
        self.window_id = Some(window.id());

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = window.canvas() {
                let pending_rom_load = self.pending_rom_load.clone();
                let visible_flag = self.wasm_tab_visible.clone();

                let setup = wasm_bindgen::closure::Closure::once_into_js(move || {
                    let web_window = web_sys::window().expect("browser window must exist");
                    let document = web_window.document().expect("document must exist");
                    let body = document.body().expect("document body must exist");
                    let _ = body.append_child(&canvas);
                    canvas.set_attribute("style", "width:100%;height:100%").ok();

                    let target: &web_sys::EventTarget = body.unchecked_ref();
                    crate::platform::setup_drop_handler(target, pending_rom_load);

                    let doc_clone = document.clone();
                    let vis_cb = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                        let hidden = doc_clone.hidden();
                        visible_flag.set(!hidden);
                    })
                        as Box<dyn Fn()>);
                    let _ = document.add_event_listener_with_callback(
                        "visibilitychange",
                        vis_cb.as_ref().unchecked_ref(),
                    );
                    vis_cb.forget();
                });

                let _ = web_sys::window()
                    .expect("browser window must exist")
                    .set_timeout_with_callback(setup.unchecked_ref());
            }
        }

        let vsync = self.settings.video.vsync_mode;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let gfx = pollster::block_on(Graphics::new(window, vsync)) // platform-ok
                .expect("failed to initialize graphics");
            self.finalize_gfx_init(gfx);
            self.sync_debug_presentation(event_loop);
        }

        #[cfg(target_arch = "wasm32")]
        {
            crate::platform::set_boot_status(
                "Starting graphics…",
                "Creating the WebGPU surface and render pipeline.",
            );
            let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
            let slot_clone = slot.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = Graphics::new(window, vsync).await;
                *slot_clone.borrow_mut() = Some(result);
            });
            self.pending_gfx = Some(slot);
        }
    }

    fn finalize_gfx_init(&mut self, gfx: Graphics) {
        let size = gfx.window().inner_size();
        self.window_size = (size.width as f32, size.height as f32);

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

        self.gfx = Some(gfx);
        let (native_w, native_h) = self.active_display_size();
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
        }

        #[cfg(target_arch = "wasm32")]
        crate::platform::hide_boot_screen();
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn check_pending_gfx(&mut self) {
        if self.gfx.is_some() {
            return;
        }
        let slot = match self.pending_gfx.take() {
            Some(s) => s,
            None => return,
        };
        if let Some(result) = slot.borrow_mut().take() {
            match result {
                Ok(gfx) => {
                    self.finalize_gfx_init(gfx);
                    if let Some(gfx) = self.gfx.as_mut() {
                        let size = gfx.window().inner_size();
                        if size.width > 0 && size.height > 0 {
                            gfx.resize(size.width, size.height);
                        }
                        gfx.window().request_redraw();
                    }
                }
                Err(e) => {
                    log::error!("Graphics initialization failed: {e}");
                    crate::platform::show_boot_error(
                        "Graphics initialization failed.",
                        "zeff-boy could not start WebGPU rendering. This usually means browser hardware acceleration is disabled, the GPU is blocked by the browser, or no compatible adapter is available.",
                        &e.to_string(),
                    );
                }
            }
        } else {
            self.pending_gfx = Some(slot);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn check_tab_visibility(&mut self) {
        let visible = self.wasm_tab_visible.get();
        if visible != self.wasm_tab_was_visible {
            self.wasm_tab_was_visible = visible;
            self.handle_focus_change(visible);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn wasm_poll_hooks(&mut self, _event_loop: &ActiveEventLoop) {}

    #[cfg(target_arch = "wasm32")]
    pub(super) fn wasm_poll_hooks(&mut self, event_loop: &ActiveEventLoop) {
        self.check_pending_gfx();
        self.check_pending_rom();
        self.check_pending_state_load();
        self.check_pending_nes_palette_load();
        self.check_tab_visibility();
        if self.gfx.is_none() && self.pending_gfx.is_some() {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        }
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
            SpeedMode::Normal | SpeedMode::SlowMotion => {
                // On WASM, Normal mode uses requestAnimationFrame (via request_redraw)
                // instead of setTimeout (WaitUntil). rAF is vsync-aligned and jitter-free,
                // while setTimeout has ≥4ms granularity that causes visible hitches.
                #[cfg(target_arch = "wasm32")]
                {
                    event_loop.set_control_flow(ControlFlow::Wait);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let effective = self.effective_frame_duration();
                    let now = Instant::now();
                    let next_emu_frame_time = self.timing.last_frame_time + effective;
                    let next_ui_frame_time = self.timing.last_render_time + UI_RENDER_INTERVAL;
                    let next_frame_time = next_emu_frame_time.max(next_ui_frame_time);
                    if now >= next_frame_time {
                        event_loop.set_control_flow(ControlFlow::Poll);
                    } else {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
                    }
                }
                gfx.window().request_redraw();
            }
            SpeedMode::FastForward | SpeedMode::Uncapped => {
                event_loop.set_control_flow(ControlFlow::Poll);
                gfx.window().request_redraw();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_debug_presentation(&mut self, event_loop: &ActiveEventLoop) {
        let desired = super::effective_debug_presentation(self.settings.ui.debug_presentation);
        if self.activate_debug_presentation(desired) {
            if desired == crate::settings::DebugPresentation::GameAndDebugger {
                self.settings.ui.debugger_window_open = true;
            }
            self.settings.save();
        }

        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if desired == crate::settings::DebugPresentation::GameAndDebugger
            && self.settings.ui.debugger_window_open
        {
            if gfx.debugger_window_id().is_none()
                && let Err(err) = gfx.open_debugger_window(event_loop, &self.settings)
            {
                log::error!("Failed to open debugger window: {err}");
                self.active_debug_presentation = crate::settings::DebugPresentation::Floating;
                self.settings.ui.debug_presentation = crate::settings::DebugPresentation::Floating;
                self.settings.ui.debugger_window_open = false;
                self.debug_dock = crate::debug::create_default_dock_state();
                self.settings.save();
                self.toast_manager.error("Failed to open debugger window");
            }
        } else if gfx.debugger_window_id().is_some() {
            gfx.close_debugger_window();
            self.debugger_window_focused = false;
            self.focus_state_dirty = true;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_settings_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if self.show_settings_window {
            if gfx.settings_window_id().is_none()
                && let Err(err) = gfx.open_settings_window(event_loop, &self.settings)
            {
                log::error!("Failed to open settings window: {err}");
                self.show_settings_window = false;
                self.toast_manager.error("Failed to open settings window");
            }
        } else if gfx.settings_window_id().is_some() {
            gfx.close_settings_window();
            self.settings_window_focused = false;
            self.focus_state_dirty = true;
        }
    }
}
