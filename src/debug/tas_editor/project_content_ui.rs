use super::*;

pub(super) fn draw_project_content(
    ui: &mut egui::Ui,
    state: &mut TasEditorWindowState,
    actions: &mut Vec<TasEditorAction>,
    live_action: &mut Option<TasEditorLiveAction>,
) {
    let follow_selection = std::mem::take(&mut state.timeline_follow_selection);
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
    let recording_input_summary = match session.project().identity().system.as_str() {
        "coleco" | "colecovision" => "P1/P2 directions, side buttons, and keypad keys",
        "sms" | "master_system" | "sg" | "sg1000" | "sg-1000" => "P1/P2 directions and 1/2 buttons",
        "ws" | "wonderswan" => {
            "P1 X/Y controls and A/B/Start; input follows the displayed cartridge orientation"
        }
        _ => "the declared controller inputs",
    };

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.strong(project_name)
            .on_hover_text(manual_path.display().to_string());
        ui.separator();
        ui.strong(state.live_status.primary_label());
        ui.separator();
        ui.label(if dirty {
            "Unsaved changes — Save"
        } else {
            "Saved"
        });
        ui.separator();
        if cursor == frame_count {
            ui.label(format!("End · before input frame {frame_count}"));
        } else {
            ui.label(format!(
                "Selected input frame {cursor}; {frame_count} input frames"
            ));
        }
        ui.small(format!(
            "Branch: {}; {source}; gen {generation}; rerecords {rerecord_count}",
            session.selected_branch().name(),
        ));
    });
    if state.live_status.is_linked() {
        ui.small("The timeline and loaded game are one TAS-controlled session.");
    } else {
        ui.small(
            "Each timeline row is one emulated frame. Record captures neutral input automatically.",
        );
    }

    live_execution_ui::draw_live_transport_strip(
        ui,
        live_execution_ui::LiveTransportStripControls {
            status: &state.live_status,
            cursor,
            frame_count,
            enabled: state.recording.is_none(),
            action: live_action,
        },
    );

    let input_selection = state.timeline_selection.snapshot(session);
    let selected_input_range = input_selection
        .as_ref()
        .map(|selection| (selection.start, selection.end));
    let go_to_selection_available = selected_input_range.is_some()
        && state
            .live_status
            .execution_boundary()
            .is_some_and(|boundary| boundary != cursor);
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
    let timeline_height = timeline_height(ui.available_height());
    if uses_two_pane_layout(ui.available_width()) {
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
                                execution_boundary: state.live_status.execution_boundary(),
                                height: timeline_height,
                                follow_cursor: state.recording.is_some()
                                    || state.live_status.follows_cursor()
                                    || follow_selection,
                                go_to_selection_available,
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
                    egui::ScrollArea::vertical()
                        .id_salt("tas_editor_wide_inspector")
                        .max_height(timeline_height + 92.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            inspector_ui::draw_wide(
                                ui,
                                &mut inspector_ui::InspectorDrawContext {
                                    session,
                                    live_status: &state.live_status,
                                    recording_stopped: state.recording.is_none(),
                                    selection: input_selection.as_ref(),
                                    actions,
                                },
                                &mut inspector_ui::InspectorState {
                                    tab: &mut state.inspector_tab,
                                    new_branch_id: &mut state.new_branch_id,
                                    new_branch_name: &mut state.new_branch_name,
                                    active_branch_name: &mut state.active_branch_name,
                                    metadata_editor: &mut state.metadata_editor,
                                    event_editor: &mut state.event_editor,
                                    branch_diff_editor: &mut state.branch_diff_editor,
                                    input_clipboard: &mut state.input_clipboard,
                                },
                            );
                            ui.separator();
                            draw_playback_and_save(
                                ui,
                                PlaybackPanelContext {
                                    cursor,
                                    frame_count,
                                    dirty,
                                    recording: state.recording.is_some(),
                                    recording_input_summary,
                                },
                                PlaybackPanelControls {
                                    availability: &state.execution_availability,
                                    preview: &mut state.execution_preview,
                                    live_status: &state.live_status,
                                    live_recording_mode: &mut state.live_recording_mode,
                                    live_action,
                                    private_execution_allowed: !state.live_status.holds_authority(),
                                    live_execution_allowed: state.recording.is_none(),
                                    actions,
                                },
                            );
                        });
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
                    execution_boundary: state.live_status.execution_boundary(),
                    height: timeline_height,
                    follow_cursor: state.recording.is_some()
                        || state.live_status.follows_cursor()
                        || follow_selection,
                    go_to_selection_available,
                },
                &mut state.neutral_insert_count,
            );
        });
        ui.separator();
        inspector_ui::draw_compact_drawer(
            ui,
            &mut inspector_ui::InspectorDrawContext {
                session,
                live_status: &state.live_status,
                recording_stopped: state.recording.is_none(),
                selection: input_selection.as_ref(),
                actions,
            },
            &mut inspector_ui::InspectorState {
                tab: &mut state.inspector_tab,
                new_branch_id: &mut state.new_branch_id,
                new_branch_name: &mut state.new_branch_name,
                active_branch_name: &mut state.active_branch_name,
                metadata_editor: &mut state.metadata_editor,
                event_editor: &mut state.event_editor,
                branch_diff_editor: &mut state.branch_diff_editor,
                input_clipboard: &mut state.input_clipboard,
            },
        );
        ui.separator();
        draw_playback_and_save(
            ui,
            PlaybackPanelContext {
                cursor,
                frame_count,
                dirty,
                recording: state.recording.is_some(),
                recording_input_summary,
            },
            PlaybackPanelControls {
                availability: &state.execution_availability,
                preview: &mut state.execution_preview,
                live_status: &state.live_status,
                live_recording_mode: &mut state.live_recording_mode,
                live_action,
                private_execution_allowed: !state.live_status.holds_authority(),
                live_execution_allowed: state.recording.is_none(),
                actions,
            },
        );
    }
}

pub(super) fn uses_two_pane_layout(available_width: f32) -> bool {
    available_width >= TWO_PANE_MIN_WIDTH
}

#[cfg(test)]
mod tests {
    use super::uses_two_pane_layout;
    use crate::debug::tas_editor::TWO_PANE_MIN_WIDTH;

    #[test]
    fn compact_layout_stays_timeline_first_until_the_inspector_fits() {
        assert!(!uses_two_pane_layout(TWO_PANE_MIN_WIDTH - 1.0));
        assert!(uses_two_pane_layout(TWO_PANE_MIN_WIDTH));
    }
}
