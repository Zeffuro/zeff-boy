#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use anyhow::Context;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use super::{App, VIEWER_UPDATE_INTERVAL};
use crate::debug::{
    TasEditorFileRequest, TasEditorHostRequest, TasEditorLiveAction, TasEditorLiveStatus,
    TasEditorPresentation,
};
use crate::graphics;
use crate::platform::Instant;

mod conversion;
mod selection;
pub(super) use conversion::VerifiedReplayExportCoordinator;
use selection::{
    has_coleco_extension, has_fds_extension, has_game_gear_extension, has_gb_extension,
    has_gba_extension, has_gbc_extension, has_nes_extension, has_pce_cd_extension,
    has_pce_extension, has_sg1000_extension, has_sms_extension, has_ws_extension,
    has_zip_extension, readiness_summary, tas_source_matches,
};

impl App {
    pub(super) fn handle_tas_editor_host_request(&mut self, request: TasEditorHostRequest) {
        match request {
            TasEditorHostRequest::File(request) => self.handle_tas_editor_file_request(request),
            TasEditorHostRequest::ReplaceProject {
                path,
                game_gear_no_save,
            } => self.create_tas_project(path, true, game_gear_no_save),
            TasEditorHostRequest::Live(TasEditorLiveAction::ReloadLoadedGame) => {
                if let Err(error) = self.repair_loaded_game_and_connect_tas() {
                    self.toast_manager
                        .error(format!("Could not reload and connect TAS: {error:#}"));
                }
            }
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
            TasEditorHostRequest::Live(TasEditorLiveAction::GoToSelection) => {
                if let Err(error) = self.seek_linked_tas_to_editor_cursor() {
                    self.toast_manager
                        .error(format!("Could not move the linked game: {error:#}"));
                    self.cancel_tas_control();
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::ReconstructAfterEdit {
                start,
                end,
            }) => {
                if let Err(error) = self.reconstruct_linked_tas_after_edit(start, end) {
                    self.toast_manager
                        .error(format!("Could not show the edited TAS input: {error:#}"));
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
            TasEditorHostRequest::Live(TasEditorLiveAction::StartPlayback) => {
                if let Err(error) = self.start_linked_tas_playback() {
                    self.toast_manager
                        .error(format!("Could not play TAS movie input: {error:#}"));
                }
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::PausePlayback) => {
                self.pause_linked_tas_playback();
            }
            TasEditorHostRequest::Live(TasEditorLiveAction::ReturnToGameUnchanged) => {
                self.cancel_tas_control();
            }
        }
        self.refresh_tas_editor_live_status();
    }

    pub(super) fn refresh_tas_editor_live_status(&mut self) {
        self.refresh_tas_control_readiness();
        let status = match self.tas_control_live_status() {
            TasEditorLiveStatus::Ready { .. } => self.detached_tas_editor_live_status(),
            status => status,
        };
        self.debug_windows.tas_editor.set_live_status(status);
        if let Some(request) = self.debug_windows.tas_editor.take_pending_host_request() {
            self.handle_tas_editor_host_request(request);
        }
    }

    pub(in crate::app) fn repair_loaded_game_and_connect_tas(&mut self) -> anyhow::Result<()> {
        let source_path = self
            .rom_info
            .source_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no source-loaded game is available to repair"))?;
        let (project, project_content_sha256, profile) = {
            let session = self
                .debug_windows
                .tas_editor
                .active_session()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            (
                session.project().clone(),
                session.project_content_sha256(),
                crate::emu_backend::loader::classify_direct_tas_execution_profile(
                    session.project(),
                )?,
            )
        };
        let loader = crate::emu_backend::loader::select_private_tas_execution_loader_for_project(
            source_path,
            self.active_system,
            self.settings.emulation.firmware_search_dirs(),
            &project,
        )?;
        let identity = project.identity();
        let repaired_backend = loader.load_repair_backend(&project)?;
        let persistence = crate::app::tas_control::repair::persistence_contract_for_project(
            &project,
            &repaired_backend,
            profile,
        )?;
        let prepared = self.prepare_tas_repair(
            crate::app::tas_control::repair::TasRepairTarget {
                project_content_sha256,
                profile,
                source_media_sha256: identity.source_media_sha256,
                effective_media_sha256: identity.effective_media_sha256,
                required_sample_rate: 48_000,
                persistence,
            },
            repaired_backend,
        )?;
        self.activate_prepared_tas_repair(prepared)?;
        Ok(())
    }

    pub(in crate::app) fn detached_tas_editor_live_status(&self) -> TasEditorLiveStatus {
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
                    && (source_path.is_some_and(has_nes_extension)
                        || (source_path.is_some_and(has_zip_extension)
                            && self
                                .rom_info
                                .rom_path
                                .as_deref()
                                .is_some_and(has_nes_extension)))
                    && crate::emu_backend::loader::DirectNesTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectFdsDisk => {
                self.active_system == crate::emu_backend::ActiveSystem::Nes
                    && (source_path.is_some_and(has_fds_extension)
                        || (source_path.is_some_and(has_zip_extension)
                            && self
                                .rom_info
                                .rom_path
                                .as_deref()
                                .is_some_and(has_fds_extension)))
                    && crate::emu_backend::loader::DirectFdsTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg => {
                self.active_system == crate::emu_backend::ActiveSystem::GameBoy
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_gb_extension)
                    && crate::emu_backend::loader::DirectGbTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb => {
                self.active_system == crate::emu_backend::ActiveSystem::GameBoy
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_gbc_extension)
                    && crate::emu_backend::loader::DirectGbcTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::Coleco
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_coleco_extension)
                    && crate::emu_backend::loader::DirectColecoTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectSmsCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::MasterSystem
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_sms_extension)
                    && crate::emu_backend::loader::DirectSmsTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::GameGear
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_game_gear_extension)
                    && crate::emu_backend::loader::DirectGameGearTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::GameBoyAdvance
                    && (source_path.is_some_and(has_gba_extension)
                        || (source_path.is_some_and(has_zip_extension)
                            && self
                                .rom_info
                                .rom_path
                                .as_deref()
                                .is_some_and(has_gba_extension)))
                    && crate::emu_backend::loader::DirectGbaTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::Sg1000
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_sg1000_extension)
                    && crate::emu_backend::loader::DirectSg1000TasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
                self.active_system == crate::emu_backend::ActiveSystem::WonderSwan
                    && tas_source_matches(source_path, self.rom_info.rom_path.as_deref(), has_ws_extension)
                    && crate::emu_backend::loader::DirectWsTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectPceHuCard
            | crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard => {
                self.active_system == crate::emu_backend::ActiveSystem::Pce
                    && (source_path.is_some_and(has_pce_extension)
                        || (source_path.is_some_and(has_zip_extension)
                            && self
                                .rom_info
                                .rom_path
                                .as_deref()
                                .is_some_and(has_pce_extension)))
                    && crate::emu_backend::loader::DirectPceTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
            crate::emu_thread::TasExecutionProfile::DirectPceCd => {
                self.active_system == crate::emu_backend::ActiveSystem::Pce
                    && source_path.is_some_and(has_pce_cd_extension)
                    && crate::emu_backend::loader::DirectPceCdTasExecutionLoader::validate_project_branch_scope(
                        session.project(), session.selected_branch_id()).is_ok()
            }
        };
        if !valid_source {
            return TasEditorLiveStatus::Unavailable(
                "The loaded game does not match this direct TAS profile".to_owned(),
            );
        }
        if self.tas_control.readiness_pending() {
            return TasEditorLiveStatus::Unavailable("Checking TAS readiness…".to_owned());
        }
        let Some(readiness) = self.tas_control_readiness_report() else {
            return TasEditorLiveStatus::Unavailable("Checking TAS readiness…".to_owned());
        };
        match readiness.status {
            crate::app::tas_control::readiness::TasReadinessStatus::Ready => {}
            crate::app::tas_control::readiness::TasReadinessStatus::ReloadRequired => {
                return TasEditorLiveStatus::ReloadRequired(readiness_summary(readiness, true));
            }
            crate::app::tas_control::readiness::TasReadinessStatus::Incompatible => {
                return TasEditorLiveStatus::Unavailable(readiness_summary(readiness, false));
            }
        }
        TasEditorLiveStatus::Ready {
            recording_available: crate::app::tas_control::profile_supports_live_input_recording(
                profile,
            ),
        }
    }

    pub(super) fn record_current_tas_input_and_advance(&mut self) -> anyhow::Result<()> {
        if !self.tas_control.can_record_live_input() {
            anyhow::bail!("no completed TAS execution is awaiting a live input frame");
        }
        let (profile, _) = self.tas_control.linked_identity().ok_or_else(|| {
            anyhow::anyhow!("no completed TAS execution is awaiting a live input frame")
        })?;
        let mut input = crate::tas_project::TasInputFrame::default();
        match profile {
            crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => {
                input.coleco = self.host_input.coleco_tas_controllers()?;
            }
            crate::emu_thread::TasExecutionProfile::DirectSmsCartridge
            | crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                let (p2_buttons, p2_dpad) = self.current_host_joypad_p2_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
                input.players[1].buttons = p2_buttons;
                input.players[1].dpad = p2_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
            }
            profile => {
                let (p1_buttons, p1_dpad) = self.current_host_joypad_input();
                input.players[0].buttons = p1_buttons;
                input.players[0].dpad = p1_dpad;
                if matches!(
                    profile,
                    crate::emu_thread::TasExecutionProfile::DirectNesCartridge
                        | crate::emu_thread::TasExecutionProfile::DirectFdsDisk
                ) {
                    let (p2_buttons, p2_dpad) = self.current_host_joypad_p2_input();
                    input.players[1].buttons = p2_buttons;
                    input.players[1].dpad = p2_dpad;
                    if profile == crate::emu_thread::TasExecutionProfile::DirectNesCartridge {
                        let zapper = self.nes_zapper_input();
                        input.zapper.enabled = zapper.enabled;
                        input.zapper.trigger = zapper.trigger;
                        input.zapper.hit = zapper.hit;
                        input.zapper.screen_pos = zapper.screen_pos.map(|(x, y)| [x, y]);
                    }
                }
            }
        }
        self.record_live_tas_frame(input)
    }

    pub(super) fn handle_tas_editor_file_request(&mut self, request: TasEditorFileRequest) {
        if self.tas_verified_replay_export.is_some() {
            self.toast_manager
                .error("Finish the verified replay export before changing TAS files");
            return;
        }
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
            TasEditorFileRequest::NewGameGearNoSaveProject => {
                self.new_game_gear_no_save_tas_project_dialog()
            }
            TasEditorFileRequest::ImportReplay => self.import_replay_as_tas_dialog(),
            TasEditorFileRequest::ExportReplay => self.export_tas_as_verified_replay_dialog(),
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
                .error("Load a supported direct cartridge first");
            return;
        };
        if self.active_system == crate::emu_backend::ActiveSystem::GameGear {
            let loader = if source_path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
            {
                crate::emu_backend::loader::DirectGameGearTasExecutionLoader::new_zip(
                    source_path.clone(),
                    self.rom_info.rom_path.clone(),
                    false,
                )
            } else {
                crate::emu_backend::loader::DirectGameGearTasExecutionLoader::new(
                    source_path.clone(),
                )
            };
            match loader.requires_confirmed_no_cartridge_save_memory() {
                Ok(true) => {
                    self.debug_windows
                        .tas_editor
                        .request_game_gear_no_save_confirmation();
                    return;
                }
                Ok(false) => {}
                Err(error) => {
                    self.toast_manager
                        .error(format!("Could not create a TAS project: {error:#}"));
                    return;
                }
            }
        }
        self.new_tas_project_dialog_with_board_choice(source_path, false);
    }

    fn new_game_gear_no_save_tas_project_dialog(&mut self) {
        let Some(source_path) = self.rom_info.source_path.clone() else {
            self.toast_manager
                .error("Load a supported direct cartridge first");
            return;
        };
        if self.active_system != crate::emu_backend::ActiveSystem::GameGear {
            self.toast_manager
                .error("The no-save confirmation is only available for Game Gear cartridges");
            return;
        }
        self.new_tas_project_dialog_with_board_choice(source_path, true);
    }

    fn new_tas_project_dialog_with_board_choice(
        &mut self,
        source_path: PathBuf,
        game_gear_no_save: bool,
    ) {
        let preflight = if game_gear_no_save {
            crate::emu_backend::loader::DirectGameGearTasExecutionLoader::new_zip(
                source_path.clone(),
                self.rom_info.rom_path.clone(),
                true,
            )
            .load_fresh_backend()
            .map(|_| ())
        } else {
            crate::emu_backend::loader::select_private_tas_execution_loader_with_rom_path(
                source_path.clone(),
                self.rom_info.rom_path.clone(),
                self.active_system,
                self.settings.emulation.firmware_search_dirs(),
            )
            .map(|_| ())
        };
        if let Err(error) = preflight {
            self.toast_manager
                .error(format!("Could not create a TAS project: {error:#}"));
            return;
        }
        let mut dialog = crate::platform::FileDialog::new()
            .set_title("Create TAS project")
            .add_filter("Zeff TAS project", &["ztas"])
            .set_file_name(&default_project_name(&source_path));
        if let Some(parent) = source_path.parent() {
            dialog = dialog.set_directory(parent);
        }
        self.pause_for_dialog();
        let project_path = dialog.save_file().map(ensure_project_extension);
        self.resume_after_dialog();
        let Some(project_path) = project_path else {
            return;
        };
        if project_path.exists() {
            self.debug_windows
                .tas_editor
                .request_project_replacement_with_game_gear_no_save(
                    project_path,
                    game_gear_no_save,
                );
            return;
        }

        self.create_tas_project(project_path, false, game_gear_no_save);
    }

    fn create_tas_project(
        &mut self,
        project_path: PathBuf,
        replace_existing: bool,
        game_gear_no_save: bool,
    ) {
        match self.create_tas_project_with_board_choice(
            project_path.clone(),
            replace_existing,
            game_gear_no_save,
        ) {
            Ok(()) => self
                .toast_manager
                .info(format!("Created {}", project_path.display())),
            Err(error) => self
                .toast_manager
                .error(format!("Failed to create TAS project: {error:#}")),
        }
    }

    pub(in crate::app) fn create_tas_project_for_live_control(
        &mut self,
        project_path: PathBuf,
        replace_existing: bool,
    ) -> anyhow::Result<()> {
        self.create_tas_project_with_board_choice(project_path, replace_existing, false)
    }

    fn create_tas_project_with_board_choice(
        &mut self,
        project_path: PathBuf,
        replace_existing: bool,
        game_gear_no_save: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.tas_verified_replay_export.is_none(),
            "finish the verified replay export before changing TAS projects"
        );
        anyhow::ensure!(
            crate::tas_project::TasProject::is_project_path(&project_path),
            "TAS projects must use the .ztas extension"
        );
        anyhow::ensure!(
            replace_existing || !project_path.exists(),
            "TAS project destination already exists; set replace_existing to confirm replacement"
        );
        let Some(source_path) = self.rom_info.source_path.clone() else {
            anyhow::bail!("Load a supported direct cartridge first");
        };
        let loader = if game_gear_no_save {
            anyhow::ensure!(
                self.active_system == crate::emu_backend::ActiveSystem::GameGear,
                "the no-save confirmation is only available for Game Gear cartridges"
            );
            crate::emu_backend::loader::PrivateTasExecutionLoader::DirectGameGear(
                crate::emu_backend::loader::DirectGameGearTasExecutionLoader::new_zip(
                    source_path,
                    self.rom_info.rom_path.clone(),
                    true,
                ),
            )
        } else if self.active_system == crate::emu_backend::ActiveSystem::Pce
            && self.settings.emulation.pce_controller.core_mode()
                == zeff_pce_core::hardware::PceControllerMode::SixButton
        {
            let loader = if source_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pce"))
            {
                crate::emu_backend::loader::DirectPceTasExecutionLoader::new_six_button(source_path)
            } else if source_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
            {
                crate::emu_backend::loader::DirectPceTasExecutionLoader::new_zip_six_button(
                    source_path,
                    self.rom_info.rom_path.clone(),
                )
            } else {
                anyhow::bail!(
                    "PC Engine TAS execution requires a direct .pce file or selected ZIP member"
                );
            };
            crate::emu_backend::loader::PrivateTasExecutionLoader::DirectPce(loader)
        } else {
            crate::emu_backend::loader::select_private_tas_execution_loader_with_rom_path(
                source_path,
                self.rom_info.rom_path.clone(),
                self.active_system,
                self.settings.emulation.firmware_search_dirs(),
            )?
        };

        if replace_existing {
            loader.replace_project_file(&project_path)
        } else {
            loader.create_project_file(&project_path)
        }?;
        self.cancel_tas_control();
        self.debug_windows
            .tas_editor
            .open_project(project_path)
            .context("created the TAS project but could not open it")?;
        self.reevaluate_tas_execution_attachment();
        Ok(())
    }

    pub(super) fn sync_tas_editor(&mut self, event_loop: &ActiveEventLoop) {
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
        assert!(has_coleco_extension(Path::new("GAME.COL")));
        assert!(has_sms_extension(Path::new("GAME.SMS")));
        assert!(!has_sms_extension(Path::new("GAME.GG")));
        assert!(has_sg1000_extension(Path::new("GAME.SG")));
        assert!(has_sg1000_extension(Path::new("GAME.SC")));
        assert!(!has_sg1000_extension(Path::new("GAME.SMS")));
        assert!(has_gba_extension(Path::new("GAME.GBA")));
        assert!(has_ws_extension(Path::new("GAME.WS")));
        assert!(has_ws_extension(Path::new("GAME.WSC")));
        assert!(!has_ws_extension(Path::new("GAME.GB")));
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
