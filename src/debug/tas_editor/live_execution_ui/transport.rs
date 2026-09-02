use super::{
    TasEditorLiveAction, TasEditorLiveStatus, can_record_current_input, can_stage_selected_input,
};

pub(crate) struct LiveTransportStripControls<'a> {
    pub(crate) status: &'a TasEditorLiveStatus,
    pub(crate) cursor: u64,
    pub(crate) frame_count: u64,
    pub(crate) enabled: bool,
    pub(crate) action: &'a mut Option<TasEditorLiveAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TasPrimaryTransportAction {
    Connect,
    Play,
    Pause,
    Record,
    GoToSelection,
    FrameAdvance,
    Stop,
}

pub(crate) fn draw_live_transport_strip(
    ui: &mut egui::Ui,
    controls: LiveTransportStripControls<'_>,
) {
    let LiveTransportStripControls {
        status,
        cursor,
        frame_count,
        enabled,
        action,
    } = controls;
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("TAS session");
            ui.label(status.primary_label());
            for primary_action in primary_transport_actions(status, cursor, frame_count) {
                match primary_action {
                    TasPrimaryTransportAction::Connect => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Connect to Loaded Game"))
                            .on_hover_text("Connect the loaded game at the selected TAS position")
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::StageSelectedInput);
                        }
                    }
                    TasPrimaryTransportAction::Record => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("● Record"))
                            .on_hover_text("Record gameplay, including neutral input frames")
                            .clicked()
                        {
                            *action = Some(match status {
                                TasEditorLiveStatus::Ready { .. } => {
                                    TasEditorLiveAction::RecordFromSelectedInput
                                }
                                TasEditorLiveStatus::Linked { .. } => {
                                    TasEditorLiveAction::StartRealtimeRecording
                                }
                                _ => unreachable!("record action was not offered for this status"),
                            });
                        }
                    }
                    TasPrimaryTransportAction::Play => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("▶ Play"))
                            .on_hover_text("Play the stored TAS movie without changing it")
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::StartPlayback);
                        }
                    }
                    TasPrimaryTransportAction::Pause => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Pause"))
                            .on_hover_text("Pause at the next stable frame boundary")
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::PausePlayback);
                        }
                    }
                    TasPrimaryTransportAction::GoToSelection => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Go to Selection"))
                            .on_hover_text(format!(
                                "Move the loaded game before input frame {cursor}"
                            ))
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::GoToSelection);
                        }
                    }
                    TasPrimaryTransportAction::FrameAdvance => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Frame Advance & Record"))
                            .on_hover_text(
                                "Record exactly one input frame, then advance the loaded game",
                            )
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::RecordCurrentInputAndAdvance);
                        }
                    }
                    TasPrimaryTransportAction::Stop => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Stop Recording"))
                            .clicked()
                        {
                            *action = Some(TasEditorLiveAction::StopRealtimeRecording);
                        }
                    }
                }
            }
        });
    });
}

fn primary_transport_actions(
    status: &TasEditorLiveStatus,
    cursor: u64,
    frame_count: u64,
) -> Vec<TasPrimaryTransportAction> {
    let can_stage = can_stage_selected_input(cursor, frame_count);
    let can_record = can_record_current_input(cursor, frame_count);
    match status {
        TasEditorLiveStatus::Ready {
            recording_available,
        } if can_stage => {
            let mut actions = vec![TasPrimaryTransportAction::Connect];
            if *recording_available {
                actions.push(TasPrimaryTransportAction::Record);
            }
            actions
        }
        TasEditorLiveStatus::Linked {
            cursor: linked_cursor,
            recording_available,
        } => {
            let mut actions = Vec::new();
            if *linked_cursor != cursor {
                actions.push(TasPrimaryTransportAction::GoToSelection);
            }
            if *recording_available && *linked_cursor == cursor && can_record {
                actions.push(TasPrimaryTransportAction::Record);
                actions.push(TasPrimaryTransportAction::FrameAdvance);
            }
            if *linked_cursor < frame_count {
                actions.insert(0, TasPrimaryTransportAction::Play);
            }
            actions
        }
        TasEditorLiveStatus::Playing { .. } => vec![TasPrimaryTransportAction::Pause],
        TasEditorLiveStatus::Recording => vec![TasPrimaryTransportAction::Stop],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TasPrimaryTransportAction, primary_transport_actions};
    use crate::debug::TasEditorLiveStatus;

    #[test]
    fn primary_transport_actions_match_the_available_live_session_actions() {
        assert_eq!(
            primary_transport_actions(
                &TasEditorLiveStatus::Ready {
                    recording_available: true,
                },
                4,
                8,
            ),
            vec![
                TasPrimaryTransportAction::Connect,
                TasPrimaryTransportAction::Record
            ]
        );
        assert_eq!(
            primary_transport_actions(
                &TasEditorLiveStatus::Linked {
                    cursor: 3,
                    recording_available: true,
                },
                4,
                8,
            ),
            vec![
                TasPrimaryTransportAction::Play,
                TasPrimaryTransportAction::GoToSelection
            ]
        );
        assert_eq!(
            primary_transport_actions(
                &TasEditorLiveStatus::Linked {
                    cursor: 4,
                    recording_available: true,
                },
                4,
                8,
            ),
            vec![
                TasPrimaryTransportAction::Play,
                TasPrimaryTransportAction::Record,
                TasPrimaryTransportAction::FrameAdvance
            ]
        );
        assert_eq!(
            primary_transport_actions(&TasEditorLiveStatus::Recording, 4, 8),
            vec![TasPrimaryTransportAction::Stop]
        );
        assert_eq!(
            primary_transport_actions(
                &TasEditorLiveStatus::Playing {
                    cursor: 4,
                    pause_pending: false,
                },
                4,
                8,
            ),
            vec![TasPrimaryTransportAction::Pause]
        );
    }
}
