use super::*;

#[test]
fn recording_focus_feedback_is_rendered_in_the_header_and_panel() {
    for (status, expected) in [
        (TasEditorLiveStatus::Recording, "Recording"),
        (
            TasEditorLiveStatus::RecordingWaitingForGameInput,
            "Recording: waiting for game input focus",
        ),
        (
            TasEditorLiveStatus::Linked {
                cursor: 4,
                recording_available: true,
            },
            "Connected · paused before input frame 4",
        ),
    ] {
        let context = egui::Context::default();
        let mut mode = TasLiveRecordingMode::ReplaceExistingInput;
        let mut action = None;
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw_live_transport_strip(
                    ui,
                    LiveTransportStripControls {
                        status: &status,
                        cursor: 4,
                        frame_count: 4,
                        enabled: true,
                        action: &mut action,
                    },
                );
                draw_live_execution_panel(
                    ui,
                    LiveExecutionPanelControls {
                        status: &status,
                        cursor: 4,
                        frame_count: 4,
                        recording_input_summary: "controller input",
                        recording_mode: &mut mode,
                        action: &mut action,
                        actions: &mut actions,
                    },
                );
            },
        );
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(texts.contains(&expected), "missing status: {texts:?}");
        if status == TasEditorLiveStatus::RecordingWaitingForGameInput {
            assert_eq!(texts.iter().filter(|text| **text == expected).count(), 2);
        } else {
            assert!(
                texts
                    .iter()
                    .all(|text| !text.contains("waiting for game input focus"))
            );
        }
        assert!(action.is_none());
        assert!(actions.is_empty());
    }
}
