use super::special_input_editor::{
    GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE, GBA_TILT_DEVICE, NES_ZAPPER_DEVICE,
};
use super::*;
use crate::tas_project::{TasInputFrame, TasSpecialTransform, TasZapperInput};

const WIDE: egui::Vec2 = egui::vec2(640.0, 800.0);
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
            .expect("special transform UI test requires an open session");
        let selection = state.timeline_selection.snapshot(session);
        input_clipboard::special_transform_ui::draw(
            ui,
            session,
            &mut state.input_clipboard,
            selection.as_ref(),
            &mut actions,
        );
    });
    (output, actions)
}

fn label_position(output: &egui::FullOutput, label: &str, exact: bool) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && if exact {
                    text.galley.job.text == label
                } else {
                    text.galley.job.text.starts_with(label)
                }
            {
                Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("could not find {label:?} in special transform controls"))
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
    let header = label_position(&output, "Edit recorded special input ranges", false);
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
    let position = label_position(&output, label, false);
    let (_, actions) = render(context, state, size, pointer_event(position, true));
    assert!(actions.is_empty());
    let (_, actions) = render(context, state, size, pointer_event(position, false));
    actions
}

fn click_exact(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) -> Vec<TasEditorAction> {
    let (output, actions) = render(context, state, size, Vec::new());
    assert!(actions.is_empty());
    let position = label_position(&output, label, true);
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
fn special_transform_controls_emit_selection_and_marked_targets_at_compact_sizes() {
    for size in [WIDE, NARROW] {
        let (_root, mut state) = special_input_tests::synthetic_state(
            "gb",
            &[GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE],
            TasInputFrame::default(),
            Default::default(),
        );
        select_range(&mut state, 0, 1);
        let context = test_context();
        open_controls(&context, &mut state, size);
        let (output, actions) = render(&context, &mut state, size, Vec::new());
        assert!(
            actions.is_empty(),
            "idle special controls emit no actions at {size:?}"
        );
        assert!(
            output.shapes.len() < 220,
            "special controls must stay bounded at {size:?}"
        );
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Recorded tilt"))
        }));
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Camera")
        }));
        assert!(!output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Zapper")
        }));

        assert!(click(&context, &mut state, size, "X").is_empty());
        let selection_apply = click(&context, &mut state, size, "Apply Clear to selection");
        assert!(matches!(
            selection_apply.as_slice(),
            [TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::ApplySpecialTransform(
                input_clipboard::special_ranges::TasSpecialRangeAction {
                    target: input_clipboard::special_ranges::TasSpecialRangeTarget::Selection(selection),
                    mask,
                    transform: TasSpecialTransform::Clear,
                    ..
                }
            ))] if selection.start == 0 && selection.end == 2 && mask.tilt_x && !mask.tilt_y && !mask.zapper && !mask.camera
        ));

        let add = click(&context, &mut state, size, "Add selection to edit ranges");
        state.reduce(add.into_iter().next().unwrap()).unwrap();
        let marked_apply = click(&context, &mut state, size, "Apply Clear to 1 marked ranges");
        assert!(matches!(
            marked_apply.as_slice(),
            [TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::ApplySpecialTransform(
                input_clipboard::special_ranges::TasSpecialRangeAction {
                    target: input_clipboard::special_ranges::TasSpecialRangeTarget::Marked(snapshot),
                    mask,
                    transform: TasSpecialTransform::Clear,
                    ..
                }
            ))] if snapshot.ranges == [(0, 2)] && mask.tilt_x
        ));
    }
}

#[test]
fn special_transform_reverse_action_is_selection_stale_and_capability_scoped() {
    let (_root, mut state) = special_input_tests::synthetic_state(
        "gba",
        &[GBA_TILT_DEVICE],
        TasInputFrame::default(),
        Default::default(),
    );
    select_range(&mut state, 0, 1);
    let context = test_context();
    open_controls(&context, &mut state, WIDE);
    assert!(click(&context, &mut state, WIDE, "X").is_empty());
    assert!(click_exact(&context, &mut state, WIDE, "Clear").is_empty());
    assert!(click_exact(&context, &mut state, WIDE, "Reverse").is_empty());
    let action = click(&context, &mut state, WIDE, "Apply Reverse to selection")
        .into_iter()
        .next()
        .expect("reverse selection apply must emit one action");
    assert!(matches!(
        &action,
        TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::ApplySpecialTransform(
            input_clipboard::special_ranges::TasSpecialRangeAction {
                target: input_clipboard::special_ranges::TasSpecialRangeTarget::Selection(selection),
                mask,
                transform: TasSpecialTransform::Reverse,
                ..
            }
        )) if selection.start == 0 && selection.end == 2 && mask.tilt_x && !mask.zapper && !mask.camera
    ));
    select_range(&mut state, 1, 2);
    assert!(
        state.reduce(action).is_err(),
        "selection-targeted special transform must reject a stale selection"
    );
}

#[test]
fn special_transform_controls_disable_empty_mask_and_unsupported_channels() {
    let (_root, mut no_selection) = special_input_tests::synthetic_state(
        "nes",
        &[NES_ZAPPER_DEVICE],
        TasInputFrame::default(),
        Default::default(),
    );
    no_selection
        .reduce(TasEditorAction::SelectCursor(3))
        .unwrap();
    let context = test_context();
    open_controls(&context, &mut no_selection, NARROW);
    let (output, actions) = render(&context, &mut no_selection, NARROW, Vec::new());
    assert!(actions.is_empty());
    assert!(output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Zapper")
    }));
    assert!(!output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Recorded tilt"))
    }));
    assert!(!output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Camera")
    }));
    assert!(
        click(
            &context,
            &mut no_selection,
            NARROW,
            "Apply Clear to selection"
        )
        .is_empty(),
        "empty masks and missing selections must keep selection apply disabled"
    );
    assert!(
        click(
            &context,
            &mut no_selection,
            NARROW,
            "Apply Clear to 0 marked ranges"
        )
        .is_empty(),
        "empty masks and marked lists must keep marked apply disabled"
    );
}

#[test]
fn special_transform_zapper_clear_preserves_an_unchecked_channel() {
    let mut input = TasInputFrame {
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some([30, 40]),
        },
        ..TasInputFrame::default()
    };
    input.players[0].buttons = 1;
    let (_root, mut state) = special_input_tests::synthetic_state(
        "nes",
        &[NES_ZAPPER_DEVICE],
        input,
        Default::default(),
    );
    select_range(&mut state, 0, 0);
    let context = test_context();
    open_controls(&context, &mut state, WIDE);
    assert!(click(&context, &mut state, WIDE, "Zapper").is_empty());
    let action = click(&context, &mut state, WIDE, "Apply Clear to selection")
        .into_iter()
        .next()
        .unwrap();
    state.reduce(action).unwrap();
    let frame = state
        .session
        .as_ref()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(frame.zapper, TasZapperInput::default());
    assert_eq!(frame.players[0].buttons, 1);
}
