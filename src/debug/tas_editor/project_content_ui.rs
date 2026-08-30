use super::*;

pub(super) fn draw_project_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    live_action: &mut Option<TasEditorLiveAction>,
    body_height: f32,
) {
    let session = state.session.as_ref().expect("open session checked above");
    let cursor = session.cursor();
    let frame_count = session.selected_branch().frame_count();
    let generation = session.project().edit_generation();
    let rerecord_count = session.project().rerecord_count();
    let dirty = session.is_dirty();
    let source = source_label(session.source());
    let manual_path = session.manual_path();
    let project_name = manual_path.file_name().map_or_else(
        || manual_path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.strong(project_name)
            .on_hover_text(manual_path.display().to_string());
        ui.separator();
        ui.label(if dirty {
            "Unsaved changes — Save"
        } else {
            "Saved"
        });
        ui.separator();
        ui.label(format!(
            "Selected frame {cursor}; {frame_count} input frames"
        ));
        ui.small(format!(
            "{source}; gen {generation}; rerecords {rerecord_count}"
        ));
    });

    let selected_input_range =
        input_clipboard::selected_input_range(session, &mut state.input_clipboard);
    if state.recording.is_some() {
        ui.separator();
        recording::draw_recording_strip(
            ui,
            state.recording.as_ref(),
            !state.live_status.locks_editor(),
            actions,
        );
    }
    ui.separator();
    let timeline_height = timeline_height(body_height);
    if ui.available_width() >= TWO_PANE_MIN_WIDTH {
        let spacing = ui.spacing().item_spacing.x;
        let sidebar_width = 330.0_f32.min(ui.available_width() * 0.38);
        let timeline_width = ui.available_width() - sidebar_width - spacing;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(timeline_width, timeline_height + 92.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add_enabled_ui(!state.live_status.locks_editor(), |ui| {
                        draw_timeline_editor(
                            ui,
                            session,
                            actions,
                            TimelinePanelContext {
                                selected_input_range,
                                height: timeline_height,
                                follow_cursor: state.recording.is_some()
                                    || state.live_status.follows_cursor(),
                            },
                            &mut state.neutral_insert_count,
                        );
                    });
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(sidebar_width, timeline_height + 92.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    draw_playback_and_save(
                        ui,
                        PlaybackPanelContext {
                            cursor,
                            frame_count,
                            dirty,
                            recording: state.recording.is_some(),
                        },
                        PlaybackPanelControls {
                            availability: &state.execution_availability,
                            preview: &mut state.execution_preview,
                            live_status: &state.live_status,
                            live_action,
                            private_execution_allowed: !state.live_status.holds_authority(),
                            live_execution_allowed: state.recording.is_none(),
                            actions,
                        },
                    );
                },
            );
        });
    } else {
        ui.add_enabled_ui(!state.live_status.locks_editor(), |ui| {
            draw_timeline_editor(
                ui,
                session,
                actions,
                TimelinePanelContext {
                    selected_input_range,
                    height: timeline_height,
                    follow_cursor: state.recording.is_some() || state.live_status.follows_cursor(),
                },
                &mut state.neutral_insert_count,
            );
        });
        ui.separator();
        draw_playback_and_save(
            ui,
            PlaybackPanelContext {
                cursor,
                frame_count,
                dirty,
                recording: state.recording.is_some(),
            },
            PlaybackPanelControls {
                availability: &state.execution_availability,
                preview: &mut state.execution_preview,
                live_status: &state.live_status,
                live_action,
                private_execution_allowed: !state.live_status.holds_authority(),
                live_execution_allowed: state.recording.is_none(),
                actions,
            },
        );
    }

    ui.add_enabled_ui(
        !state.live_status.holds_authority() && state.recording.is_none(),
        |ui| {
            ui.collapsing("Branches, markers, and advanced editing", |ui| {
                ui.collapsing("Manual row entry", |ui| {
                    recording::draw_recording_strip(ui, None, true, actions);
                });
                ui.collapsing("Create branch from cursor", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("ID");
                        ui.text_edit_singleline(&mut state.new_branch_id);
                        ui.label("Name");
                        ui.text_edit_singleline(&mut state.new_branch_name);
                        if ui.button("Create and select").clicked() {
                            actions.push(TasEditorAction::ForkBranch {
                                id: state.new_branch_id.clone(),
                                name: state.new_branch_name.clone(),
                            });
                        }
                    });
                });
                metadata_editor::draw_metadata_editor(
                    ui,
                    session,
                    &mut state.metadata_editor,
                    actions,
                );
                special_input_editor::draw_special_input_editor(ui, session, actions);
                event_editor::draw_event_editor(ui, session, &mut state.event_editor, actions);
                branch_diff_editor::draw_branch_diff_editor(
                    ui,
                    session,
                    &mut state.branch_diff_editor,
                    actions,
                );
                input_clipboard::draw_input_clipboard(
                    ui,
                    session,
                    &mut state.input_clipboard,
                    actions,
                );
            });
        },
    );
}
