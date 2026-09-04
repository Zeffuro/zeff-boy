#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorExecutionEngine, TasEditorSession,
    TasEditorSessionSource, TasFrameRange, TasLiveRecordingMode, TasSeekStateCache,
};

mod action;
mod attachment;
mod autosave;
mod branch_diff_editor;
mod branch_navigator;
mod coleco_input;
mod content;
mod event_editor;
mod execution_preview;
mod input_clipboard;
mod input_columns;
mod inspector_ui;
mod live_control;
mod live_execution_ui;
mod metadata_editor;
mod presentation;
mod project_content_ui;
mod recording;
mod special_input_editor;
mod state;
mod timeline;
mod timeline_selection;
mod workflow_ui;

use action::{TasEditorAction, TasTimelineNavigation};
#[cfg(test)]
use coleco_input::ColecoControl;
#[cfg(test)]
use content::draw_scrollable_project_content;
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
enum TasInspectorTab {
    #[default]
    Branches,
    Markers,
    Tools,
}

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
    NewGameGearNoSaveProject,
    ImportReplay,
    ExportReplay,
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

#[derive(Clone, Copy)]
enum TasTimelineSelectionChange {
    Boundary(u64),
    Frame {
        frame: u64,
        extend_selection: bool,
    },
    Range {
        anchor: u64,
        active: u64,
    },
    Navigate {
        navigation: TasTimelineNavigation,
        extend_selection: bool,
    },
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
    pending_project_replacement: Option<(PathBuf, bool)>,
    pending_game_gear_no_save_confirmation: bool,
    pending_autosave_recovery: bool,
    execution_engine: Option<TasEditorExecutionEngine>,
    execution_availability: TasEditorExecutionAvailability,
    live_status: TasEditorLiveStatus,
    linked_session_active: bool,
    close_keep_after_live_command: bool,
    live_recording_mode: TasLiveRecordingMode,
    execution_preview: execution_preview::TasEditorExecutionPreview,
    branch_diff_editor: branch_diff_editor::TasBranchDiffEditorState,
    input_clipboard: input_clipboard::TasInputClipboardState,
    timeline_selection: timeline_selection::TasTimelineSelectionState,
    timeline_follow_selection: bool,
    event_editor: event_editor::TasEventEditorState,
    metadata_editor: metadata_editor::TasMetadataEditorState,
    inspector_tab: TasInspectorTab,
    autosave_scheduler: crate::tas_project::TasEditorAutosaveScheduler,
    autosave_clock: Instant,
    seek_cache_root: PathBuf,
    new_branch_id: String,
    new_branch_name: String,
    active_branch_name: String,
    recording: Option<TasEditorRecordingState>,
    verified_export_busy: bool,
    verified_export_status: Option<String>,
    verified_export_cancel_requested: bool,
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
            pending_project_replacement: None,
            pending_game_gear_no_save_confirmation: false,
            pending_autosave_recovery: false,
            execution_engine: None,
            execution_availability: TasEditorExecutionAvailability::Unavailable(
                "no emulator is running".to_owned(),
            ),
            live_status: TasEditorLiveStatus::default(),
            linked_session_active: false,
            close_keep_after_live_command: false,
            live_recording_mode: TasLiveRecordingMode::default(),
            execution_preview: execution_preview::TasEditorExecutionPreview::new(),
            branch_diff_editor: branch_diff_editor::TasBranchDiffEditorState::new(),
            input_clipboard: input_clipboard::TasInputClipboardState::new(),
            timeline_selection: timeline_selection::TasTimelineSelectionState::new(),
            timeline_follow_selection: false,
            event_editor: event_editor::TasEventEditorState::new(),
            metadata_editor: metadata_editor::TasMetadataEditorState::new(),
            inspector_tab: TasInspectorTab::default(),
            autosave_scheduler: crate::tas_project::TasEditorAutosaveScheduler::default(),
            autosave_clock: Instant::now(),
            seek_cache_root: default_seek_cache_root(),
            new_branch_id: String::new(),
            new_branch_name: String::new(),
            active_branch_name: String::new(),
            recording: None,
            verified_export_busy: false,
            verified_export_status: None,
            verified_export_cancel_requested: false,
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

    pub(crate) fn request_project_replacement_with_game_gear_no_save(
        &mut self,
        path: PathBuf,
        game_gear_no_save: bool,
    ) {
        self.pending_project_replacement = Some((path, game_gear_no_save));
    }

    pub(crate) fn request_game_gear_no_save_confirmation(&mut self) {
        self.pending_game_gear_no_save_confirmation = true;
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

    pub(crate) fn set_verified_export_busy(&mut self, busy: bool) {
        self.verified_export_busy = busy;
        if !busy {
            self.verified_export_cancel_requested = false;
        }
    }

    pub(crate) fn set_verified_export_status(&mut self, status: Option<String>) {
        self.verified_export_status = status;
    }

    pub(crate) fn request_verified_export_cancellation(&mut self) {
        self.verified_export_cancel_requested = true;
    }

    pub(crate) fn take_verified_export_cancellation_request(&mut self) -> bool {
        std::mem::take(&mut self.verified_export_cancel_requested)
    }

    pub(crate) fn verified_export_status(&self) -> Option<&str> {
        self.verified_export_status.as_deref()
    }

    pub(crate) fn verified_export_cancel_requested(&self) -> bool {
        self.verified_export_cancel_requested
    }

    pub(crate) fn install_verified_export_session(&mut self, session: TasEditorSession) {
        self.session = Some(session);
        self.reset_active_branch_name();
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
        if self.verified_export_busy {
            bail!("wait for the verified replay export to finish before changing the TAS project");
        }
        if self.live_status.locks_editor()
            && action.blocked_while_live_authority_is_active()
            && !self.allows_linked_boundary_branch(&action)
        {
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
                self.timeline_selection.reset();
                self.timeline_follow_selection = true;
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
                self.reset_active_branch_name();
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
                    self.timeline_follow_selection = true;
                    self.execution_preview.clear();
                    self.branch_diff_editor.clear();
                    self.input_clipboard.clear()?;
                    self.timeline_selection.reset();
                    self.event_editor.clear();
                    self.metadata_editor.clear();
                    self.reset_active_branch_name();
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
                    self.timeline_follow_selection = true;
                    self.execution_preview.clear();
                    self.reset_active_branch_name();
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
                    self.timeline_follow_selection = true;
                    self.execution_preview.clear();
                    self.reset_active_branch_name();
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
                self.timeline_follow_selection = true;
                self.execution_preview.clear();
                self.reset_active_branch_name();
                Ok(None)
            }
            TasEditorAction::SelectCursor(cursor) => {
                self.apply_timeline_selection_change(TasTimelineSelectionChange::Boundary(cursor))?;
                Ok(None)
            }
            TasEditorAction::SelectTimelineFrame {
                frame,
                extend_selection,
            } => {
                self.apply_timeline_selection_change(TasTimelineSelectionChange::Frame {
                    frame,
                    extend_selection,
                })?;
                Ok(None)
            }
            TasEditorAction::SelectTimelineRange { anchor, active } => {
                self.apply_timeline_selection_change(TasTimelineSelectionChange::Range {
                    anchor,
                    active,
                })?;
                Ok(None)
            }
            TasEditorAction::NavigateTimelineSelection {
                navigation,
                extend_selection,
            } => {
                self.apply_timeline_selection_change(TasTimelineSelectionChange::Navigate {
                    navigation,
                    extend_selection,
                })?;
                Ok(None)
            }
            TasEditorAction::SelectAllTimelineFrames => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
                self.timeline_selection.select_all(session);
                Ok(None)
            }
            TasEditorAction::ClearTimelineSelection => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
                self.timeline_selection.collapse_to_cursor(session);
                Ok(None)
            }
            TasEditorAction::SelectLiveExecutionBoundary => {
                if let Some(cursor) = self.live_status.execution_boundary() {
                    self.apply_timeline_selection_change(TasTimelineSelectionChange::Boundary(
                        cursor,
                    ))?;
                }
                Ok(None)
            }
            TasEditorAction::RequestLiveGoToSelection => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
                if self.timeline_selection.snapshot(session).is_none() {
                    return Ok(None);
                }
                let target = session.cursor();
                if self
                    .live_status
                    .execution_boundary()
                    .is_some_and(|boundary| boundary != target)
                {
                    self.pending_host_request = Some(TasEditorHostRequest::Live(
                        TasEditorLiveAction::GoToSelection,
                    ));
                }
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
            } => self.edit_digital_input(cursor, player, field, mask, None),
            TasEditorAction::SetDigital {
                cursor,
                player,
                field,
                mask,
                pressed,
            } => self.edit_digital_input(cursor, player, field, mask, Some(pressed)),
            TasEditorAction::ToggleColecoControl {
                cursor,
                player,
                control,
            } => self.toggle_coleco_control(cursor, player, control),
            TasEditorAction::SetColecoKeypad {
                cursor,
                player,
                key,
            } => self.set_coleco_keypad(cursor, player, key),
            TasEditorAction::InsertNeutralFrames { cursor, count } => {
                if count == 0 {
                    bail!("neutral frame count must be greater than zero");
                }
                self.discard_recording_draft()?;
                let session = self.session_mut()?;
                session.insert_neutral_frames(cursor, count)?;
                session.set_cursor(cursor)?;
                let end = cursor
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("inserted TAS selection boundary overflow"))?;
                let session = self
                    .session
                    .as_ref()
                    .expect("open session was checked before inserting frames");
                self.timeline_selection.select_range(session, cursor, end);
                self.timeline_follow_selection = true;
                self.execution_preview.clear();
                Ok(None)
            }
            TasEditorAction::DeleteFrames { start, count } => {
                if count == 0 {
                    bail!("select one or more input frames to delete");
                }
                self.discard_recording_draft()?;
                let end = start
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("deleted TAS range boundary overflow"))?;
                self.session_mut()?
                    .delete_frame_range(TasFrameRange::new(start, end)?)?;
                let session = self
                    .session
                    .as_ref()
                    .expect("open session was checked before deleting selected frames");
                if start < session.selected_branch().frame_count() {
                    self.timeline_selection.select_frame(session, start, false);
                } else {
                    self.timeline_selection.collapse_to_cursor(session);
                }
                self.timeline_follow_selection = true;
                self.execution_preview.clear();
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
                self.timeline_follow_selection = true;
                self.new_branch_id.clear();
                self.new_branch_name.clear();
                self.execution_preview.clear();
                self.reset_active_branch_name();
                Ok(None)
            }
            TasEditorAction::DeleteBranchSubtree { id } => {
                if self.recording.is_some() {
                    bail!("stop frame recording before deleting a branch");
                }
                if self.live_status.holds_authority() {
                    bail!("finish the live game decision before deleting a branch");
                }
                let session = self.session_mut()?;
                let name = session
                    .project()
                    .branch(&id)
                    .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {id:?}"))?
                    .name()
                    .to_owned();
                let mut deleted = 0;
                session.edit_transaction(|edit| {
                    deleted = edit.delete_branch_subtree(&id)?;
                    Ok(())
                })?;
                self.branch_diff_editor.clear();
                self.execution_preview.clear();
                Ok(Some(if deleted == 1 {
                    format!("Deleted branch {name}")
                } else {
                    format!("Deleted branch {name} and {} descendants", deleted - 1)
                }))
            }
            TasEditorAction::RenameActiveBranch { name } => {
                if self.recording.is_some() {
                    bail!("stop frame recording before renaming a branch");
                }
                let name = name.trim().to_owned();
                if name.is_empty() {
                    bail!("branch name is required");
                }
                let session = self.session_mut()?;
                let branch_id = session.selected_branch_id().to_owned();
                let changed = session
                    .edit_transaction(move |edit| edit.rename_branch(&branch_id, name))?
                    .changed;
                if changed {
                    self.reset_active_branch_name();
                    Ok(Some(format!(
                        "Renamed active branch to {}",
                        self.session
                            .as_ref()
                            .expect("open session was checked before renaming a branch")
                            .selected_branch()
                            .name()
                    )))
                } else {
                    Ok(None)
                }
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
    presentation::draw_embedded_window(ctx, state)
}

pub(crate) fn draw_tas_editor_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
) -> Option<TasEditorHostRequest> {
    let mut actions = Vec::new();
    let mut file_request = None;
    let mut project_replacement: Option<(PathBuf, bool)> = None;
    let mut live_action = None;
    content::draw(
        ui,
        state,
        &mut actions,
        &mut file_request,
        &mut project_replacement,
        &mut live_action,
    );
    for action in actions {
        state.apply(action);
    }
    state
        .take_pending_host_request()
        .or_else(|| {
            live_action.map(TasEditorHostRequest::Live).or_else(|| {
                file_request
                    .or_else(|| state.ready_file_request.take())
                    .map(TasEditorHostRequest::File)
            })
        })
        .or_else(|| {
            project_replacement.map(|(path, game_gear_no_save)| {
                TasEditorHostRequest::ReplaceProject {
                    path,
                    game_gear_no_save,
                }
            })
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
