#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use super::{App, VIEWER_UPDATE_INTERVAL};
use crate::debug::{
    TasEditorFileRequest, TasEditorHostRequest, TasEditorLiveAction, TasEditorLiveStatus,
    TasEditorPresentation,
};
use crate::graphics;
use crate::platform::Instant;

impl App {
    pub(super) fn handle_tas_editor_host_request(&mut self, request: TasEditorHostRequest) {
        match request {
            TasEditorHostRequest::File(request) => self.handle_tas_editor_file_request(request),
            TasEditorHostRequest::Live(TasEditorLiveAction::StageSelectedInput) => {
                if let Err(error) = self.begin_tas_control_acquire() {
                    self.toast_manager
                        .error(format!("Could not stage TAS input: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::RecordFromSelectedInput) => {
                if let Err(error) = self.begin_tas_control_recording_acquire() {
                    self.toast_manager
                        .error(format!("Could not start TAS recording: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::SeekLinkedInput) => {
                if let Err(error) = self.seek_linked_tas_to_editor_cursor() {
                    self.toast_manager
                        .error(format!("Could not move the linked game: {error:#}"));
                    self.cancel_tas_control();
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::KeepResultAndReturnToGame) => {
                if let Err(error) = self.commit_tas_control() {
                    self.toast_manager
                        .error(format!("Could not disconnect the TAS editor: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::RecordCurrentInputAndAdvance) => {
                if let Err(error) = self.record_current_tas_input_and_advance() {
                    self.toast_manager
                        .error(format!("Could not record live TAS input: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::StartRealtimeRecording) => {
                if let Err(error) = self.start_realtime_tas_recording() {
                    self.toast_manager
                        .error(format!("Could not start TAS recording: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::StopRealtimeRecording) => {
                self.stop_realtime_tas_recording();
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::ReturnToGameUnchanged) => {
                self.cancel_tas_control();
            }
        }
        self.refresh_tas_editor_live_status();
    }

    pub(super) fn refresh_tas_editor_live_status(&mut self) {
        let status = match self.tas_control_live_status() {
            TasEditorLiveStatus::Ready { .. } => self.detached_tas_editor_live_status(),
            status => status,
        };
        self.debug_windows.tas_editor.set_live_status(status);
    }

    fn detached_tas_editor_live_status(&self) -> TasEditorLiveStatus {
        if self.emu_thread.is_none() {
            return TasEditorLiveStatus::Unavailable("No game is running".to_owned());
        }
        let Some(session) = self.debug_windows.tas_editor.active_session() else {
            return TasEditorLiveStatus::Unavailable("Open or create a TAS project".to_owned());
        };
        let profile = match crate::emu_backend::loader::classify_direct_tas_execution_profile(
            session.project(),
        ) {
            Ok(profile) => profile,
            Err(error) => return TasEditorLiveStatus::Unavailable(error.to_string()),
        };
        let source_path = self.rom_info.source_path.as_deref();
        let valid_source = match profile {
            crate::emu_thread::TasExecutionProfile::DirectNesCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::Nes
                    && source_path.is_some_and(has_nes_extension)
                    && crate::emu_backend::loader::DirectNesTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectGbRomOnlyDmg => {
                self.active_system == crate::emu_backend::ActiveSystem::GameBoy
                    && source_path.is_some_and(has_gb_extension)
                    && crate::emu_backend::loader::DirectGbTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
        };
        if !valid_source {
            return TasEditorLiveStatus::Unavailable(
                "The loaded game does not match this direct TAS profile".to_owned(),
            );
        }
        TasEditorLiveStatus::Ready {
            recording_available: matches!(
                profile,
                crate::emu_thread::TasExecutionProfile::DirectNesCartridge
            ),
        }
    }

    pub(super) fn record_current_tas_input_and_advance(&mut self) -> anyhow::Result<()> {
        if !self.tas_control.can_record_live_input() {
            anyhow::bail!("no completed TAS execution is awaiting a live input frame");
        }
        let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
        let (p2_buttons, p2_dpad) = self.current_host_joypad_p2_input();
        let mut input = crate::tas_project::TasInputFrame::default();
        input.players[0].buttons = p1_buttons;
        input.players[0].dpad = p1_dpad;
        input.players[1].buttons = p2_buttons;
        input.players[1].dpad = p2_dpad;
        self.record_live_tas_frame(input)
    }

    pub(super) fn handle_tas_editor_file_request(&mut self, request: TasEditorFileRequest) {
        if !self.worker_gameplay_commands_allowed() {
            self.cancel_tas_control();
            self.toast_manager
                .info("Returning the loaded game unchanged; try the file action again afterward");
            return;
        }
        match request {
            TasEditorFileRequest::LoadGame => self.open_file_dialog(),
            TasEditorFileRequest::OpenProject => self.open_tas_project_dialog(),
            TasEditorFileRequest::NewProject => self.new_tas_project_dialog(),
        }
    }

    pub(super) fn open_tas_project_dialog(&mut self) {
        self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .set_title("Open TAS project")
            .add_filter("Zeff TAS project", &["ztas"])
            .pick_file();
        self.resume_after_dialog();
        if let Some(path) = path {
            self.cancel_tas_control();
            match self.debug_windows.tas_editor.open_project(path) {
                Ok(()) => self.reevaluate_tas_execution_attachment(),
                Err(error) => self
                    .toast_manager
                    .error(format!("Failed to open TAS project: {error:#}")),
            }
        }
    }

    fn new_tas_project_dialog(&mut self) {
        let Some(source_path) = self.rom_info.source_path.clone() else {
            self.toast_manager
                .error("Load a direct NES or Game Boy cartridge first");
            return;
        };
        let loader = match crate::emu_backend::loader::select_private_tas_execution_loader(
            source_path.clone(),
            self.active_system,
            self.settings.emulation.firmware_search_dirs(),
        ) {
            Ok(loader) => loader,
            Err(error) => {
                self.toast_manager
                    .error(format!("Could not create a TAS project: {error:#}"));
                return;
            }
        };
        let mut dialog = crate::platform::FileDialog::new()
            .set_title("Create TAS project")
            .add_filter("Zeff TAS project", &["ztas"])
            .set_file_name(&default_project_name(&source_path));
        if let Some(parent) = source_path.parent() {
            dialog = dialog.set_directory(parent);
        }
        self.pause_for_dialog();
        let project_path = dialog.save_file().map(ensure_project_extension);
        let replace_existing = project_path.as_deref().is_some_and(|path| {
            path.exists()
                && crate::platform::confirm_warning(
                    "Replace TAS project?",
                    format!(
                        "{} already exists. Replace it with a new TAS project?\n\nThe existing valid project will be kept as a .bak file.",
                        path.display()
                    ),
                )
        });
        self.resume_after_dialog();
        let Some(project_path) = project_path else {
            return;
        };
        if project_path.exists() && !replace_existing {
            return;
        }

        let creation = if replace_existing {
            loader.replace_project_file(&project_path)
        } else {
            loader.create_project_file(&project_path)
        };
        match creation {
            Ok(_) => {
                self.cancel_tas_control();
                match self
                    .debug_windows
                    .tas_editor
                    .open_project(project_path.clone())
                {
                    Ok(()) => {
                        self.reevaluate_tas_execution_attachment();
                        self.toast_manager
                            .info(format!("Created {}", project_path.display()));
                    }
                    Err(error) => self.toast_manager.error(format!(
                        "Created {}, but could not open it: {error:#}",
                        project_path.display()
                    )),
                }
            }
            Err(error) => self
                .toast_manager
                .error(format!("Failed to create TAS project: {error:#}")),
        }
    }

    pub(super) fn sync_tas_editor(&mut self, event_loop: &ActiveEventLoop) {
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

    pub(super) fn is_tas_editor_window(&self, window_id: winit::window::WindowId) -> bool {
        self.gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::tas_editor_window_id)
            == Some(window_id)
    }

    pub(super) fn handle_tas_editor_window_event(&mut self, event: WindowEvent) {
        let window_interaction = matches!(&event, WindowEvent::Resized(_) | WindowEvent::Moved(_));
        let needs_repaint = self
            .gfx
            .as_mut()
            .is_some_and(|gfx| gfx.tas_editor_handles_event(&event));

        match event {
            WindowEvent::CloseRequested => {
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
                    >= VIEWER_UPDATE_INTERVAL =>
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

    pub(super) fn render_tas_editor_frame(&mut self) -> bool {
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

fn has_nes_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nes"))
}

fn has_gb_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gb"))
}

fn default_project_name(source_path: &Path) -> String {
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("movie");
    format!("{stem}.ztas")
}

fn ensure_project_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("ztas")
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dialog_helpers_keep_explicit_extensions() {
        assert_eq!(
            default_project_name(Path::new("Mega Man.nes")),
            "Mega Man.ztas"
        );
        assert!(has_nes_extension(Path::new("GAME.NES")));
        assert!(has_gb_extension(Path::new("GAME.GB")));
        assert_eq!(
            ensure_project_extension(PathBuf::from("movie")),
            PathBuf::from("movie.ztas")
        );
        assert_eq!(
            ensure_project_extension(PathBuf::from("movie.bin")),
            PathBuf::from("movie.bin")
        );
    }
}
