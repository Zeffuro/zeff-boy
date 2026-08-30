#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorExecutionEngine, TasEditorSession,
    TasEditorSessionSource, TasSeekStateCache,
};

mod attachment;
mod autosave;
mod branch_diff_editor;
mod event_editor;
mod execution_preview;
mod input_clipboard;
mod input_columns;
mod live_execution_ui;
mod metadata_editor;
mod presentation;
mod project_content_ui;
mod recording;
mod special_input_editor;
mod timeline;
mod workflow_ui;

use input_columns::DigitalField;
#[cfg(test)]
use input_columns::{applicable_player_count, digital_columns, player_number};
pub(crate) use live_execution_ui::{
    TasEditorHostRequest, TasEditorLiveAction, TasEditorLiveStatus,
};
use presentation::{
    PlaybackPanelContext, PlaybackPanelControls, TimelinePanelContext, draw_playback_and_save,
    draw_status_message, draw_timeline_editor, timeline_height,
};

const ROW_HEIGHT: f32 = 24.0;
const MIN_TIMELINE_HEIGHT: f32 = 240.0;
const MAX_TIMELINE_HEIGHT: f32 = 920.0;
const TWO_PANE_MIN_WIDTH: f32 = 860.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TasEditorPresentation {
    #[default]
    Embedded,
    SeparateWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorFileRequest {
    LoadGame,
    OpenProject,
    NewProject,
}

#[derive(Debug, Eq, PartialEq)]
enum TasEditorAction {
    OpenProject(PathBuf),
    SaveManual,
    Autosave,
    RecoverAutosave,
    Undo,
    Redo,
    ContinueFileRequest {
        save: bool,
    },
    CancelFileRequest,
    CancelAutosaveRecovery,
    SelectBranch(String),
    SelectCursor(u64),
    ExecuteSeek(u64),
    JumpToBranchDiffHunk(branch_diff_editor::TasBranchDiffJumpAction),
    InputClipboard(input_clipboard::TasInputClipboardAction),
    Event(event_editor::TasEventAction),
    Metadata(metadata_editor::TasMetadataAction),
    SpecialInput(special_input_editor::TasSpecialInputAction),
    ToggleDigital {
        cursor: u64,
        player: usize,
        field: DigitalField,
        mask: u8,
    },
    InsertNeutralFrames {
        cursor: u64,
        count: u64,
    },
    DeleteFrame(u64),
    StartRecordingAtEnd,
    CaptureRecordingFrame,
    StopRecording,
    ForkBranch {
        id: String,
        name: String,
    },
}

impl TasEditorAction {
    fn blocked_while_live_authority_is_active(&self) -> bool {
        matches!(
            self,
            Self::RecoverAutosave
                | Self::Undo
                | Self::Redo
                | Self::ContinueFileRequest { .. }
                | Self::SelectBranch(_)
                | Self::SelectCursor(_)
                | Self::ExecuteSeek(_)
                | Self::JumpToBranchDiffHunk(_)
                | Self::InputClipboard(_)
                | Self::Event(_)
                | Self::Metadata(_)
                | Self::SpecialInput(_)
                | Self::ToggleDigital { .. }
                | Self::InsertNeutralFrames { .. }
                | Self::DeleteFrame(_)
                | Self::StartRecordingAtEnd
                | Self::CaptureRecordingFrame
                | Self::StopRecording
                | Self::ForkBranch { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TasEditorRecordingState {
    branch_id: String,
    cursor: u64,
    draft_undo_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TasEditorExecutionAvailability {
    Checking,
    GameReady,
    Ready,
    Unavailable(String),
}

pub(crate) struct TasEditorWindowState {
    pub(crate) open: bool,
    presentation: TasEditorPresentation,
    separate_focus_pending: bool,
    host_window_focused: bool,
    last_host_render: Instant,
    session: Option<TasEditorSession>,
    pending_file_request: Option<TasEditorFileRequest>,
    ready_file_request: Option<TasEditorFileRequest>,
    pending_autosave_recovery: bool,
    execution_engine: Option<TasEditorExecutionEngine>,
    execution_availability: TasEditorExecutionAvailability,
    live_status: TasEditorLiveStatus,
    execution_preview: execution_preview::TasEditorExecutionPreview,
    branch_diff_editor: branch_diff_editor::TasBranchDiffEditorState,
    input_clipboard: input_clipboard::TasInputClipboardState,
    event_editor: event_editor::TasEventEditorState,
    metadata_editor: metadata_editor::TasMetadataEditorState,
    autosave_scheduler: crate::tas_project::TasEditorAutosaveScheduler,
    autosave_clock: Instant,
    seek_cache_root: PathBuf,
    new_branch_id: String,
    new_branch_name: String,
    recording: Option<TasEditorRecordingState>,
    neutral_insert_count: u64,
    message: Option<(bool, String)>,
    pending_host_request: Option<TasEditorHostRequest>,
}

impl TasEditorWindowState {
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            presentation: TasEditorPresentation::Embedded,
            separate_focus_pending: false,
            host_window_focused: false,
            last_host_render: Instant::now(),
            session: None,
            pending_file_request: None,
            ready_file_request: None,
            pending_autosave_recovery: false,
            execution_engine: None,
            execution_availability: TasEditorExecutionAvailability::Unavailable(
                "no emulator is running".to_owned(),
            ),
            live_status: TasEditorLiveStatus::default(),
            execution_preview: execution_preview::TasEditorExecutionPreview::new(),
            branch_diff_editor: branch_diff_editor::TasBranchDiffEditorState::new(),
            input_clipboard: input_clipboard::TasInputClipboardState::new(),
            event_editor: event_editor::TasEventEditorState::new(),
            metadata_editor: metadata_editor::TasMetadataEditorState::new(),
            autosave_scheduler: crate::tas_project::TasEditorAutosaveScheduler::default(),
            autosave_clock: Instant::now(),
            seek_cache_root: default_seek_cache_root(),
            new_branch_id: String::new(),
            new_branch_name: String::new(),
            recording: None,
            neutral_insert_count: 60,
            message: None,
            pending_host_request: None,
        }
    }

    pub(crate) fn open_project(&mut self, path: PathBuf) -> Result<()> {
        match self.reduce(TasEditorAction::OpenProject(path)) {
            Ok(message) => {
                if let Some(message) = message {
                    self.message = Some((false, message));
                }
                Ok(())
            }
            Err(error) => {
                self.message = Some((true, error.to_string()));
                Err(error)
            }
        }
    }

    pub(crate) fn open_embedded(&mut self) {
        self.presentation = TasEditorPresentation::Embedded;
        self.separate_focus_pending = false;
        self.open = true;
    }

    pub(crate) fn open_separate_window(&mut self) {
        self.presentation = TasEditorPresentation::SeparateWindow;
        self.separate_focus_pending = true;
        self.open = true;
    }

    pub(crate) fn presentation(&self) -> TasEditorPresentation {
        self.presentation
    }

    pub(crate) fn take_separate_focus_request(&mut self) -> bool {
        std::mem::take(&mut self.separate_focus_pending)
    }

    pub(crate) fn host_window_focused(&self) -> bool {
        self.host_window_focused
    }

    pub(crate) fn set_host_window_focused(&mut self, focused: bool) {
        self.host_window_focused = focused;
    }

    pub(crate) fn last_host_render(&self) -> Instant {
        self.last_host_render
    }

    pub(crate) fn mark_host_rendered(&mut self) {
        self.last_host_render = Instant::now();
    }

    pub(crate) fn close(&mut self) {
        self.queue_return_to_game_unchanged();
        self.open = false;
        self.separate_focus_pending = false;
    }

    pub(crate) fn set_live_status(&mut self, status: TasEditorLiveStatus) {
        if matches!(&status, TasEditorLiveStatus::Acquiring) {
            self.execution_preview.clear();
        }
        self.live_status = status;
    }

    pub(crate) fn take_pending_host_request(&mut self) -> Option<TasEditorHostRequest> {
        self.pending_host_request.take()
    }

    pub(crate) fn active_session(&self) -> Option<&TasEditorSession> {
        self.session.as_ref()
    }

    pub(crate) fn live_status(&self) -> &TasEditorLiveStatus {
        &self.live_status
    }

    pub(crate) fn select_cursor_for_live_control(&mut self, cursor: u64) -> Result<()> {
        self.reduce(TasEditorAction::SelectCursor(cursor))?;
        Ok(())
    }

    pub(crate) fn select_end_cursor_for_live_control(&mut self) -> Result<()> {
        let frame_count = self.session_mut()?.selected_branch().frame_count();
        self.select_cursor_for_live_control(frame_count)
    }

    pub(crate) fn commit_prepared_live_frame(
        &mut self,
        prepared: crate::tas_project::TasPreparedLiveFrame,
    ) -> Result<()> {
        self.session_mut()?.commit_prepared_live_frame(prepared)?;
        self.execution_preview.clear();
        Ok(())
    }

    pub(crate) fn begin_live_recording_history_group(&mut self) -> Result<()> {
        self.session_mut()?.begin_live_recording_history_group()
    }

    pub(crate) fn end_live_recording_history_group(&mut self) -> Result<bool> {
        let session = self.session_mut()?;
        if !session.live_recording_history_group_active() {
            return Ok(false);
        }
        session.end_live_recording_history_group()
    }

    fn queue_return_to_game_unchanged(&mut self) {
        if self.live_status.is_linked() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::KeepResultAndReturnToGame,
            ));
        } else if self.live_status.requires_return_on_close() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::ReturnToGameUnchanged,
            ));
        }
    }

    fn queue_linked_seek(&mut self) {
        if self.live_status.is_linked() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::SeekLinkedInput,
            ));
        }
    }

    #[cfg(test)]
    fn with_seek_cache_root(root: PathBuf) -> Self {
        Self {
            seek_cache_root: root,
            ..Self::new()
        }
    }

    fn apply(&mut self, action: TasEditorAction) {
        match self.reduce(action) {
            Ok(message) => {
                if let Some(message) = message {
                    self.message = Some((false, message));
                }
            }
            Err(error) => self.message = Some((true, error.to_string())),
        }
    }

    fn reduce(&mut self, action: TasEditorAction) -> Result<Option<String>> {
        if self.live_status.locks_editor() && action.blocked_while_live_authority_is_active() {
            bail!("finish the live game decision before changing the TAS project");
        }
        match action {
            TasEditorAction::OpenProject(path) => {
                let autosaves =
                    TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default())?;
                let seek_cache = TasSeekStateCache::open(&self.seek_cache_root)?;
                let session = TasEditorSession::open(&path, autosaves, seek_cache)?;
                let source = source_label(session.source());
                self.input_clipboard.clear()?;
                let _ = self.execution_engine.take();
                self.execution_availability = TasEditorExecutionAvailability::Checking;
                self.execution_preview.clear();
                self.branch_diff_editor.clear();
                self.event_editor.clear();
                self.metadata_editor.clear();
                self.autosave_scheduler.reset();
                self.pending_file_request = None;
                self.ready_file_request = None;
                self.pending_autosave_recovery = false;
                self.recording = None;
                self.session = Some(session);
                Ok(Some(format!("Opened {} ({source})", path.display())))
            }
            TasEditorAction::SaveManual => {
                if self.recording.is_some() {
                    bail!("stop frame recording before saving the project");
                }
                let session = self.session_mut()?;
                session.save_manual()?;
                Ok(Some(format!("Saved {}", session.manual_path().display())))
            }
            TasEditorAction::Autosave => {
                if self.recording.is_some() {
                    bail!("stop frame recording before creating a recovery copy");
                }
                let session = self.session_mut()?;
                let saved = session.autosave_now()?;
                Ok(Some(format!(
                    "Autosaved generation {} to {}",
                    saved.generation,
                    saved.path.display()
                )))
            }
            TasEditorAction::RecoverAutosave => {
                self.pending_autosave_recovery = false;
                self.recording = None;
                let session = self.session_mut()?;
                let recovered = session.install_newest_autosave()?;
                let attachment_error = recovered.as_ref().and_then(|_| {
                    self.execution_engine.as_ref().and_then(|engine| {
                        engine
                            .validate_editor_session(
                                self.session
                                    .as_ref()
                                    .expect("autosave recovery requires an open session"),
                            )
                            .err()
                    })
                });
                if recovered.is_some() {
                    self.execution_preview.clear();
                    self.branch_diff_editor.clear();
                    self.input_clipboard.clear()?;
                    self.event_editor.clear();
                    self.metadata_editor.clear();
                }
                if attachment_error.is_some() {
                    let _ = self.execution_engine.take();
                    self.execution_availability = TasEditorExecutionAvailability::Unavailable(
                        "The recovered project no longer matches the loaded game".to_owned(),
                    );
                }
                Ok(Some(match recovered {
                    Some(recovery) => {
                        let detached = attachment_error.map_or_else(String::new, |error| {
                            format!(
                                "; private execution detached because the recovered project no longer matches it: {error:#}"
                            )
                        });
                        format!(
                            "Recovered autosave generation {} from {}{detached}",
                            recovery.generation,
                            recovery.path.display()
                        )
                    }
                    None => "No valid autosave generation was found".to_owned(),
                }))
            }
            TasEditorAction::Undo => {
                if self.recording.is_some() {
                    bail!("stop frame recording before using undo");
                }
                let changed = self.session_mut()?.undo()?;
                if changed {
                    self.execution_preview.clear();
                    self.queue_linked_seek();
                }
                Ok(changed.then(|| {
                    self.detach_incompatible_execution().map_or_else(
                        || "Undid TAS edit".to_owned(),
                        |error| {
                            format!(
                                "Undid TAS edit; private execution detached because the restored project is outside its profile: {error:#}"
                            )
                        },
                    )
                }))
            }
            TasEditorAction::Redo => {
                if self.recording.is_some() {
                    bail!("stop frame recording before using redo");
                }
                let changed = self.session_mut()?.redo()?;
                if changed {
                    self.execution_preview.clear();
                    self.queue_linked_seek();
                }
                Ok(changed.then(|| {
                    self.detach_incompatible_execution().map_or_else(
                        || "Redid TAS edit".to_owned(),
                        |error| {
                            format!(
                                "Redid TAS edit; private execution detached because the restored project is outside its profile: {error:#}"
                            )
                        },
                    )
                }))
            }
            TasEditorAction::ContinueFileRequest { save } => {
                if self.recording.is_some() {
                    bail!("stop frame recording before changing projects");
                }
                let request = self
                    .pending_file_request
                    .ok_or_else(|| anyhow::anyhow!("no TAS file action is waiting"))?;
                if save {
                    self.session_mut()?.save_manual()?;
                }
                self.pending_file_request = None;
                self.ready_file_request = Some(request);
                Ok(save.then(|| "Saved the current TAS project".to_owned()))
            }
            TasEditorAction::CancelFileRequest => {
                self.pending_file_request = None;
                Ok(None)
            }
            TasEditorAction::CancelAutosaveRecovery => {
                self.pending_autosave_recovery = false;
                Ok(None)
            }
            TasEditorAction::SelectBranch(branch_id) => {
                self.discard_recording_draft()?;
                self.session_mut()?.select_branch(&branch_id)?;
                self.execution_preview.clear();
                Ok(None)
            }
            TasEditorAction::SelectCursor(cursor) => {
                if self
                    .recording
                    .as_ref()
                    .is_some_and(|recording| recording.cursor != cursor)
                {
                    self.discard_recording_draft()?;
                }
                let frame_count = self.session_mut()?.selected_branch().frame_count();
                self.session_mut()?.set_cursor(cursor.min(frame_count))?;
                self.execution_preview.clear();
                self.queue_linked_seek();
                Ok(None)
            }
            TasEditorAction::ExecuteSeek(cursor) => {
                self.discard_recording_draft()?;
                let frame_count = self.session_mut()?.selected_branch().frame_count();
                Ok(Some(self.execute_private_seek(cursor.min(frame_count))?))
            }
            TasEditorAction::JumpToBranchDiffHunk(action) => {
                self.discard_recording_draft()?;
                Ok(Some(self.apply_branch_diff_jump_action(action)?))
            }
            TasEditorAction::InputClipboard(action) => {
                self.discard_recording_draft()?;
                Ok(Some(self.apply_input_clipboard_action(action)?))
            }
            TasEditorAction::Event(action) => {
                if self.recording.is_some() {
                    bail!("stop frame recording before editing replay events");
                }
                Ok(Some(self.apply_event_action(action)?))
            }
            TasEditorAction::Metadata(action) => {
                if self.recording.is_some() {
                    bail!("stop frame recording before editing markers or annotations");
                }
                Ok(Some(self.apply_metadata_action(action)?))
            }
            TasEditorAction::SpecialInput(action) => {
                if self.recording.is_some() {
                    bail!("stop frame recording before editing special input");
                }
                Ok(Some(self.apply_special_input_action(action)?))
            }
            TasEditorAction::ToggleDigital {
                cursor,
                player,
                field,
                mask,
            } => {
                if player >= 5 || mask.count_ones() != 1 {
                    bail!("invalid TAS digital input column");
                }
                let session = self.session_mut()?;
                let branch_id = session.selected_branch_id().to_owned();
                if cursor >= session.selected_branch().frame_count() {
                    bail!("cannot edit input at the end cursor");
                }
                let mut input = session.selected_branch().input_at(cursor);
                let controller = &mut input.players[player];
                match field {
                    DigitalField::Buttons => controller.buttons ^= mask,
                    DigitalField::Dpad => controller.dpad ^= mask,
                }
                session.edit_transaction(move |edit| {
                    edit.set_input_range(&branch_id, cursor, 1, input)
                })?;
                self.execution_preview.clear();
                self.queue_linked_seek();
                Ok(None)
            }
            TasEditorAction::InsertNeutralFrames { cursor, count } => {
                if count == 0 {
                    bail!("neutral frame count must be greater than zero");
                }
                self.discard_recording_draft()?;
                let session = self.session_mut()?;
                let branch_id = session.selected_branch_id().to_owned();
                session
                    .edit_transaction(move |edit| edit.insert_frames(&branch_id, cursor, count))?;
                session.set_cursor(cursor)?;
                self.execution_preview.clear();
                self.queue_linked_seek();
                Ok(None)
            }
            TasEditorAction::DeleteFrame(cursor) => {
                self.discard_recording_draft()?;
                let session = self.session_mut()?;
                let branch_id = session.selected_branch_id().to_owned();
                if cursor >= session.selected_branch().frame_count() {
                    bail!("cannot delete the end cursor");
                }
                session.edit_transaction(move |edit| edit.delete_frames(&branch_id, cursor, 1))?;
                self.execution_preview.clear();
                self.queue_linked_seek();
                Ok(None)
            }
            TasEditorAction::StartRecordingAtEnd => self.start_frame_recording(),
            TasEditorAction::CaptureRecordingFrame => self.capture_recording_frame(),
            TasEditorAction::StopRecording => self.stop_frame_recording(),
            TasEditorAction::ForkBranch { id, name } => {
                self.discard_recording_draft()?;
                let id = id.trim().to_owned();
                let name = name.trim().to_owned();
                if id.is_empty() || name.is_empty() {
                    bail!("new branch ID and name are required");
                }
                let session = self.session_mut()?;
                let source_id = session.selected_branch_id().to_owned();
                let fork_cursor = session.cursor();
                let selected_id = id.clone();
                session.edit_transaction(move |edit| {
                    edit.fork_branch(&source_id, fork_cursor, id, name)?;
                    edit.set_active_branch(&selected_id)
                })?;
                self.new_branch_id.clear();
                self.new_branch_name.clear();
                self.execution_preview.clear();
                Ok(None)
            }
        }
    }

    fn session_mut(&mut self) -> Result<&mut TasEditorSession> {
        self.session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))
    }

    fn begin_file_request(
        &mut self,
        request: TasEditorFileRequest,
    ) -> Option<TasEditorFileRequest> {
        if self
            .session
            .as_ref()
            .is_some_and(TasEditorSession::is_dirty)
        {
            self.pending_file_request = Some(request);
            None
        } else {
            Some(request)
        }
    }
}

pub(crate) fn draw_tas_editor_window(
    ctx: &egui::Context,
    state: &mut TasEditorWindowState,
) -> Option<TasEditorHostRequest> {
    if !state.open || state.presentation != TasEditorPresentation::Embedded {
        return None;
    }

    let mut open = state.open;
    let mut host_request = None;
    egui::Window::new("TAS Editor")
        .id(egui::Id::new("tas_editor_window"))
        .open(&mut open)
        .default_size([980.0, 640.0])
        .min_size([620.0, 360.0])
        .show(ctx, |ui| {
            host_request = draw_tas_editor_content(ui, state);
        });
    state.open = open;
    if !open {
        state.queue_return_to_game_unchanged();
        return state.take_pending_host_request();
    }
    host_request.or_else(|| state.take_pending_host_request())
}

pub(crate) fn draw_tas_editor_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
) -> Option<TasEditorHostRequest> {
    let mut actions = Vec::new();
    let mut file_request = None;
    let mut live_action = None;
    draw_content(ui, state, &mut actions, &mut file_request, &mut live_action);
    for action in actions {
        state.apply(action);
    }
    state.take_pending_host_request().or_else(|| {
        live_action.map(TasEditorHostRequest::Live).or_else(|| {
            file_request
                .or_else(|| state.ready_file_request.take())
                .map(TasEditorHostRequest::File)
        })
    })
}

fn draw_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_request: &mut Option<TasEditorFileRequest>,
    live_action: &mut Option<TasEditorLiveAction>,
) {
    let editor_locked = state.live_status.locks_editor();
    let file_actions_locked = state.live_status.holds_authority() || state.recording.is_some();
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!file_actions_locked, egui::Button::new("Open .ztas..."))
            .clicked()
        {
            *file_request = state.begin_file_request(TasEditorFileRequest::OpenProject);
        }
        if ui
            .add_enabled(!file_actions_locked, egui::Button::new("New TAS..."))
            .clicked()
        {
            *file_request = state.begin_file_request(TasEditorFileRequest::NewProject);
        }
        let loaded = state.session.is_some();
        if ui
            .add_enabled(
                loaded && !file_actions_locked,
                egui::Button::new("Save project"),
            )
            .on_hover_text("Write this project to its .ztas file")
            .clicked()
        {
            actions.push(TasEditorAction::SaveManual);
        }
        let can_undo = state
            .session
            .as_ref()
            .is_some_and(TasEditorSession::can_undo);
        let can_redo = state
            .session
            .as_ref()
            .is_some_and(TasEditorSession::can_redo);
        if ui
            .add_enabled(!file_actions_locked && can_undo, egui::Button::new("Undo"))
            .on_hover_text("Ctrl/Cmd+Z")
            .clicked()
        {
            actions.push(TasEditorAction::Undo);
        }
        if ui
            .add_enabled(!file_actions_locked && can_redo, egui::Button::new("Redo"))
            .on_hover_text("Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z")
            .clicked()
        {
            actions.push(TasEditorAction::Redo);
        }
    });
    workflow_ui::draw_pending_file_request(
        ui,
        state.pending_file_request,
        !file_actions_locked,
        actions,
    );

    ui.collapsing("Recovery", |ui| {
        ui.small("Autosaves are separate recovery copies; Save updates the project file.");
        ui.horizontal_wrapped(|ui| {
            let loaded = state.session.is_some();
            if ui
                .add_enabled(
                    !file_actions_locked && loaded,
                    egui::Button::new("Autosave now"),
                )
                .on_hover_text("Write a separate recovery copy without changing the manual save")
                .clicked()
            {
                actions.push(TasEditorAction::Autosave);
            }
            if ui
                .add_enabled(
                    !file_actions_locked && loaded,
                    egui::Button::new("Recover newest autosave"),
                )
                .on_hover_text("Restore the newest valid recovery copy in the editor")
                .clicked()
            {
                state.pending_autosave_recovery = true;
            }
        });
    });
    workflow_ui::draw_autosave_recovery_confirmation(
        ui,
        state.pending_autosave_recovery,
        !file_actions_locked,
        actions,
    );

    let (undo_requested, redo_requested) = ui.input(|input| {
        let command = input.modifiers.command;
        let undo = command && input.key_pressed(egui::Key::Z) && !input.modifiers.shift;
        let redo = command
            && (input.key_pressed(egui::Key::Y)
                || (input.key_pressed(egui::Key::Z) && input.modifiers.shift));
        (undo, redo)
    });
    if !file_actions_locked
        && undo_requested
        && state
            .session
            .as_ref()
            .is_some_and(TasEditorSession::can_undo)
    {
        actions.push(TasEditorAction::Undo);
    } else if !file_actions_locked
        && redo_requested
        && state
            .session
            .as_ref()
            .is_some_and(TasEditorSession::can_redo)
    {
        actions.push(TasEditorAction::Redo);
    }

    draw_status_message(ui, state.message.as_ref());

    if state.session.is_none() {
        workflow_ui::draw_empty_project_state(
            ui,
            &state.execution_availability,
            !editor_locked,
            file_request,
        );
        return;
    };

    let body_height = ui.available_height();
    draw_scrollable_project_content(ui, state, actions, live_action, body_height);
}

fn draw_scrollable_project_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    live_action: &mut Option<TasEditorLiveAction>,
    body_height: f32,
) -> egui::scroll_area::ScrollAreaOutput<()> {
    egui::ScrollArea::vertical()
        .id_salt("tas_editor_body")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            project_content_ui::draw_project_content(ui, state, actions, live_action, body_height);
        })
}

fn source_label(source: TasEditorSessionSource) -> &'static str {
    match source {
        TasEditorSessionSource::Unsaved => "unsaved",
        TasEditorSessionSource::Primary => "manual save",
        TasEditorSessionSource::Backup => "backup recovery",
        TasEditorSessionSource::Autosave => "autosave recovery",
    }
}

fn default_seek_cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zeff-boy")
        .join("tas-seek-v1")
}

#[cfg(test)]
mod branch_diff_tests;
#[cfg(test)]
mod event_tests;
#[cfg(test)]
mod execution_tests;
#[cfg(test)]
mod input_clipboard_tests;
#[cfg(test)]
mod input_pattern_tests;
#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod special_input_tests;
#[cfg(test)]
mod workflow_tests;

#[cfg(test)]
#[path = "tas_editor/state_tests.rs"]
mod tests;
