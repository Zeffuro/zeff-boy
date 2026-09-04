use super::*;

pub(super) struct InspectorState<'a> {
    pub(super) tab: &'a mut TasInspectorTab,
    pub(super) new_branch_id: &'a mut String,
    pub(super) new_branch_name: &'a mut String,
    pub(super) active_branch_name: &'a mut String,
    pub(super) metadata_editor: &'a mut metadata_editor::TasMetadataEditorState,
    pub(super) event_editor: &'a mut event_editor::TasEventEditorState,
    pub(super) branch_diff_editor: &'a mut branch_diff_editor::TasBranchDiffEditorState,
    pub(super) input_clipboard: &'a mut input_clipboard::TasInputClipboardState,
}

pub(super) struct InspectorDrawContext<'a> {
    pub(super) session: &'a TasEditorSession,
    pub(super) live_status: &'a TasEditorLiveStatus,
    pub(super) recording_stopped: bool,
    pub(super) selection: Option<&'a timeline_selection::TasInputSelection>,
    pub(super) actions: &'a mut Vec<TasEditorAction>,
}

pub(super) fn draw_wide(
    ui: &mut egui::Ui,
    context: &mut InspectorDrawContext<'_>,
    state: &mut InspectorState<'_>,
) {
    ui.strong("Selection");
    draw_selection_summary(ui, context.session, context.selection);
    ui.separator();
    draw_tabs(ui, state.tab);
    ui.separator();
    draw_tab_content(ui, context, state);
}

pub(super) fn draw_compact_drawer(
    ui: &mut egui::Ui,
    context: &mut InspectorDrawContext<'_>,
    state: &mut InspectorState<'_>,
) {
    ui.collapsing("Branch, markers & more", |ui| {
        draw_selection_summary(ui, context.session, context.selection);
        ui.separator();
        draw_tabs(ui, state.tab);
        ui.separator();
        draw_tab_content(ui, context, state);
    });
}

fn draw_selection_summary(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    selection: Option<&timeline_selection::TasInputSelection>,
) {
    match selection {
        Some(selection) => ui.label(format!(
            "Frames {}–{} ({} selected)",
            selection.start,
            selection.end - 1,
            selection.end - selection.start
        )),
        None => ui.label(format!("Cursor before input frame {}", session.cursor())),
    };
    ui.small("Use the timeline to edit, insert, delete, or move the selection.");
}

fn draw_tabs(ui: &mut egui::Ui, tab: &mut TasInspectorTab) {
    ui.horizontal_wrapped(|ui| {
        for candidate in [
            TasInspectorTab::Branches,
            TasInspectorTab::Markers,
            TasInspectorTab::Tools,
        ] {
            if ui
                .selectable_label(*tab == candidate, candidate.label())
                .clicked()
            {
                *tab = candidate;
            }
        }
    });
}

fn draw_tab_content(
    ui: &mut egui::Ui,
    context: &mut InspectorDrawContext<'_>,
    state: &mut InspectorState<'_>,
) {
    match *state.tab {
        TasInspectorTab::Branches => {
            ui.add_enabled_ui(!context.live_status.locks_editor(), |ui| {
                branch_navigator::draw(
                    ui,
                    context.session,
                    context.live_status,
                    context.recording_stopped,
                    state.active_branch_name,
                    context.actions,
                );
                branch_navigator::draw_advanced_controls(
                    ui,
                    context.session,
                    context.live_status,
                    state.new_branch_id,
                    state.new_branch_name,
                    context.actions,
                );
            });
        }
        TasInspectorTab::Markers => {
            ui.add_enabled_ui(!context.live_status.holds_authority(), |ui| {
                metadata_editor::draw_metadata_editor(
                    ui,
                    context.session,
                    state.metadata_editor,
                    context.actions,
                );
            });
        }
        TasInspectorTab::Tools => {
            ui.add_enabled_ui(!context.live_status.holds_authority(), |ui| {
                ui.collapsing("Manual row entry", |ui| {
                    recording::draw_recording_strip(ui, None, true, context.actions);
                });
                special_input_editor::draw_special_input_editor(
                    ui,
                    context.session,
                    context.actions,
                );
                event_editor::draw_event_editor(
                    ui,
                    context.session,
                    state.event_editor,
                    context.actions,
                );
                branch_diff_editor::draw_branch_diff_editor(
                    ui,
                    context.session,
                    state.branch_diff_editor,
                    context.actions,
                );
                input_clipboard::draw_input_clipboard(
                    ui,
                    context.session,
                    state.input_clipboard,
                    context.selection,
                    context.actions,
                );
            });
        }
    }
}

impl TasInspectorTab {
    const fn label(self) -> &'static str {
        match self {
            Self::Branches => "Branches",
            Self::Markers => "Markers",
            Self::Tools => "Tools",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TasInspectorTab;

    #[test]
    fn inspector_tabs_match_the_wide_and_compact_navigation_order() {
        assert_eq!(
            [
                TasInspectorTab::Branches.label(),
                TasInspectorTab::Markers.label(),
                TasInspectorTab::Tools.label(),
            ],
            ["Branches", "Markers", "Tools"]
        );
    }

    #[test]
    fn branches_are_the_default_inspector_tab() {
        assert_eq!(TasInspectorTab::default(), TasInspectorTab::Branches);
    }
}
