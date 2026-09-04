use super::App;
use crate::debug::{DebugDataRefs, MenuAction};
use crate::graphics;

mod debug_actions;

#[cfg(not(target_arch = "wasm32"))]
fn estimate_load_eta(
    elapsed: std::time::Duration,
    completed_bytes: u64,
    total_bytes: u64,
) -> Option<String> {
    if elapsed < std::time::Duration::from_secs(1)
        || completed_bytes == 0
        || completed_bytes >= total_bytes
    {
        return None;
    }
    let seconds =
        elapsed.as_secs_f64() * (total_bytes - completed_bytes) as f64 / completed_bytes as f64;
    let seconds = seconds.ceil().min(24.0 * 60.0 * 60.0) as u64;
    let text = if seconds < 60 {
        format!("About {seconds}s remaining")
    } else if seconds < 60 * 60 {
        format!("About {}m {}s remaining", seconds / 60, seconds % 60)
    } else {
        format!("About {}h {}m remaining", seconds / 3600, seconds / 60 % 60)
    };
    Some(text)
}

impl App {
    pub(super) fn render_frame(&mut self, ui_frame_data: Option<&crate::ui::UiFrameData>) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(result) = self.update_checker.poll() {
            match result {
                crate::update::UpdatePoll::Available => {
                    self.toast_manager
                        .info("A new zeff-boy version is available");
                }
                crate::update::UpdatePoll::Current => {
                    self.toast_manager.success("zeff-boy is up to date");
                }
                crate::update::UpdatePoll::CheckFailed(err) => {
                    self.toast_manager
                        .error(format!("Update check failed: {err}"));
                }
                crate::update::UpdatePoll::InstallReady => {
                    self.toast_manager.success("Update ready to install");
                }
                crate::update::UpdatePoll::InstallFailed(err) => {
                    self.toast_manager
                        .error(format!("Update install failed: {err}"));
                }
            }
        }
        let supports_save_states = self.core_supports_save_states();
        let supports_replay = self.core_supports_replay();
        let supports_audio = self.core_supports_audio();
        let supports_rewind = self.core_supports_rewind();
        let supports_debugger = self.core_supports_debugger();
        let supports_execution_controls = self.core_supports_execution_controls();
        #[cfg(not(target_arch = "wasm32"))]
        let package_load = self.pending_rom_preparation.as_ref().map(|pending| {
            let completed_bytes = pending.progress.completed_bytes();
            let total_bytes = pending.progress.total_bytes();
            let elapsed = pending.started_at.elapsed();
            let eta = estimate_load_eta(elapsed, completed_bytes, total_bytes);
            let phase = match pending.progress.phase() {
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::Inspecting => {
                    "Inspecting archive"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::ReadingCue => {
                    "Reading CUE and validating package"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::ReadingData => {
                    "Reading disc data"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::ReadingRom => {
                    "Extracting ROM"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::Firmware => {
                    "Resolving System Card"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::Building => {
                    "Building emulator"
                }
                crate::emu_backend::pce_cd_archive::PceCdPackageLoadPhase::Complete => "Finishing",
            };
            crate::graphics::PackageLoadView {
                filename: pending
                    .source_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("archive")
                    .to_owned(),
                phase,
                completed_bytes,
                total_bytes,
                eta,
            }
        });
        let nominal_frame_duration_ns = self.nominal_frame_duration_ns();
        #[cfg(not(target_arch = "wasm32"))]
        self.refresh_tas_editor_live_status();
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };

        #[cfg(not(target_arch = "wasm32"))]
        let update_info = self.update_checker.available().cloned();
        #[cfg(not(target_arch = "wasm32"))]
        let show_update_dialog = self.update_checker.show_dialog();
        #[cfg(not(target_arch = "wasm32"))]
        let update_install_state = self.update_checker.install_state();

        let settings_was_open = self.show_settings_window;

        let speed_label = ui_frame_data
            .and_then(|d| d.perf_info.as_ref())
            .map(|info| info.speed_mode_label);

        let is_recording = self.recording.is_audio_recording();
        let is_recording_replay =
            self.recording.replay_recorder.is_some() || self.recording.is_replay_start_pending();
        let is_playing_replay = self.recording.replay_player.is_some();
        let media_event_change_allowed = self.emu_thread.is_some()
            && !is_playing_replay
            && self.recording.pending_media_commands.is_empty()
            && (self.recording.replay_recorder.is_some() || !self.recording.is_replay_active());
        #[cfg(not(target_arch = "wasm32"))]
        let game_boy_serial_device_change_allowed = self.emu_thread.is_some()
            && !self.recording.is_replay_active()
            && !self.tcp_link_active;
        #[cfg(target_arch = "wasm32")]
        let game_boy_serial_device_change_allowed =
            self.emu_thread.is_some() && !self.recording.is_replay_active();
        let is_rewinding = self.rewind.held
            && supports_rewind
            && self.settings.rewind.enabled
            && !self.recording.is_replay_active();
        let autohide_menu_bar = self.settings.ui.autohide_menu_bar;
        let cursor_y = self.cursor_pos.map(|(_, y)| y);
        let rewind_seconds_back = (self.rewind.frames_rewound as f64
            * nominal_frame_duration_ns as f64
            / 1_000_000_000.0) as f32;
        let slot_labels = &self.cached_slot_info.labels;
        let slot_occupied = self.cached_slot_info.occupied;
        let debugger_window_open = self.settings.ui.debugger_window_open;
        #[cfg(not(target_arch = "wasm32"))]
        let mut focus_debugger_window = false;

        match gfx.render(graphics::RenderContext {
            data: debug_data_refs(ui_frame_data, &self.symbols),
            active_system: Some(self.active_system),
            media_slot_snapshot: self.media_slot_snapshot.as_ref(),
            media_event_change_allowed,
            game_boy_serial_device: self.game_boy_serial_device,
            game_boy_serial_device_change_allowed,
            debug_windows: &mut self.debug_windows,
            settings: &mut self.settings,
            #[cfg(target_arch = "wasm32")]
            nes_palette_file_slot: self.pending_nes_palette_load.clone(),
            show_settings_window: &mut self.show_settings_window,
            #[cfg(not(target_arch = "wasm32"))]
            show_mods_window: &mut self.show_mods_window,
            #[cfg(not(target_arch = "wasm32"))]
            show_cheats_window: &mut self.show_cheats_window,
            #[cfg(target_arch = "wasm32")]
            show_printer_window: &mut self.show_printer_window,
            dock_state: &mut self.debug_dock,
            toast_manager: &mut self.toast_manager,
            speed_mode_label: speed_label,
            is_recording_audio: is_recording,
            is_recording_replay,
            is_playing_replay,
            supports_save_states,
            supports_rewind,
            supports_replay,
            supports_audio,
            supports_debugger,
            supports_execution_controls,
            is_rewinding,
            rewind_seconds_back,
            is_paused: self.speed.paused,
            ws_display_rotated: self.ws_display_rotated,
            #[cfg(target_arch = "wasm32")]
            is_pocket_camera: self.rom_info.is_pocket_camera,
            autohide_menu_bar,
            cursor_y,
            slot_labels,
            slot_occupied,
            active_save_slot: self.active_save_slot,
            can_undo_load_state: self.undo_load_state.is_some(),
            can_undo_save_state: self.undo_save_state_path.is_some(),
            recovery_state_available: self.recovery_state_available,
            archive_selection: self.pending_archive_selection.as_ref(),
            #[cfg(not(target_arch = "wasm32"))]
            package_load,
            show_debug_dock: self.active_debug_presentation
                != crate::settings::DebugPresentation::GameAndDebugger,
            debugger_window_open,
            debug_presentation: self.active_debug_presentation,
            #[cfg(not(target_arch = "wasm32"))]
            update_info: update_info.as_ref(),
            #[cfg(not(target_arch = "wasm32"))]
            show_update_dialog,
            #[cfg(not(target_arch = "wasm32"))]
            update_install_state,
        }) {
            Ok(result) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(request) = result.tas_editor_host_request {
                    self.handle_tas_editor_host_request(request);
                }
                let mut settings_dirty = false;
                for action in &result.actions {
                    match action {
                        MenuAction::OpenFile => self.open_file_dialog(),
                        MenuAction::LoadSymbolFile => {
                            #[cfg(not(target_arch = "wasm32"))]
                            if supports_debugger {
                                self.open_symbol_file_dialog();
                            }
                            #[cfg(target_arch = "wasm32")]
                            if supports_debugger {
                                self.toast_manager.error("Symbol files are native-only");
                            }
                        }
                        MenuAction::ResetGame => self.reset_game(),
                        #[cfg(not(target_arch = "wasm32"))]
                        MenuAction::ReloadGame => self.reload_game(),
                        MenuAction::StopGame => self.stop_game(),
                        MenuAction::SaveStateFile => self.save_state_file_dialog(),
                        MenuAction::LoadStateFile => self.load_state_file_dialog(),
                        MenuAction::LoadRecoveryState => self.load_available_recovery(),
                        MenuAction::UndoLoadState => self.undo_load_state(),
                        MenuAction::UndoSaveState => self.undo_save_state(),
                        MenuAction::SaveStateSlot(slot) => self.save_state_slot(*slot),
                        MenuAction::LoadStateSlot(slot) => self.load_state_slot(*slot),
                        MenuAction::LoadRecentRom(path) => self.load_rom(path),
                        MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
                        MenuAction::TogglePause => {
                            self.toggle_user_paused();
                        }
                        MenuAction::SpeedChange(delta) => {
                            let mult =
                                self.settings.emulation.fast_forward_multiplier as i32 + delta;
                            self.settings.emulation.fast_forward_multiplier =
                                mult.clamp(1, 16) as usize;
                            settings_dirty = true;
                        }
                        MenuAction::StartAudioRecording => {
                            self.start_audio_recording();
                        }
                        MenuAction::StopAudioRecording => {
                            self.stop_audio_recording();
                        }
                        MenuAction::StartReplayRecording => self.start_replay_recording(),
                        MenuAction::StopReplayRecording => {
                            if let Err(error) = self.stop_replay_recording() {
                                self.toast_manager.error(error.to_string());
                            }
                        }
                        MenuAction::LoadReplay => self.load_and_play_replay(),
                        MenuAction::TakeScreenshot => self.take_screenshot(),
                        MenuAction::ApplyMediaEvent(event) => {
                            self.request_media_event(event.clone())
                        }
                        MenuAction::SetGameBoySerialDevice(device) => {
                            self.set_game_boy_serial_device(*device);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        MenuAction::ScanBardigunBarcodeFile => {
                            self.scan_bardigun_barcode_file_dialog();
                        }
                        MenuAction::OpenBarcodeBoyScan => {
                            self.open_barcode_boy_scan();
                        }
                        MenuAction::TriggerBarcodeBoyScan(digits) => {
                            self.trigger_barcode_boy_scan(digits.clone());
                        }
                        MenuAction::HostTcpLink => {
                            #[cfg(not(target_arch = "wasm32"))]
                            if let Err(error) = self.host_tcp_link(None) {
                                self.toast_manager.error(error);
                            }
                            #[cfg(target_arch = "wasm32")]
                            self.toast_manager.error("TCP link is native-only");
                        }
                        MenuAction::JoinTcpLink => {
                            #[cfg(not(target_arch = "wasm32"))]
                            if let Err(error) = self.join_tcp_link(None) {
                                self.toast_manager.error(error);
                            }
                            #[cfg(target_arch = "wasm32")]
                            self.toast_manager.error("TCP link is native-only");
                        }
                        MenuAction::DisconnectLink => {
                            #[cfg(not(target_arch = "wasm32"))]
                            if let Err(error) = self.disconnect_link() {
                                self.toast_manager.error(error);
                            }
                            #[cfg(target_arch = "wasm32")]
                            self.toast_manager.info("No TCP link active");
                        }
                        MenuAction::ToggleWsRotation => self.toggle_ws_rotation(),
                        MenuAction::ToolbarSettingsChanged => settings_dirty = true,
                        MenuAction::SetLayerToggles(bg, win, sprites) => {
                            match self.preflight_emu_command_kind(
                                crate::emu_thread::TasControlCommandKind::DebuggerMutation,
                            ) {
                                Ok(()) => {
                                    self.pending_debug_actions.layer_toggles =
                                        Some((*bg, *win, *sprites));
                                }
                                Err(error) => self.toast_manager.error(error.to_string()),
                            }
                        }
                        MenuAction::SetGbaBgLayerToggles(layers) => {
                            match self.preflight_emu_command_kind(
                                crate::emu_thread::TasControlCommandKind::DebuggerMutation,
                            ) {
                                Ok(()) => {
                                    self.pending_debug_actions.gba_bg_layer_toggles = Some(*layers);
                                }
                                Err(error) => self.toast_manager.error(error.to_string()),
                            }
                        }
                        MenuAction::OpenDebuggerWindow => {
                            self.settings.ui.debugger_window_open = true;
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                focus_debugger_window = true;
                            }
                            settings_dirty = true;
                        }
                        MenuAction::OpenPrinterWindow => {
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                request_native_tool_window(
                                    &mut self.show_printer_window,
                                    &mut self.focus_printer_window_pending,
                                );
                            }
                            #[cfg(target_arch = "wasm32")]
                            {
                                self.show_printer_window = true;
                            }
                        }
                        MenuAction::SetDebugPresentation(presentation) => {
                            self.settings.ui.debug_presentation = *presentation;
                            #[cfg(target_arch = "wasm32")]
                            {
                                let desired =
                                    super::lifecycle::effective_debug_presentation(*presentation);
                                self.settings.ui.debug_presentation = desired;
                                self.activate_debug_presentation(desired);
                            }
                            settings_dirty = true;
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        MenuAction::CheckForUpdates => self.update_checker.request(true),
                        MenuAction::SetAspectRatio(_) => {}
                        MenuAction::OpenSettings => {
                            #[cfg(not(target_arch = "wasm32"))]
                            request_native_tool_window(
                                &mut self.show_settings_window,
                                &mut self.focus_settings_window_pending,
                            );
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        MenuAction::OpenMods => request_native_tool_window(
                            &mut self.show_mods_window,
                            &mut self.focus_mods_window_pending,
                        ),
                        #[cfg(not(target_arch = "wasm32"))]
                        MenuAction::OpenCheats => request_native_tool_window(
                            &mut self.show_cheats_window,
                            &mut self.focus_cheats_window_pending,
                        ),
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(action) = result.update_action {
                    let info = self.update_checker.available().cloned();
                    match action {
                        crate::update::UpdateAction::Install => {
                            if let Err(err) = self.update_checker.install() {
                                self.toast_manager
                                    .error(format!("Can't install update: {err}"));
                            }
                        }
                        crate::update::UpdateAction::Restart => {
                            match self.update_checker.activate() {
                                Ok(()) => self.exit_requested = true,
                                Err(err) => self
                                    .toast_manager
                                    .error(format!("Can't activate update: {err}")),
                            }
                        }
                        crate::update::UpdateAction::Download => {
                            if let Some(info) = info.as_ref() {
                                crate::platform::open_url(&info.download_url);
                            }
                            self.update_checker.dismiss();
                        }
                        crate::update::UpdateAction::ReleaseNotes => {
                            if let Some(info) = info.as_ref() {
                                crate::platform::open_url(&info.release_url);
                            }
                            self.update_checker.dismiss();
                        }
                        crate::update::UpdateAction::Later => self.update_checker.dismiss(),
                        crate::update::UpdateAction::SkipVersion => {
                            if let Some(version) = self.update_checker.skip_version() {
                                self.settings.ui.skipped_update_version = Some(version);
                                self.settings.save();
                            }
                        }
                    }
                }
                if let Some(action) = result.archive_selection_action {
                    match action {
                        crate::rom_archive::ArchiveSelectionAction::Load {
                            archive_path,
                            entry_index,
                        } => {
                            self.pending_archive_selection = None;
                            self.load_archive_entry_with_options(&archive_path, entry_index, true);
                        }
                        crate::rom_archive::ArchiveSelectionAction::Cancel => {
                            self.pending_archive_selection = None;
                            self.toast_manager.info("Archive load canceled");
                        }
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                if result.cancel_package_load {
                    self.cancel_pending_rom_preparation(true);
                }
                if settings_dirty {
                    self.settings.save();
                }
                self.merge_debug_actions(result.debug_actions);
                if !self.show_settings_window {
                    self.clear_rebinding_state();
                }
                self.egui_wants_keyboard = result.egui_wants_keyboard;
                self.game_view_focused = result.game_view_focused;
            }
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout) => {}
        }

        #[cfg(not(target_arch = "wasm32"))]
        if focus_debugger_window && let Some(gfx) = self.gfx.as_ref() {
            gfx.focus_debugger_window();
        }

        if settings_was_open && !self.show_settings_window {
            self.settings.save();
        }

        if self.settings.video.vsync_mode != self.timing.last_vsync_mode {
            self.timing.last_vsync_mode = self.settings.video.vsync_mode;
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_vsync(self.settings.video.vsync_mode);
            }
        }

        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_debugger_frame(
        &mut self,
        ui_frame_data: Option<&crate::ui::UiFrameData>,
    ) -> bool {
        let supports_rewind = self.core_supports_rewind();
        let supports_debugger = self.core_supports_debugger();
        let supports_execution_controls = self.core_supports_execution_controls();
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        match gfx.render_debugger(graphics::DebuggerRenderContext {
            data: debug_data_refs(ui_frame_data, &self.symbols),
            debug_windows: &mut self.debug_windows,
            settings: &self.settings,
            dock_state: &mut self.debug_dock,
            supports_rewind,
            supports_debugger,
            supports_execution_controls,
        }) {
            Ok(result) => {
                self.merge_debug_actions(result.debug_actions);
                true
            }
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.debugger_window().map(winit::window::Window::inner_size) {
                    gfx.resize_debugger(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_settings_frame(
        &mut self,
        ui_frame_data: Option<&crate::ui::UiFrameData>,
    ) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        let gb_hardware_mode_label = ui_frame_data
            .and_then(|data| data.perf_info.as_ref())
            .filter(|perf| perf.platform_name == "Game Boy")
            .map(|perf| perf.hardware_label.as_ref());
        match gfx.render_settings_window(graphics::SettingsRenderContext {
            settings: &mut self.settings,
            state: &mut self.debug_windows,
            active_system: self.emu_thread.as_ref().map(|_| self.active_system),
            gb_hardware_mode_label,
            is_pocket_camera: self.rom_info.is_pocket_camera,
        }) {
            Ok(()) => true,
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.settings_window().map(winit::window::Window::inner_size) {
                    gfx.resize_settings_window(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_mods_frame(&mut self) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        match gfx.render_mods_window(graphics::ModsRenderContext {
            settings: &self.settings,
            state: &mut self.debug_windows.mod_state,
        }) {
            Ok(()) => true,
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.mods_window().map(winit::window::Window::inner_size) {
                    gfx.resize_mods_window(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_cheats_frame(&mut self) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        match gfx.render_cheats_window(graphics::CheatsRenderContext {
            settings: &self.settings,
            state: &mut self.debug_windows.cheat,
        }) {
            Ok(()) => true,
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.cheats_window().map(winit::window::Window::inner_size) {
                    gfx.resize_cheats_window(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_printer_frame(&mut self) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };
        match gfx.render_printer_window(graphics::PrinterRenderContext {
            settings: &self.settings,
            state: &mut self.debug_windows.printer,
        }) {
            Ok(()) => true,
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                if let Some(size) = gfx.printer_window().map(winit::window::Window::inner_size) {
                    gfx.resize_printer_window(size.width, size.height);
                }
                false
            }
            Err(graphics::FrameError::Timeout) => false,
        }
    }

    pub(super) fn clear_rebinding_state(&mut self) {
        self.debug_windows.rebinding_action = None;
        self.debug_windows.rebinding_shortcut = None;
        self.debug_windows.rebinding_gamepad = None;
        self.debug_windows.rebinding_gamepad_p2 = None;
        self.debug_windows.rebinding_ws_gamepad = None;
        self.debug_windows.rebinding_gamepad_action = None;
        self.debug_windows.rebinding_speedup = false;
        self.debug_windows.rebinding_rewind = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn request_native_tool_window(show: &mut bool, focus_pending: &mut bool) {
    *show = true;
    *focus_pending = true;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::request_native_tool_window;

    #[test]
    fn repeat_tool_window_request_marks_an_open_window_for_focus() {
        let mut visible = true;
        let mut focus_pending = false;

        request_native_tool_window(&mut visible, &mut focus_pending);

        assert!(visible);
        assert!(focus_pending);

        focus_pending = false;
        request_native_tool_window(&mut visible, &mut focus_pending);
        assert!(focus_pending);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod load_eta_tests {
    use std::time::Duration;

    use super::estimate_load_eta;

    #[test]
    fn archive_eta_waits_for_a_stable_sample_and_formats_remaining_time() {
        assert_eq!(estimate_load_eta(Duration::from_millis(900), 50, 100), None);
        assert_eq!(
            estimate_load_eta(Duration::from_secs(10), 25, 100).as_deref(),
            Some("About 30s remaining")
        );
        assert_eq!(
            estimate_load_eta(Duration::from_secs(90), 25, 100).as_deref(),
            Some("About 4m 30s remaining")
        );
        assert_eq!(estimate_load_eta(Duration::from_secs(10), 100, 100), None);
    }
}

fn debug_data_refs<'a>(
    data: Option<&'a crate::ui::UiFrameData>,
    symbols: &'a crate::symbols::SymbolSession,
) -> DebugDataRefs<'a> {
    DebugDataRefs {
        symbols,
        cpu_debug: data.and_then(|data| data.cpu_debug.as_ref()),
        perf_info: data.and_then(|data| data.perf_info.as_ref()),
        apu_debug: data.and_then(|data| data.apu_debug.as_ref()),
        oam_debug: data.and_then(|data| data.oam_debug.as_ref()),
        palette_debug: data.and_then(|data| data.palette_debug.as_ref()),
        rom_debug: data.and_then(|data| data.rom_debug.as_ref()),
        input_debug: data.and_then(|data| data.input_debug.as_ref()),
        graphics_data: data.and_then(|data| data.graphics_data.as_ref()),
        disassembly_view: data.and_then(|data| data.disassembly_view.as_ref()),
        memory_page: data.and_then(|data| data.memory_page.as_deref()),
        rom_page: data.and_then(|data| data.rom_page.as_deref()),
        rom_size: data.map_or(0, |data| data.rom_size),
    }
}
