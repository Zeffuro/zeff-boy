use std::path::PathBuf;

use super::{TasEditorAction, TasEditorFileRequest, TasLiveRecordingMode};

mod transport;

pub(super) use transport::{LiveTransportStripControls, draw_live_transport_strip};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorLiveStatus {
    Unavailable(String),
    ReloadRequired(String),
    Ready {
        recording_available: bool,
    },
    Acquiring,
    Staging {
        completed: u64,
        total: u64,
    },
    Linked {
        cursor: u64,
        recording_available: bool,
    },
    Playing {
        cursor: u64,
        pause_pending: bool,
    },
    AdvancingFrame,
    Recording,
    Returning,
    Keeping,
    Terminal(String),
}

impl Default for TasEditorLiveStatus {
    fn default() -> Self {
        Self::Unavailable("Live playback is not ready".to_owned())
    }
}

impl TasEditorLiveStatus {
    pub(crate) fn primary_label(&self) -> String {
        match self {
            Self::Unavailable(_) => "Offline".to_owned(),
            Self::ReloadRequired(_) => "Reload required".to_owned(),
            Self::Ready { .. } => "Ready to connect".to_owned(),
            Self::Acquiring => "Connecting…".to_owned(),
            Self::Staging { completed, total } => {
                format!("Connecting… {completed}/{total} frames")
            }
            Self::Linked { cursor, .. } => {
                format!("Connected · paused before input frame {cursor}")
            }
            Self::Playing {
                cursor,
                pause_pending,
            } => {
                if *pause_pending {
                    format!("Pausing at the next boundary after B({cursor})…")
                } else {
                    format!("▶ Playing from B({cursor})")
                }
            }
            Self::AdvancingFrame => "Recording one frame…".to_owned(),
            Self::Recording => "Recording".to_owned(),
            Self::Returning => "Restoring pre-TAS game…".to_owned(),
            Self::Keeping => "Disconnecting…".to_owned(),
            Self::Terminal(_) => "Live session needs attention".to_owned(),
        }
    }

    pub(crate) fn locks_editor(&self) -> bool {
        !matches!(
            self,
            Self::Unavailable(_)
                | Self::ReloadRequired(_)
                | Self::Ready { .. }
                | Self::Linked { .. }
        )
    }

    pub(crate) fn holds_authority(&self) -> bool {
        !matches!(
            self,
            Self::Unavailable(_) | Self::ReloadRequired(_) | Self::Ready { .. }
        )
    }

    pub(crate) fn requires_return_on_close(&self) -> bool {
        matches!(
            self,
            Self::Acquiring
                | Self::Staging { .. }
                | Self::Playing { .. }
                | Self::AdvancingFrame
                | Self::Recording
        )
    }

    pub(crate) fn follows_cursor(&self) -> bool {
        matches!(
            self,
            Self::Playing { .. } | Self::AdvancingFrame | Self::Recording
        )
    }

    pub(crate) fn is_linked(&self) -> bool {
        matches!(self, Self::Linked { .. })
    }

    pub(crate) fn execution_boundary(&self) -> Option<u64> {
        match self {
            Self::Linked { cursor, .. } | Self::Playing { cursor, .. } => Some(*cursor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorLiveAction {
    ReloadLoadedGame,
    StageSelectedInput,
    RecordFromSelectedInput,
    GoToSelection,
    ReconstructAfterEdit { start: u64, end: u64 },
    RecordCurrentInputAndAdvance,
    StartRealtimeRecording,
    StopRealtimeRecording,
    StartPlayback,
    PausePlayback,
    KeepResultAndReturnToGame,
    ReturnToGameUnchanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorHostRequest {
    File(TasEditorFileRequest),
    ReplaceProject {
        path: PathBuf,
        game_gear_no_save: bool,
    },
    Live(TasEditorLiveAction),
}

pub(super) struct LiveExecutionPanelControls<'a> {
    pub(super) status: &'a TasEditorLiveStatus,
    pub(super) cursor: u64,
    pub(super) frame_count: u64,
    pub(super) recording_input_summary: &'a str,
    pub(super) recording_mode: &'a mut TasLiveRecordingMode,
    pub(super) action: &'a mut Option<TasEditorLiveAction>,
    pub(super) actions: &'a mut Vec<TasEditorAction>,
}

pub(super) fn draw_live_execution_panel(
    ui: &mut egui::Ui,
    controls: LiveExecutionPanelControls<'_>,
) {
    let LiveExecutionPanelControls {
        status,
        cursor,
        frame_count,
        recording_input_summary,
        recording_mode,
        action,
        actions,
    } = controls;
    ui.group(|ui| {
        ui.strong("Live TAS session");
        match status {
            TasEditorLiveStatus::Unavailable(reason) => {
                ui.colored_label(ui.visuals().warn_fg_color, reason);
            }
            TasEditorLiveStatus::ReloadRequired(reason) => {
                ui.colored_label(ui.visuals().warn_fg_color, reason);
                if ui.button("Reload Game & Connect").clicked() {
                    *action = Some(TasEditorLiveAction::ReloadLoadedGame);
                }
                ui.small(
                    "The current game is parked unchanged while a matching TAS session is loaded. Disconnect Restore returns to this exact game; Disconnect Keep discards it.",
                );
            }
            TasEditorLiveStatus::Ready {
                recording_available,
            } => {
                let target = selected_input_target(cursor, frame_count);
                if *recording_available {
                    draw_recording_mode_choice(ui, target, frame_count, recording_mode);
                }
                match target {
                    None => ui.small("Select an input frame first."),
                    Some(target) if target == frame_count => ui.small(format!(
                        "Will connect before input frame {frame_count} · End. Record appends from there."
                    )),
                    Some(target) => {
                        ui.small(format!("Will connect before input frame {target}."))
                    }
                };
                if *recording_available {
                    ui.small(format!("Record captures {recording_input_summary}."));
                }
            }
            TasEditorLiveStatus::Acquiring => {
                ui.label("Linking the timeline to the loaded game…");
                return_unchanged_button(ui, action);
            }
            TasEditorLiveStatus::Staging { completed, total } => {
                ui.label(format!("Moving the loaded game: frame {completed} of {total}…"));
                return_unchanged_button(ui, action);
            }
            TasEditorLiveStatus::Linked {
                cursor: linked_cursor,
                recording_available,
            } => {
                ui.strong(format!(
                    "Connected · paused before input frame {linked_cursor}"
                ));
                ui.small(
                    "Selection does not move the loaded game. Editing input rebuilds once to show the changed result.",
                );
                draw_review_controls(
                    ui,
                    *linked_cursor,
                    cursor,
                    frame_count,
                    action,
                    actions,
                );
                if *recording_available {
                    draw_recording_mode_choice(
                        ui,
                        selected_input_target(cursor, frame_count),
                        frame_count,
                        recording_mode,
                    );
                    ui.small(format!(
                        "Play in the game window. {recording_input_summary} and neutral frames are recorded automatically."
                    ));
                }
                if ui.button("Disconnect — Keep Current Game Position").clicked() {
                    *action = Some(TasEditorLiveAction::KeepResultAndReturnToGame);
                }
                return_unchanged_button(ui, action);
                ui.small("Disconnecting does not save the TAS project file.");
            }
            TasEditorLiveStatus::Playing {
                cursor: linked_cursor,
                pause_pending,
            } => {
                if *pause_pending {
                    ui.strong(format!("Pausing after B({linked_cursor})…"));
                    ui.small("The in-flight movie frame will finish at one stable boundary.");
                } else {
                    ui.strong(format!("▶ Playing stored input from B({linked_cursor})"));
                    ui.small("Playback does not sample host controls or change the TAS project.");
                }
            }
            TasEditorLiveStatus::AdvancingFrame => {
                ui.label("Recording the current controls in the loaded game…");
                return_unchanged_button(ui, action);
            }
            TasEditorLiveStatus::Recording => {
                ui.strong("Recording");
                ui.small(match recording_mode {
                    TasLiveRecordingMode::ReplaceExistingInput => {
                        "Existing input is replaced as the game advances; recording appends after End. Neutral input is recorded."
                    }
                    TasLiveRecordingMode::InsertNewFrames => {
                        "New input frames are inserted as the game advances, moving existing input later. Neutral input is recorded."
                    }
                });
                ui.small(format!("Recording captures {recording_input_summary}."));
                ui.small(
                    "Switching windows pauses capture safely. Click Stop Recording before reviewing or creating a branch.",
                );
                return_unchanged_button(ui, action);
                ui.small(
                    "Recorded frames already edit the TAS project. Restoring the pre-TAS game does not remove them.",
                );
            }
            TasEditorLiveStatus::Returning => {
                ui.label("Restoring the game state captured when TAS connected…");
            }
            TasEditorLiveStatus::Keeping => {
                ui.label("Disconnecting the TAS editor…");
            }
            TasEditorLiveStatus::Terminal(reason) => {
                ui.colored_label(ui.visuals().error_fg_color, reason);
            }
        }
    });
}

fn draw_review_controls(
    ui: &mut egui::Ui,
    linked_cursor: u64,
    selected_cursor: u64,
    frame_count: u64,
    action: &mut Option<TasEditorLiveAction>,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.separator();
    ui.strong("Review recording");
    ui.small("Move backward until the game shows the moment you want, then create a branch there.");
    ui.horizontal_wrapped(|ui| {
        for (label, delta) in [
            ("−300 frames", -300),
            ("−60 frames", -60),
            ("−1 frame", -1),
            ("+1 frame", 1),
            ("+60 frames", 60),
            ("+300 frames", 300),
        ] {
            let target = review_jump_target(linked_cursor, delta, frame_count);
            if ui
                .add_enabled(target != linked_cursor, egui::Button::new(label))
                .clicked()
            {
                actions.push(TasEditorAction::SelectCursor(target));
                *action = Some(TasEditorLiveAction::GoToSelection);
            }
        }
    });
    let mut target = selected_cursor;
    if ui
        .add(egui::Slider::new(&mut target, 0..=frame_count).text("Selected position"))
        .changed()
    {
        actions.push(TasEditorAction::SelectCursor(target));
    }
}

pub(super) fn draw_linked_branch_controls(
    ui: &mut egui::Ui,
    linked_cursor: u64,
    selected_cursor: u64,
    new_branch_id: &mut String,
    new_branch_name: &mut String,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.separator();
    ui.group(|ui| {
        ui.strong(format!("New branch before input frame {linked_cursor}"));
        ui.small("Create a new route from the game position currently shown.");
        ui.horizontal_wrapped(|ui| {
            ui.label("ID");
            ui.text_edit_singleline(new_branch_id);
            ui.label("Name (optional)");
            ui.text_edit_singleline(new_branch_name);
        });
        let ready = linked_cursor == selected_cursor && !new_branch_id.trim().is_empty();
        if ui
            .add_enabled(ready, egui::Button::new("Create Branch Here"))
            .on_hover_text("The new branch starts at the game position currently shown")
            .clicked()
        {
            actions.push(TasEditorAction::ForkBranch {
                id: new_branch_id.clone(),
                name: branch_name_or_id(new_branch_id, new_branch_name),
            });
        }
        if linked_cursor != selected_cursor {
            ui.small("Move the game to the selected position before creating the branch.");
        } else if new_branch_id.trim().is_empty() {
            ui.small("Enter a unique branch ID. The display name can be left empty.");
        }
    });
}

pub(super) fn branch_name_or_id(id: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        id.trim().to_owned()
    } else {
        name.to_owned()
    }
}

fn review_jump_target(cursor: u64, delta: i64, frame_count: u64) -> u64 {
    cursor.saturating_add_signed(delta).min(frame_count)
}

fn draw_recording_mode_choice(
    ui: &mut egui::Ui,
    target: Option<u64>,
    frame_count: u64,
    recording_mode: &mut TasLiveRecordingMode,
) {
    if !recording_target_has_future(target, frame_count) {
        return;
    }
    ui.small("Existing input follows this position. Recording mode:");
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(
            recording_mode,
            TasLiveRecordingMode::ReplaceExistingInput,
            "Replace Existing Input",
        );
        ui.radio_value(
            recording_mode,
            TasLiveRecordingMode::InsertNewFrames,
            "Insert New Frames",
        );
    });
}

fn recording_target_has_future(target: Option<u64>, frame_count: u64) -> bool {
    target.is_some_and(|target| target < frame_count)
}

pub(crate) fn selected_input_target(cursor: u64, frame_count: u64) -> Option<u64> {
    if frame_count > crate::tas_project::MAX_PROJECT_FRAMES || cursor > frame_count {
        return None;
    }
    Some(cursor)
}

pub(crate) fn can_stage_selected_input(cursor: u64, frame_count: u64) -> bool {
    selected_input_target(cursor, frame_count).is_some()
}

fn can_record_current_input(cursor: u64, frame_count: u64) -> bool {
    cursor <= frame_count && frame_count < crate::tas_project::MAX_PROJECT_FRAMES
}

fn return_unchanged_button(ui: &mut egui::Ui, action: &mut Option<TasEditorLiveAction>) {
    if ui
        .button("Disconnect — Restore Game to Before TAS Connected")
        .clicked()
    {
        *action = Some(TasEditorLiveAction::ReturnToGameUnchanged);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TasEditorLiveStatus, branch_name_or_id, can_record_current_input,
        recording_target_has_future, review_jump_target, selected_input_target,
    };

    #[test]
    fn live_status_uses_user_facing_session_labels() {
        assert_eq!(
            TasEditorLiveStatus::Ready {
                recording_available: true,
            }
            .primary_label(),
            "Ready to connect"
        );
        assert_eq!(
            TasEditorLiveStatus::ReloadRequired("sample rate".to_owned()).primary_label(),
            "Reload required"
        );
        assert_eq!(
            TasEditorLiveStatus::Linked {
                cursor: 42,
                recording_available: true,
            }
            .primary_label(),
            "Connected · paused before input frame 42"
        );
        assert_eq!(TasEditorLiveStatus::Recording.primary_label(), "Recording");
    }

    #[test]
    fn end_cursor_stages_the_complete_existing_movie() {
        assert_eq!(selected_input_target(599, 600), Some(599));
        assert_eq!(selected_input_target(600, 600), Some(600));
        assert_eq!(selected_input_target(601, 600), None);
        assert_eq!(selected_input_target(0, 0), Some(0));
    }

    #[test]
    fn live_recording_uses_the_project_frame_limit() {
        assert!(can_record_current_input(599, 600));
        assert!(can_record_current_input(
            crate::tas_project::MAX_PROJECT_FRAMES - 2,
            crate::tas_project::MAX_PROJECT_FRAMES - 1,
        ));
        assert!(!can_record_current_input(
            crate::tas_project::MAX_PROJECT_FRAMES - 1,
            crate::tas_project::MAX_PROJECT_FRAMES,
        ));
        assert!(can_record_current_input(600, 600));
    }

    #[test]
    fn recording_mode_choice_only_applies_before_existing_future_input() {
        assert!(recording_target_has_future(Some(1), 12));
        assert!(!recording_target_has_future(Some(12), 12));
        assert!(!recording_target_has_future(None, 12));
    }

    #[test]
    fn review_jumps_clamp_to_the_movie_boundaries() {
        assert_eq!(review_jump_target(30, -60, 120), 0);
        assert_eq!(review_jump_target(30, 60, 120), 90);
        assert_eq!(review_jump_target(100, 300, 120), 120);
    }

    #[test]
    fn blank_branch_name_uses_the_required_id() {
        assert_eq!(branch_name_or_id(" route-b ", ""), "route-b");
        assert_eq!(branch_name_or_id("route-b", " Boss route "), "Boss route");
    }
}
