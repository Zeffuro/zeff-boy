use super::*;

#[test]
fn song_rows_dispatch_double_click_and_context_actions_without_audio() {
    let mut bytes = vec![0; 2 * 0x1000];
    for index in 0..2 {
        crate::audio_discovery::test_support::collection(&mut bytes, index * 0x1000 + 0x100);
    }
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let context = egui::Context::default();
    let mut workspace = AudioWorkspace::default();
    let render = |workspace: &mut AudioWorkspace, events, time| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                super::super::workspace::draw(
                    ui,
                    workspace,
                    &report,
                    &bytes,
                    &crate::audio_discovery::roles::classify(&report),
                    |_| true,
                )
            },
        )
    };
    let _ = render(&mut workspace, Vec::new(), 1.0);
    let output = render(&mut workspace, Vec::new(), 1.1);
    let rows = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text.starts_with("MP2k +")
                    && text.galley.job.text.contains("tracks") =>
            {
                Some(text)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "fixture has two visible song rows");
    let row_position = rows[0].pos + egui::vec2(8.0, rows[0].galley.size().y / 2.0);
    let second_row_position = rows[1].pos + egui::vec2(8.0, rows[1].galley.size().y / 2.0);
    for (time, pressed) in [(1.2, true), (1.21, false), (1.3, true), (1.31, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(row_position),
                egui::Event::PointerButton {
                    pos: row_position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    assert_eq!(workspace.take_preview_request(), Some(SongId::Mp2k(0)));

    for (time, pressed) in [(2.0, true), (2.01, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(second_row_position),
                egui::Event::PointerButton {
                    pos: second_row_position,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    let output = render(&mut workspace, Vec::new(), 2.1);
    let preview = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Preview" => Some(text),
            _ => None,
        })
        .expect("song context menu offers preview");
    let preview_position = preview.pos + preview.galley.size() / 2.0;
    for (time, pressed) in [(2.2, true), (2.21, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(preview_position),
                egui::Event::PointerButton {
                    pos: preview_position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    assert_eq!(workspace.take_preview_request(), Some(SongId::Mp2k(1)));
    assert_eq!(workspace.selected_candidate, Some(SongId::Mp2k(1)));
    let _ = render(&mut workspace, Vec::new(), 2.3);
    assert_eq!(workspace.selected_candidate, Some(SongId::Mp2k(1)));

    for (time, pressed) in [(3.0, true), (3.01, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(second_row_position),
                egui::Event::PointerButton {
                    pos: second_row_position,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    let output = render(&mut workspace, Vec::new(), 3.1);
    let export = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Open export controls" => Some(text),
            _ => None,
        })
        .expect("song context menu offers export controls");
    let export_position = export.pos + export.galley.size() / 2.0;
    for (time, pressed) in [(3.2, true), (3.21, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(export_position),
                egui::Event::PointerButton {
                    pos: export_position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    assert_eq!(workspace.take_export_request(), Some(SongId::Mp2k(1)));
    let _ = render(&mut workspace, Vec::new(), 3.3);

    for (time, pressed) in [(4.0, true), (4.01, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(second_row_position),
                egui::Event::PointerButton {
                    pos: second_row_position,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    let output = render(&mut workspace, Vec::new(), 4.1);
    let details = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "View song details" => Some(text),
            _ => None,
        })
        .expect("song context menu offers details");
    let details_position = details.pos + details.galley.size() / 2.0;
    for (time, pressed) in [(4.2, true), (4.21, false)] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(details_position),
                egui::Event::PointerButton {
                    pos: details_position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            time,
        );
    }
    let output = render(&mut workspace, Vec::new(), 4.3);
    assert!(output.shapes.iter().any(
        |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Song details")
    ));
}
