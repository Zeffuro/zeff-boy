use super::*;

#[derive(Clone, Copy)]
pub(super) struct PlaybackPanelContext {
    pub(super) cursor: u64,
    pub(super) frame_count: u64,
    pub(super) dirty: bool,
    pub(super) recording: bool,
}

pub(super) struct PlaybackPanelControls<'a> {
    pub(super) availability: &'a TasEditorExecutionAvailability,
    pub(super) preview: &'a mut execution_preview::TasEditorExecutionPreview,
    pub(super) live_status: &'a TasEditorLiveStatus,
    pub(super) live_action: &'a mut Option<TasEditorLiveAction>,
    pub(super) private_execution_allowed: bool,
    pub(super) live_execution_allowed: bool,
    pub(super) actions: &'a mut Vec<TasEditorAction>,
}

#[derive(Clone, Copy)]
pub(super) struct TimelinePanelContext {
    pub(super) selected_input_range: Option<(u64, u64)>,
    pub(super) height: f32,
    pub(super) follow_cursor: bool,
}

pub(super) fn draw_timeline_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
    context: TimelinePanelContext,
    neutral_insert_count: &mut u64,
) {
    let branches = session
        .project()
        .branches()
        .iter()
        .map(|branch| (branch.id().to_owned(), branch.name().to_owned()))
        .collect::<Vec<_>>();
    let selected_branch_id = session.selected_branch_id();
    let cursor = session.cursor();
    let frame_count = session.selected_branch().frame_count();

    ui.horizontal_wrapped(|ui| {
        ui.heading("Input timeline");
        ui.small("Select a frame, then click cells to toggle inputs.");
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Branch");
        egui::ComboBox::from_id_salt("tas_editor_branch")
            .selected_text(session.selected_branch().name())
            .show_ui(ui, |ui| {
                for (id, name) in &branches {
                    if ui
                        .selectable_label(id == selected_branch_id, name)
                        .clicked()
                    {
                        actions.push(TasEditorAction::SelectBranch(id.clone()));
                    }
                }
            });
        if ui
            .add_enabled(cursor < frame_count, egui::Button::new("Delete frame"))
            .clicked()
        {
            actions.push(TasEditorAction::DeleteFrame(cursor));
        }
        if ui.button("Jump to end").clicked() {
            actions.push(TasEditorAction::SelectCursor(frame_count));
        }
        ui.menu_button("Bulk neutral wait…", |ui| {
            ui.label("Optional: insert many no-input frames at once.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(neutral_insert_count)
                        .range(1..=3_600)
                        .speed(1),
                );
                if ui.button("Insert frames").clicked() {
                    actions.push(TasEditorAction::InsertNeutralFrames {
                        cursor,
                        count: *neutral_insert_count,
                    });
                    ui.close();
                }
            });
        });
    });
    timeline::draw_timeline(
        ui,
        session,
        actions,
        context.selected_input_range,
        context.height,
        context.follow_cursor,
    );
}

pub(super) fn draw_playback_and_save(
    ui: &mut egui::Ui,
    context: PlaybackPanelContext,
    controls: PlaybackPanelControls<'_>,
) {
    if controls.private_execution_allowed {
        execution_preview::draw_execution_panel(
            ui,
            controls.availability,
            context.cursor,
            context.frame_count,
            controls.preview,
            controls.actions,
        );
    } else {
        execution_preview::draw_linked_frame(ui, controls.preview);
    }
    ui.add_enabled_ui(controls.live_execution_allowed, |ui| {
        live_execution_ui::draw_live_execution_panel(
            ui,
            controls.live_status,
            context.cursor,
            context.frame_count,
            controls.live_action,
        );
    });
    ui.group(|ui| {
        ui.strong(if context.dirty {
            "Unsaved changes"
        } else {
            "Project saved"
        });
        if ui
            .add_enabled(
                context.dirty && !context.recording,
                egui::Button::new("Save changes"),
            )
            .on_hover_text("Write this project to its .ztas file")
            .clicked()
        {
            controls.actions.push(TasEditorAction::SaveManual);
        }
        ui.small("Recovery autosaves are kept separately from the project file.");
    });
}

pub(super) fn draw_status_message(ui: &mut egui::Ui, message: Option<&(bool, String)>) {
    let Some((is_error, message)) = message else {
        return;
    };
    let color = if *is_error {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().strong_text_color()
    };
    ui.add(
        egui::Label::new(egui::RichText::new(message).color(color))
            .truncate()
            .sense(egui::Sense::hover()),
    )
    .on_hover_text(message);
}

pub(super) fn timeline_height(body_height: f32) -> f32 {
    (body_height - 150.0).clamp(MIN_TIMELINE_HEIGHT, MAX_TIMELINE_HEIGHT)
}
