use super::{App, SpeedMode, UI_RENDER_INTERVAL};
use crate::audio::DEFAULT_AUDIO_SAMPLE_RATE;
use crate::{
    audio::AudioOutput,
    emu_thread::{EmuCommand, TasControlCommandKind},
    graphics::Graphics,
    platform::Instant,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Fullscreen;

pub(super) fn effective_debug_presentation(
    presentation: crate::settings::DebugPresentation,
) -> crate::settings::DebugPresentation {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::live_control::automation_mode_enabled() {
        return crate::settings::DebugPresentation::Floating;
    }

    #[cfg(target_arch = "wasm32")]
    if presentation == crate::settings::DebugPresentation::GameAndDebugger {
        return crate::settings::DebugPresentation::Floating;
    }

    presentation
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_focus_and_redraw(window: &winit::window::Window) {
    window.set_minimized(false);
    window.focus_window();
    window.request_redraw();
}

impl App {
    pub(super) fn reset_audio_output(&mut self) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::AudioOrTimingConfiguration)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        let audio = if std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1") {
            None
        } else {
            let preferred = self.settings.audio.output_sample_rate;
            AudioOutput::new(Some(preferred))
                .map_err(|e| log::warn!("Audio init failed: {e}"))
                .ok()
        };
        let sample_rate = audio
            .as_ref()
            .map_or(DEFAULT_AUDIO_SAMPLE_RATE, AudioOutput::emulator_sample_rate);
        if self.emu_thread.is_some()
            && let Err(error) =
                self.send_emu_command_checked(EmuCommand::SetSampleRate(sample_rate))
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        self.audio = audio;
    }

    pub(super) fn ensure_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        if let Some(backend) = self.initial_backend.take() {
            self.spawn_emu_thread(backend);
            self.inspect_recovery_after_normal_open();
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

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.deferred_initial_rom_load.take() {
            self.load_rom(&path);
        }

        if self.emu_thread.is_some() {
            let sample_rate = self
                .audio
                .as_ref()
                .map_or(DEFAULT_AUDIO_SAMPLE_RATE, AudioOutput::emulator_sample_rate);
            if let Err(error) =
                self.send_emu_command_checked(EmuCommand::SetSampleRate(sample_rate))
            {
                log::warn!("Could not synchronize emulator sample rate: {error}");
            }
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
                let persistence_wake = self.wasm_event_loop_proxy.clone();

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
                        let _ = persistence_wake.send_event(());
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
            if !visible && let Some(thread) = &self.emu_thread {
                thread.send(crate::emu_thread::EmuCommand::FlushBatterySram);
            }
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
        #[cfg(all(test, feature = "wasm-browser-tests"))]
        super::browser_speculation_test::drive_app(self, event_loop);
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
                // WASM uses vsync-aligned rAF because timer pacing visibly hitches.
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
                    let mut next_frame_time = next_emu_frame_time.max(next_ui_frame_time);
                    if let Some(recording_wake) = self.realtime_tas_recording_next_wake(now) {
                        next_frame_time = next_frame_time.min(recording_wake);
                    }
                    if let Some(playback_wake) = self.linked_tas_playback_next_wake(now) {
                        next_frame_time = next_frame_time.min(playback_wake);
                    }
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
        let desired = effective_debug_presentation(self.settings.ui.debug_presentation);
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
                self.focus_settings_window_pending = false;
                self.toast_manager.error("Failed to open settings window");
            }
            if self.focus_settings_window_pending
                && let Some(window) = gfx.settings_window()
            {
                restore_focus_and_redraw(window);
                self.focus_settings_window_pending = false;
            }
        } else if gfx.settings_window_id().is_some() {
            gfx.close_settings_window();
            self.settings_window_focused = false;
            self.focus_settings_window_pending = false;
            self.focus_state_dirty = true;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_mods_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if self.show_mods_window {
            if gfx.mods_window_id().is_none()
                && let Err(err) = gfx.open_mods_window(event_loop, &self.settings)
            {
                log::error!("Failed to open Mods window: {err}");
                self.show_mods_window = false;
                self.focus_mods_window_pending = false;
                self.toast_manager.error("Failed to open Mods window");
            }
            if self.focus_mods_window_pending
                && let Some(window) = gfx.mods_window()
            {
                restore_focus_and_redraw(window);
                self.focus_mods_window_pending = false;
            }
        } else if gfx.mods_window_id().is_some() {
            gfx.close_mods_window();
            self.mods_window_focused = false;
            self.focus_mods_window_pending = false;
            self.focus_state_dirty = true;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_cheats_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if self.show_cheats_window {
            if gfx.cheats_window_id().is_none()
                && let Err(err) = gfx.open_cheats_window(event_loop, &self.settings)
            {
                log::error!("Failed to open Cheats window: {err}");
                self.show_cheats_window = false;
                self.focus_cheats_window_pending = false;
                self.toast_manager.error("Failed to open Cheats window");
            }
            if self.focus_cheats_window_pending
                && let Some(window) = gfx.cheats_window()
            {
                restore_focus_and_redraw(window);
                self.focus_cheats_window_pending = false;
            }
        } else if gfx.cheats_window_id().is_some() {
            gfx.close_cheats_window();
            self.cheats_window_focused = false;
            self.focus_cheats_window_pending = false;
            self.focus_state_dirty = true;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_printer_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        if self.show_printer_window {
            if gfx.printer_window_id().is_none()
                && let Err(err) = gfx.open_printer_window(event_loop, &self.settings)
            {
                log::error!("Failed to open Game Boy Printer window: {err}");
                self.show_printer_window = false;
                self.focus_printer_window_pending = false;
                self.toast_manager
                    .error("Failed to open Game Boy Printer window");
            }
            if self.focus_printer_window_pending
                && let Some(window) = gfx.printer_window()
            {
                restore_focus_and_redraw(window);
                self.focus_printer_window_pending = false;
            }
        } else if gfx.printer_window_id().is_some() {
            gfx.close_printer_window();
            self.debug_windows.printer.clear_textures();
            self.printer_window_focused = false;
            self.focus_printer_window_pending = false;
            self.focus_state_dirty = true;
        }
    }
}
