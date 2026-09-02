use super::*;

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_request: &mut Option<TasEditorFileRequest>,
    project_replacement: &mut Option<(PathBuf, bool)>,
    live_action: &mut Option<TasEditorLiveAction>,
) {
    let editor_locked = state.live_status.locks_editor() || state.verified_export_busy;
    let file_actions_locked = state.live_status.holds_authority()
        || state.recording.is_some()
        || state.verified_export_busy;
    if state.presentation == TasEditorPresentation::SeparateWindow {
        draw_standalone_menu(
            ui,
            state,
            actions,
            file_request,
            live_action,
            file_actions_locked,
        );
    } else {
        draw_embedded_command_strip(ui, state, actions, file_request, file_actions_locked);
    }
    workflow_ui::draw_pending_file_request(
        ui,
        state.pending_file_request,
        !file_actions_locked,
        actions,
    );
    if let Some(confirmed) = workflow_ui::draw_project_replacement_confirmation(
        ui,
        state
            .pending_project_replacement
            .as_ref()
            .map(|(path, _)| path.as_path()),
    ) {
        let replacement = state
            .pending_project_replacement
            .take()
            .expect("replacement confirmation has a project path");
        if confirmed {
            *project_replacement = Some(replacement);
        }
    }
    if let Some(confirmed) = workflow_ui::draw_game_gear_no_save_confirmation(
        ui,
        state.pending_game_gear_no_save_confirmation,
    ) {
        state.pending_game_gear_no_save_confirmation = false;
        if confirmed {
            *file_request = Some(TasEditorFileRequest::NewGameGearNoSaveProject);
        }
    }

    if state.presentation == TasEditorPresentation::Embedded {
        draw_embedded_recovery(ui, state, actions, file_actions_locked);
    }
    workflow_ui::draw_autosave_recovery_confirmation(
        ui,
        state.pending_autosave_recovery,
        !file_actions_locked,
        actions,
    );

    queue_undo_redo_shortcuts(ui, state, actions, file_actions_locked);
    draw_status_message(ui, state.message.as_ref());
    if state.verified_export_busy {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            state
                .verified_export_status()
                .unwrap_or("Verified replay export is running"),
        );
        ui.horizontal_wrapped(|ui| {
            ui.small("The project is temporarily locked.");
            if state.verified_export_cancel_requested() {
                ui.small("Cancellation will stop before any remaining publication.");
            } else if ui.button("Cancel export").clicked() {
                state.request_verified_export_cancellation();
            }
        });
    }

    if state.session.is_none() {
        workflow_ui::draw_empty_project_state(
            ui,
            &state.execution_availability,
            !editor_locked,
            file_request,
        );
        return;
    };

    match project_content_render_policy(ui.available_width()) {
        TasProjectContentRenderPolicy::FixedWide => {
            project_content_ui::draw_project_content(ui, state, actions, live_action);
        }
        TasProjectContentRenderPolicy::ScrollableCompact => {
            draw_scrollable_project_content(ui, state, actions, live_action);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TasProjectContentRenderPolicy {
    FixedWide,
    ScrollableCompact,
}

pub(super) fn project_content_render_policy(available_width: f32) -> TasProjectContentRenderPolicy {
    if project_content_ui::uses_two_pane_layout(available_width) {
        TasProjectContentRenderPolicy::FixedWide
    } else {
        TasProjectContentRenderPolicy::ScrollableCompact
    }
}

fn draw_embedded_command_strip(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_request: &mut Option<TasEditorFileRequest>,
    file_actions_locked: bool,
) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!file_actions_locked, egui::Button::new("Open TAS…"))
            .clicked()
        {
            *file_request = state.begin_file_request(TasEditorFileRequest::OpenProject);
        }
        if ui
            .add_enabled(
                !file_actions_locked,
                egui::Button::new("New TAS from Loaded Game…"),
            )
            .clicked()
        {
            *file_request = state.begin_file_request(TasEditorFileRequest::NewProject);
        }
        if ui
            .add_enabled(
                !file_actions_locked,
                egui::Button::new("Import Replay as TAS…"),
            )
            .on_hover_text("Convert a .zrpl replay using the loaded matching game")
            .clicked()
        {
            *file_request = state.begin_file_request(TasEditorFileRequest::ImportReplay);
        }
        let loaded = state.session.is_some();
        if ui
            .add_enabled(loaded && !file_actions_locked, egui::Button::new("Save"))
            .on_hover_text("Write this project to its .ztas file")
            .clicked()
        {
            actions.push(TasEditorAction::SaveManual);
        }
        if ui
            .add_enabled(
                loaded && !file_actions_locked,
                egui::Button::new("Export Verified Replay…"),
            )
            .on_hover_text("Verify the active branch, save its proof, and export a .zrpl")
            .clicked()
        {
            *file_request = Some(TasEditorFileRequest::ExportReplay);
        }
        draw_undo_redo_buttons(ui, state, actions, file_actions_locked);
    });
}

fn draw_embedded_recovery(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_actions_locked: bool,
) {
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
}

fn draw_standalone_menu(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_request: &mut Option<TasEditorFileRequest>,
    live_action: &mut Option<TasEditorLiveAction>,
    file_actions_locked: bool,
) {
    let loaded = state.session.is_some();
    let selection_allowed = loaded && !state.live_status.locks_editor();
    let can_undo = state
        .session
        .as_ref()
        .is_some_and(TasEditorSession::can_undo);
    let can_redo = state
        .session
        .as_ref()
        .is_some_and(TasEditorSession::can_redo);
    let cursor = state.session.as_ref().map_or(0, TasEditorSession::cursor);
    let frame_count = state
        .session
        .as_ref()
        .map_or(0, |session| session.selected_branch().frame_count());
    let selection = state
        .session
        .as_ref()
        .and_then(|session| state.timeline_selection.selected_range(session));
    let insert_cursor = selection.map_or(cursor, |(start, _)| start);
    let linked_boundary = state.live_status.execution_boundary();
    let can_create_branch = state.recording.is_none()
        && state.session.as_ref().is_some_and(|session| {
            branch_navigator::branch_creation_enabled(session, &state.live_status)
        });

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui
                .add_enabled(
                    !file_actions_locked,
                    egui::Button::new("New TAS from Loaded Game…"),
                )
                .clicked()
            {
                *file_request = state.begin_file_request(TasEditorFileRequest::NewProject);
                ui.close();
            }
            if ui
                .add_enabled(!file_actions_locked, egui::Button::new("Open TAS…"))
                .clicked()
            {
                *file_request = state.begin_file_request(TasEditorFileRequest::OpenProject);
                ui.close();
            }
            if ui
                .add_enabled(
                    !file_actions_locked,
                    egui::Button::new("Import Replay as TAS…"),
                )
                .on_hover_text("Convert a .zrpl replay using the loaded matching game")
                .clicked()
            {
                *file_request = state.begin_file_request(TasEditorFileRequest::ImportReplay);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(loaded && !file_actions_locked, egui::Button::new("Save"))
                .clicked()
            {
                actions.push(TasEditorAction::SaveManual);
                ui.close();
            }
            if ui
                .add_enabled(
                    loaded && !file_actions_locked,
                    egui::Button::new("Export Active Branch as Verified Replay…"),
                )
                .on_hover_text("Verify the active branch, save its proof, and export a .zrpl")
                .clicked()
            {
                *file_request = Some(TasEditorFileRequest::ExportReplay);
                ui.close();
            }
            if ui
                .add_enabled(
                    loaded && !file_actions_locked,
                    egui::Button::new("Autosave now"),
                )
                .clicked()
            {
                actions.push(TasEditorAction::Autosave);
                ui.close();
            }
            if ui
                .add_enabled(
                    loaded && !file_actions_locked,
                    egui::Button::new("Recover newest autosave"),
                )
                .clicked()
            {
                state.pending_autosave_recovery = true;
                ui.close();
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(!file_actions_locked && can_undo, egui::Button::new("Undo"))
                .clicked()
            {
                actions.push(TasEditorAction::Undo);
                ui.close();
            }
            if ui
                .add_enabled(!file_actions_locked && can_redo, egui::Button::new("Redo"))
                .clicked()
            {
                actions.push(TasEditorAction::Redo);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(selection_allowed, egui::Button::new("Select All Frames"))
                .clicked()
            {
                actions.push(TasEditorAction::SelectAllTimelineFrames);
                ui.close();
            }
            if ui
                .add_enabled(
                    loaded && !file_actions_locked,
                    egui::Button::new("Insert Neutral Frames"),
                )
                .clicked()
            {
                actions.push(TasEditorAction::InsertNeutralFrames {
                    cursor: insert_cursor,
                    count: state.neutral_insert_count,
                });
                ui.close();
            }
            if ui
                .add_enabled(
                    selection.is_some() && !file_actions_locked,
                    egui::Button::new("Delete Selected Frames"),
                )
                .clicked()
            {
                let (start, end) = selection.expect("delete requires a selected range");
                actions.push(TasEditorAction::DeleteFrames {
                    start,
                    count: end - start,
                });
                ui.close();
            }
        });
        ui.menu_button("Movie", |ui| {
            if ui
                .add_enabled(selection_allowed, egui::Button::new("Select Start"))
                .clicked()
            {
                actions.push(TasEditorAction::SelectCursor(0));
                ui.close();
            }
            if ui
                .add_enabled(selection_allowed, egui::Button::new("Select End"))
                .clicked()
            {
                actions.push(TasEditorAction::SelectCursor(frame_count));
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    can_create_branch,
                    egui::Button::new(format!("Create Branch at Frame {cursor}")),
                )
                .on_hover_text("Create and select a new route; the current branch is unchanged")
                .clicked()
            {
                actions.push(branch_navigator::generated_branch_action(
                    state
                        .session
                        .as_ref()
                        .expect("loaded project checked above"),
                ));
                ui.close();
            }
            if let Some(linked_boundary) = linked_boundary {
                ui.separator();
                if ui
                    .add_enabled(
                        linked_boundary != cursor,
                        egui::Button::new("Go to Selection"),
                    )
                    .clicked()
                {
                    *live_action = Some(TasEditorLiveAction::GoToSelection);
                    ui.close();
                }
            }
        });
    });
}

fn draw_undo_redo_buttons(
    ui: &mut egui::Ui,
    state: &TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_actions_locked: bool,
) {
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
}

fn queue_undo_redo_shortcuts(
    ui: &egui::Ui,
    state: &TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    file_actions_locked: bool,
) {
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
}

pub(super) fn draw_scrollable_project_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    live_action: &mut Option<TasEditorLiveAction>,
) -> egui::scroll_area::ScrollAreaOutput<()> {
    egui::ScrollArea::vertical()
        .id_salt("tas_editor_body")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            project_content_ui::draw_project_content(ui, state, actions, live_action);
        })
}

#[cfg(test)]
mod tests {
    use super::{TasProjectContentRenderPolicy, project_content_render_policy};
    use crate::debug::tas_editor::TWO_PANE_MIN_WIDTH;

    #[test]
    fn project_content_only_scrolls_when_the_compact_layout_is_active() {
        assert_eq!(
            project_content_render_policy(TWO_PANE_MIN_WIDTH - 1.0),
            TasProjectContentRenderPolicy::ScrollableCompact
        );
        assert_eq!(
            project_content_render_policy(TWO_PANE_MIN_WIDTH),
            TasProjectContentRenderPolicy::FixedWide
        );
    }
}
