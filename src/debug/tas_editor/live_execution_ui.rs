use super::TasEditorFileRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorLiveStatus {
    Unavailable(String),
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
    pub(crate) fn locks_editor(&self) -> bool {
        !matches!(
            self,
            Self::Unavailable(_) | Self::Ready { .. } | Self::Linked { .. }
        )
    }

    pub(crate) fn holds_authority(&self) -> bool {
        !matches!(self, Self::Unavailable(_) | Self::Ready { .. })
    }

    pub(crate) fn requires_return_on_close(&self) -> bool {
        matches!(
            self,
            Self::Acquiring | Self::Staging { .. } | Self::AdvancingFrame | Self::Recording
        )
    }

    pub(crate) fn follows_cursor(&self) -> bool {
        matches!(
            self,
            Self::Linked { .. } | Self::AdvancingFrame | Self::Recording
        )
    }

    pub(crate) fn is_linked(&self) -> bool {
        matches!(self, Self::Linked { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorLiveAction {
    StageSelectedInput,
    RecordFromSelectedInput,
    SeekLinkedInput,
    RecordCurrentInputAndAdvance,
    StartRealtimeRecording,
    StopRealtimeRecording,
    KeepResultAndReturnToGame,
    ReturnToGameUnchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TasEditorHostRequest {
    File(TasEditorFileRequest),
    Live(TasEditorLiveAction),
}

pub(super) fn draw_live_execution_panel(
    ui: &mut egui::Ui,
    status: &TasEditorLiveStatus,
    cursor: u64,
    frame_count: u64,
    action: &mut Option<TasEditorLiveAction>,
) {
    ui.group(|ui| {
        ui.strong("Play and record in loaded game");
        match status {
            TasEditorLiveStatus::Unavailable(reason) => {
                ui.colored_label(ui.visuals().warn_fg_color, reason);
            }
            TasEditorLiveStatus::Ready {
                recording_available,
            } => {
                let target = selected_input_target(cursor, frame_count);
                let can_stage = can_stage_selected_input(cursor, frame_count);
                if *recording_available
                    && ui
                        .add_enabled(can_stage, egui::Button::new("Record from here"))
                        .on_hover_text(
                            "Link the loaded game to this frame, then record new input after it",
                        )
                        .clicked()
                {
                    *action = Some(TasEditorLiveAction::RecordFromSelectedInput);
                }
                if ui
                    .add_enabled(can_stage, egui::Button::new("Link loaded game here"))
                    .on_hover_text("Move the loaded game to the selected timeline position")
                    .clicked()
                {
                    *action = Some(TasEditorLiveAction::StageSelectedInput);
                }
                match target {
                    None => ui.small("Select an input frame first."),
                    Some(target) => ui.small(format!(
                        "Runs frames 0 through {} from the selected branch.",
                        target - 1
                    )),
                };
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
                ui.strong(format!("Loaded game linked at frame {linked_cursor}"));
                ui.small(
                    "Selecting or editing a timeline row moves the loaded game to the new position.",
                );
                if *recording_available {
                    if ui
                        .add_enabled(
                            can_record_current_input(cursor, frame_count),
                            egui::Button::new("Record one inserted frame"),
                        )
                        .clicked()
                    {
                        *action = Some(TasEditorLiveAction::RecordCurrentInputAndAdvance);
                    }
                    if ui
                        .add_enabled(
                            can_record_current_input(cursor, frame_count),
                            egui::Button::new("Start continuous recording"),
                        )
                        .clicked()
                    {
                        *action = Some(TasEditorLiveAction::StartRealtimeRecording);
                    }
                    ui.small("Click the game and play normally. Recording stops on Pause or focus loss.");
                }
                if ui.button("Disconnect and keep this game position").clicked() {
                    *action = Some(TasEditorLiveAction::KeepResultAndReturnToGame);
                }
                return_unchanged_button(ui, action);
                ui.small("Disconnecting does not save the TAS project file.");
            }
            TasEditorLiveStatus::AdvancingFrame => {
                ui.label("Recording the current controls in the loaded game…");
                return_unchanged_button(ui, action);
            }
            TasEditorLiveStatus::Recording => {
                ui.strong("Recording live input");
                ui.small(
                    "Play in the game window. New rows are inserted as the game advances. Recording stops on Pause or focus loss.",
                );
                if ui.button("Stop recording after current frame").clicked() {
                    *action = Some(TasEditorLiveAction::StopRealtimeRecording);
                }
                return_unchanged_button(ui, action);
                ui.small(
                    "Recorded frames already edit the TAS project. Restoring the pre-TAS game does not remove them.",
                );
            }
            TasEditorLiveStatus::Returning => {
                ui.label("Restoring the game state from before TAS linked…");
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

pub(crate) fn selected_input_target(cursor: u64, frame_count: u64) -> Option<u64> {
    if frame_count == 0
        || frame_count > crate::tas_project::MAX_PROJECT_FRAMES
        || cursor > frame_count
    {
        return None;
    }
    if cursor == frame_count {
        Some(frame_count)
    } else {
        cursor.checked_add(1)
    }
}

pub(crate) fn can_stage_selected_input(cursor: u64, frame_count: u64) -> bool {
    selected_input_target(cursor, frame_count).is_some()
}

fn can_record_current_input(cursor: u64, frame_count: u64) -> bool {
    cursor <= frame_count && frame_count < crate::tas_project::MAX_PROJECT_FRAMES
}

fn return_unchanged_button(ui: &mut egui::Ui, action: &mut Option<TasEditorLiveAction>) {
    if ui.button("Disconnect and restore pre-TAS game").clicked() {
        *action = Some(TasEditorLiveAction::ReturnToGameUnchanged);
    }
}

#[cfg(test)]
mod tests {
    use super::{can_record_current_input, selected_input_target};

    #[test]
    fn end_cursor_stages_the_complete_existing_movie() {
        assert_eq!(selected_input_target(599, 600), Some(600));
        assert_eq!(selected_input_target(600, 600), Some(600));
        assert_eq!(selected_input_target(601, 600), None);
        assert_eq!(selected_input_target(0, 0), None);
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
}
