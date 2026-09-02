use super::*;

pub(super) fn draw_embedded_window(
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
        if state.verified_export_busy {
            state.open = true;
            return None;
        }
        state.queue_return_to_game_unchanged();
        return state.take_pending_host_request();
    }
    host_request.or_else(|| state.take_pending_host_request())
}

#[derive(Clone, Copy)]
pub(super) struct PlaybackPanelContext {
    pub(super) cursor: u64,
    pub(super) frame_count: u64,
    pub(super) dirty: bool,
    pub(super) recording: bool,
    pub(super) recording_input_summary: &'static str,
}

pub(super) struct PlaybackPanelControls<'a> {
    pub(super) availability: &'a TasEditorExecutionAvailability,
    pub(super) preview: &'a mut execution_preview::TasEditorExecutionPreview,
    pub(super) live_status: &'a TasEditorLiveStatus,
    pub(super) live_recording_mode: &'a mut TasLiveRecordingMode,
    pub(super) live_action: &'a mut Option<TasEditorLiveAction>,
    pub(super) private_execution_allowed: bool,
    pub(super) live_execution_allowed: bool,
    pub(super) actions: &'a mut Vec<TasEditorAction>,
}

#[derive(Clone, Copy)]
pub(super) struct TimelinePanelContext {
    pub(super) selected_input_range: Option<(u64, u64)>,
    pub(super) execution_boundary: Option<u64>,
    pub(super) height: f32,
    pub(super) follow_cursor: bool,
    pub(super) go_to_selection_available: bool,
}

pub(super) fn draw_timeline_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
    context: TimelinePanelContext,
    neutral_insert_count: &mut u64,
) {
    let cursor = session.cursor();
    let frame_count = session.selected_branch().frame_count();
    let selection = context.selected_input_range;
    let selection_summary = selection.map(|(start, end)| {
        format!(
            "Selection: frames {start}–{} ({} frames)",
            end - 1,
            end - start
        )
    });

    ui.horizontal_wrapped(|ui| {
        ui.heading("Input timeline");
        ui.small("Each row is the input for one emulated frame.");
        if let Some(boundary) = context.execution_boundary {
            ui.separator();
            ui.strong(format!("▶ Before input frame {boundary}"));
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Select End").clicked() {
            actions.push(TasEditorAction::SelectCursor(frame_count));
        }
        if context
            .execution_boundary
            .is_some_and(|boundary| boundary != cursor)
            && ui
                .button("Show Game Position")
                .on_hover_text(
                    "Select and show the linked game's current frame boundary without moving it",
                )
                .clicked()
        {
            actions.push(TasEditorAction::SelectLiveExecutionBoundary);
        }
        ui.separator();
        ui.strong(selection_summary.as_deref().unwrap_or("Selection: none"));
        if ui
            .add_enabled(selection.is_some(), egui::Button::new("Collapse selection"))
            .clicked()
        {
            actions.push(TasEditorAction::ClearTimelineSelection);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let insert_cursor = selection.map_or(cursor, |(start, _)| start);
        ui.label("Insert neutral frames");
        ui.add(
            egui::DragValue::new(neutral_insert_count)
                .range(1..=3_600)
                .speed(1),
        );
        if ui.button("Insert Neutral Frames").clicked() {
            actions.push(TasEditorAction::InsertNeutralFrames {
                cursor: insert_cursor,
                count: *neutral_insert_count,
            });
        }
        ui.small(if selection.is_some() {
            "at the start of the selection"
        } else {
            "at the selected cursor"
        });
        let delete_label = selection.map_or_else(
            || "Delete Frames".to_owned(),
            |(start, end)| format!("Delete {} Frames", end - start),
        );
        if ui
            .add_enabled(selection.is_some(), egui::Button::new(delete_label))
            .clicked()
        {
            let (start, end) = selection.expect("delete action requires a selection");
            actions.push(TasEditorAction::DeleteFrames {
                start,
                count: end - start,
            });
        }
        ui.small("Record adds neutral frames automatically.");
    });
    if selection.is_none() {
        ui.small(
            "Click or drag frame numbers to select. Shift extends; ↑/↓ and Home/End navigate. Double-click moves a linked game when available.",
        );
    }
    timeline::draw_timeline(
        ui,
        session,
        actions,
        timeline::TimelineView {
            selected_input_range: context.selected_input_range,
            execution_boundary: context.execution_boundary,
            max_height: context.height,
            follow_cursor: context.follow_cursor,
            go_to_selection_available: context.go_to_selection_available,
        },
    );
}

pub(super) fn draw_playback_and_save(
    ui: &mut egui::Ui,
    context: PlaybackPanelContext,
    controls: PlaybackPanelControls<'_>,
) {
    ui.add_enabled_ui(controls.live_execution_allowed, |ui| {
        live_execution_ui::draw_live_execution_panel(
            ui,
            live_execution_ui::LiveExecutionPanelControls {
                status: controls.live_status,
                cursor: context.cursor,
                frame_count: context.frame_count,
                recording_input_summary: context.recording_input_summary,
                recording_mode: controls.live_recording_mode,
                action: controls.live_action,
                actions: controls.actions,
            },
        );
    });
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
    ui.add_sized(
        [ui.available_width(), ui.spacing().interact_size.y],
        egui::Label::new(egui::RichText::new(message).color(color))
            .truncate()
            .sense(egui::Sense::hover()),
    )
    .on_hover_text(message);
}

pub(super) fn timeline_height(body_height: f32) -> f32 {
    (body_height - 150.0).clamp(MIN_TIMELINE_HEIGHT, MAX_TIMELINE_HEIGHT)
}
