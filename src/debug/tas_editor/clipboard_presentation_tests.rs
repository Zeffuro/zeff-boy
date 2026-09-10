use super::*;
use crate::tas_project::{
    MAX_PROJECT_FRAMES, TasColecoControllerInput, TasColecoKeypadKey, TasControllerInput,
    TasInputFrame, TasInputPattern, TasInputSpan,
};
use zeff_emu_common::replay::ReplayEvent;

const UI_SIZE: egui::Vec2 = egui::vec2(960.0, 800.0);

fn test_context() -> egui::Context {
    let context = egui::Context::default();
    context.global_style_mut(|style| {
        style.animation_time = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
        style.interaction.tooltip_delay = 0.0;
    });
    context
}

fn raw_input(size: egui::Vec2, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..egui::RawInput::default()
    }
}

fn render_in(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<TasEditorAction>) {
    let mut actions = Vec::new();
    let output = context.run_ui(raw_input(size, events), |ui| {
        let session = state
            .session
            .as_ref()
            .expect("clipboard presentation test requires an open session");
        let selection = state.timeline_selection.snapshot(session);
        input_clipboard::draw_input_clipboard(
            ui,
            session,
            &mut state.input_clipboard,
            selection.as_ref(),
            &mut actions,
        );
    });
    (output, actions)
}

fn render(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<TasEditorAction>) {
    render_in(context, state, UI_SIZE, events)
}

fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with(label)
            {
                Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("could not find {label:?} in clipboard panel"))
}

fn pointer_event(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn open_panel_in(context: &egui::Context, state: &mut TasEditorWindowState, size: egui::Vec2) {
    let (output, actions) = render_in(context, state, size, Vec::new());
    assert!(actions.is_empty());
    let header = label_position(&output, "Input pattern");
    let (_, actions) = render_in(context, state, size, pointer_event(header, true));
    assert!(actions.is_empty());
    let (_, actions) = render_in(context, state, size, pointer_event(header, false));
    assert!(actions.is_empty());
}

fn open_panel(context: &egui::Context, state: &mut TasEditorWindowState) {
    open_panel_in(context, state, UI_SIZE)
}

fn click(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    label: &str,
) -> Vec<TasEditorAction> {
    let (output, actions) = render(context, state, Vec::new());
    assert!(actions.is_empty());
    let position = label_position(&output, label);
    let (_, actions) = render(context, state, pointer_event(position, true));
    assert!(actions.is_empty());
    let (_, actions) = render(context, state, pointer_event(position, false));
    actions
}

fn copy_pattern(
    state: &TasEditorWindowState,
    start: u64,
    pattern: TasInputPattern,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::copy_pattern(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        start,
        pattern,
    ))
}

fn copy_constant(state: &TasEditorWindowState, start: u64, length: u64) -> TasEditorAction {
    copy_pattern(
        state,
        start,
        TasInputPattern::constant(length, TasInputFrame::default()).unwrap(),
    )
}

fn set_input(state: &mut TasEditorWindowState, frame: u64, buttons: u8) {
    let session = state.session.as_mut().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    session
        .edit_transaction(move |edit| {
            edit.set_input_range(
                &branch_id,
                frame,
                1,
                TasInputFrame {
                    players: [TasControllerInput { buttons, dpad: 0 }; 5],
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
}

fn paste_action(state: &TasEditorWindowState) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::paste_at_cursor(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        session.cursor(),
        state.input_clipboard.generation(),
    ))
}

fn insert_action(state: &TasEditorWindowState) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::insert_at_cursor(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        session.cursor(),
        state.input_clipboard.generation(),
    ))
}

fn tile_action(state: &mut TasEditorWindowState) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    let selection = state.timeline_selection.snapshot(session).unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::tile_selection(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        selection.start,
        selection.end,
        state.input_clipboard.generation(),
    ))
}

#[test]
fn clipboard_panel_clicks_emit_fresh_paste_insert_and_tile_witnesses() {
    let (_root, mut state) = tests::state_with_project(8);
    state.reduce(copy_constant(&state, 0, 2)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    let context = test_context();
    open_panel(&context, &mut state);

    let old = paste_action(&state);
    set_input(&mut state, 7, 1);
    state.reduce(copy_constant(&state, 0, 2)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(4)).unwrap();
    let expected = paste_action(&state);
    assert_ne!(old, expected);
    assert_eq!(
        click(&context, &mut state, "Paste at selected cursor"),
        vec![expected]
    );

    let old = insert_action(&state);
    set_input(&mut state, 7, 2);
    state.reduce(copy_constant(&state, 0, 2)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let expected = insert_action(&state);
    assert_ne!(old, expected);
    assert_eq!(
        click(
            &context,
            &mut state,
            "Insert copied frames at selected cursor"
        ),
        vec![expected]
    );

    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 3,
            extend_selection: false,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 6,
            extend_selection: true,
        })
        .unwrap();
    let old = tile_action(&mut state);
    set_input(&mut state, 7, 4);
    state.reduce(copy_constant(&state, 0, 2)).unwrap();
    let expected = tile_action(&mut state);
    assert_ne!(old, expected);
    assert_eq!(
        click(&context, &mut state, "Tile across selection"),
        vec![expected]
    );
}

#[test]
fn clipboard_panel_disables_tiling_without_a_selection_or_for_copied_events() {
    let (_root, mut no_selection) = tests::state_with_project(5);
    no_selection
        .reduce(copy_constant(&no_selection, 0, 1))
        .unwrap();
    no_selection
        .reduce(TasEditorAction::SelectCursor(5))
        .unwrap();
    let context = test_context();
    open_panel(&context, &mut no_selection);
    assert!(
        click(&context, &mut no_selection, "Tile across selection").is_empty(),
        "tile must remain disabled without a selected frame range"
    );

    let event = ReplayEvent::FdsDiskSide { frame: 1, side: 2 };
    let (_root, mut copied_event) = event_tests::fds_state(5, vec![event.clone()]);
    copied_event
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 0,
            extend_selection: false,
        })
        .unwrap();
    copied_event
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 1,
            extend_selection: true,
        })
        .unwrap();
    let session = copied_event.session.as_ref().unwrap();
    let action = input_clipboard::TasInputClipboardAction::copy_selection_with_events(
        session.project_content_sha256(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        session.selected_branch().input_pattern(0, 2).unwrap(),
        vec![event],
        timeline_selection::TasInputSelection {
            branch_id: session.selected_branch_id().to_owned(),
            start: 0,
            end: 2,
        },
    );
    copied_event
        .reduce(TasEditorAction::InputClipboard(action))
        .unwrap();
    let context = test_context();
    open_panel(&context, &mut copied_event);
    assert!(
        click(&context, &mut copied_event, "Tile across selection").is_empty(),
        "tile must remain disabled when the copied hunk contains drive events"
    );
}

#[test]
fn clipboard_panel_blocks_insertion_at_the_movie_limit_but_allows_fixed_paste() {
    let (_root, mut state) = tests::state_with_project(MAX_PROJECT_FRAMES);
    state.reduce(copy_constant(&state, 0, 1)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(0)).unwrap();
    let context = test_context();
    open_panel(&context, &mut state);

    assert!(
        click(
            &context,
            &mut state,
            "Insert copied frames at selected cursor"
        )
        .is_empty(),
        "insertion must be disabled when the branch is already at the frame limit"
    );
    let expected = paste_action(&state);
    assert_eq!(
        click(&context, &mut state, "Paste at selected cursor"),
        vec![expected]
    );
}

#[test]
fn compact_clipboard_preview_virtualizes_sparse_rows_with_single_line_geometry() {
    let span_count = 4_096;
    let pattern = TasInputPattern::new(
        (span_count * 2) as u64,
        (0..span_count)
            .map(|index| TasInputSpan {
                start: (index * 2) as u64,
                length: 1,
                input: TasInputFrame {
                    players: [TasControllerInput {
                        buttons: (index % u8::MAX as usize) as u8 + 1,
                        dpad: 0,
                    }; 5],
                    ..TasInputFrame::default()
                },
            })
            .collect(),
    )
    .unwrap();
    let (_root, mut state) = tests::state_with_project(pattern.length());
    let branch_id = state
        .session
        .as_ref()
        .unwrap()
        .selected_branch_id()
        .to_owned();
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| edit.replace_input_pattern(&branch_id, 0, &pattern))
        .unwrap();
    state
        .reduce(copy_pattern(&state, 0, pattern.clone()))
        .unwrap();

    for size in [egui::vec2(640.0, 800.0), egui::vec2(360.0, 800.0)] {
        let context = test_context();
        open_panel_in(&context, &mut state, size);
        let (output, actions) = render_in(&context, &mut state, size, Vec::new());
        assert!(actions.is_empty());
        assert!(
            output.shapes.len() < 160,
            "the compact preview must draw only viewport rows at {size:?}"
        );
        let mut rows = output
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                text.galley
                    .job
                    .text
                    .split_once("..")
                    .and_then(|(start, _)| start.parse::<u64>().ok())
                    .map(|_| {
                        (
                            text.pos,
                            text.galley.size(),
                            text.galley.rows.len(),
                            text.galley.job.text.clone(),
                        )
                    })
            })
            .collect::<Vec<_>>();
        assert!(!rows.is_empty());
        assert!(
            rows.len() < 16,
            "the compact viewport must remain bounded at {size:?}"
        );
        assert!(rows.iter().all(|(_, _, line_count, _)| *line_count == 1));
        rows.sort_by(|left, right| left.0.y.total_cmp(&right.0.y));
        for pair in rows.windows(2) {
            assert!(
                pair[0].0.y + pair[0].1.y <= pair[1].0.y + 0.1,
                "visible sparse rows must not overlap at {size:?}"
            );
        }

        let first = &rows[0];
        let hover_position = first.0 + egui::vec2(8.0, first.1.y * 0.5);
        for _ in 0..8 {
            let (_, actions) = render_in(&context, &mut state, size, Vec::new());
            assert!(actions.is_empty());
        }
        let (_, actions) = render_in(
            &context,
            &mut state,
            size,
            vec![egui::Event::PointerMoved(hover_position)],
        );
        assert!(actions.is_empty());
        let (hover_output, actions) = render_in(&context, &mut state, size, Vec::new());
        assert!(actions.is_empty());
        assert!(hover_output.shapes.iter().any(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Text(text)
                    if text.galley.job.text == first.3 && !text.galley.elided
            )
        }));
    }
}

#[test]
fn compact_clipboard_preview_shows_both_coleco_controllers_fire_and_keypad() {
    let input = TasInputFrame {
        coleco: [
            TasColecoControllerInput {
                up: true,
                left_button: true,
                keypad: TasColecoKeypadKey::Star,
                ..TasColecoControllerInput::default()
            },
            TasColecoControllerInput {
                right: true,
                right_button: true,
                keypad: TasColecoKeypadKey::Pound,
                ..TasColecoControllerInput::default()
            },
        ],
        ..TasInputFrame::default()
    };
    let (_root, mut state) = tests::state_with_project(1);
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))
        .unwrap();
    state
        .reduce(copy_pattern(
            &state,
            0,
            TasInputPattern::constant(1, input).unwrap(),
        ))
        .unwrap();
    let context = test_context();
    open_panel_in(&context, &mut state, egui::vec2(360.0, 800.0));
    for _ in 0..8 {
        let (_, actions) = render_in(&context, &mut state, egui::vec2(360.0, 800.0), Vec::new());
        assert!(actions.is_empty());
    }
    let (output, actions) = render_in(&context, &mut state, egui::vec2(360.0, 800.0), Vec::new());
    assert!(actions.is_empty());
    let row = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with("0..1 ")
            {
                Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
            } else {
                None
            }
        })
        .expect("Coleco clipboard row must render");
    let (_, actions) = render_in(
        &context,
        &mut state,
        egui::vec2(360.0, 800.0),
        vec![egui::Event::PointerMoved(row)],
    );
    assert!(actions.is_empty());
    let (hover_output, actions) =
        render_in(&context, &mut state, egui::vec2(360.0, 800.0), Vec::new());
    assert!(actions.is_empty());
    let text_shapes = hover_output
        .shapes
        .iter()
        .filter_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            Some((text.galley.job.text.as_str(), text.galley.elided))
        })
        .collect::<Vec<_>>();
    assert!(
        text_shapes.iter().any(|(text, elided)| {
            text.contains(
                "coleco=p1=u1/r0/d0/l0/fire-l1/fire-r0/key=* p2=u0/r1/d0/l0/fire-l0/fire-r1/key=#",
            ) && !elided
        }),
        "full Coleco clipboard tooltip was not rendered: {text_shapes:#?}"
    );
}
