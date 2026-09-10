use super::*;

const TALL: egui::Vec2 = egui::vec2(640.0, 800.0);
const NARROW: egui::Vec2 = egui::vec2(360.0, 800.0);

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

fn render(
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
            .expect("marked-range UI test requires an open session");
        let selection = state.timeline_selection.snapshot(session);
        input_clipboard::digital_transform::draw(
            ui,
            session,
            &mut state.input_clipboard,
            selection.as_ref(),
            &mut actions,
        );
    });
    (output, actions)
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
        .unwrap_or_else(|| panic!("could not find {label:?} in marked-range controls"))
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

fn open_controls(context: &egui::Context, state: &mut TasEditorWindowState, size: egui::Vec2) {
    let (output, actions) = render(context, state, size, Vec::new());
    assert!(actions.is_empty());
    let header = label_position(&output, "Edit selected controls");
    let (_, actions) = render(context, state, size, pointer_event(header, true));
    assert!(actions.is_empty());
    let (_, actions) = render(context, state, size, pointer_event(header, false));
    assert!(actions.is_empty());
}

fn click(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) -> Vec<TasEditorAction> {
    let (output, actions) = render(context, state, size, Vec::new());
    assert!(actions.is_empty());
    let position = label_position(&output, label);
    let (_, actions) = render(context, state, size, pointer_event(position, true));
    assert!(actions.is_empty());
    let (_, actions) = render(context, state, size, pointer_event(position, false));
    actions
}

fn select_range(state: &mut TasEditorWindowState, start: u64, end_inclusive: u64) {
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: start,
            extend_selection: false,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: end_inclusive,
            extend_selection: true,
        })
        .unwrap();
}

#[test]
fn marked_range_controls_emit_fresh_actions_and_remove_rows_at_compact_sizes() {
    for size in [TALL, NARROW] {
        let (_root, mut state) = tests::state_with_project(8);
        select_range(&mut state, 1, 3);
        let context = test_context();
        open_controls(&context, &mut state, size);

        let (output, actions) = render(&context, &mut state, size, Vec::new());
        assert!(
            actions.is_empty(),
            "idle draw must not emit actions at {size:?}"
        );
        assert!(
            output.shapes.len() < 260,
            "control UI must remain bounded at {size:?}"
        );
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Marked edit ranges"))
        }));

        let add = click(&context, &mut state, size, "Add selection to edit ranges");
        assert!(matches!(
            add.as_slice(),
            [TasEditorAction::InputClipboard(
                input_clipboard::TasInputClipboardAction::MarkedRanges(
                    input_clipboard::marked_ranges::TasMarkedRangesAction::Add { .. }
                )
            )]
        ));
        state.reduce(add.into_iter().next().unwrap()).unwrap();

        let (output, actions) = render(&context, &mut state, size, Vec::new());
        assert!(actions.is_empty());
        let range_row = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text.starts_with("Frames 1..4")
                {
                    Some(text)
                } else {
                    None
                }
            })
            .expect("marked range row must render");
        assert_eq!(
            range_row.galley.rows.len(),
            1,
            "marked range rows must stay single-line at {size:?}"
        );
        assert!(
            range_row.pos.x + range_row.galley.size().x <= size.x + 0.1,
            "marked range label must fit beside its remove control at {size:?}"
        );

        let apply = click(&context, &mut state, size, "Apply Clear to 1 marked ranges");
        assert!(matches!(
            apply.as_slice(),
            [TasEditorAction::InputClipboard(
                input_clipboard::TasInputClipboardAction::MarkedRanges(
                    input_clipboard::marked_ranges::TasMarkedRangesAction::Apply { .. }
                )
            )]
        ));

        let remove = click(&context, &mut state, size, "Remove range 0");
        assert!(matches!(
            remove.as_slice(),
            [TasEditorAction::InputClipboard(
                input_clipboard::TasInputClipboardAction::MarkedRanges(
                    input_clipboard::marked_ranges::TasMarkedRangesAction::Remove { index: 0, .. }
                )
            )]
        ));
        state.reduce(remove.into_iter().next().unwrap()).unwrap();
        assert!(
            click(&context, &mut state, size, "Apply Clear to 0 marked ranges").is_empty(),
            "apply must stay disabled without marked ranges at {size:?}"
        );
    }
}

#[test]
fn marked_range_controls_disable_add_without_a_timeline_selection() {
    let (_root, mut state) = tests::state_with_project(4);
    state.reduce(TasEditorAction::SelectCursor(4)).unwrap();
    let context = test_context();
    open_controls(&context, &mut state, NARROW);
    assert!(click(&context, &mut state, NARROW, "Add selection to edit ranges").is_empty());
    assert!(
        click(
            &context,
            &mut state,
            NARROW,
            "Apply Clear to 0 marked ranges"
        )
        .is_empty()
    );
}

#[test]
fn marked_range_rows_stay_single_line_with_large_compact_text() {
    let (_root, mut state) = tests::state_with_project(8);
    select_range(&mut state, 1, 3);
    let context = test_context();
    context.global_style_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(28.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(28.0));
        style.spacing.interact_size.y = 42.0;
    });
    open_controls(&context, &mut state, NARROW);
    let add = click(&context, &mut state, NARROW, "Add selection to edit ranges");
    state.reduce(add.into_iter().next().unwrap()).unwrap();

    let (output, actions) = render(&context, &mut state, NARROW, Vec::new());
    assert!(actions.is_empty());
    let range_row = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with("Frames 1..4")
            {
                Some(text)
            } else {
                None
            }
        })
        .expect("large compact marked range row must render");
    assert_eq!(range_row.galley.rows.len(), 1);
    assert!(range_row.pos.x + range_row.galley.size().x <= NARROW.x + 0.1);
}
