use std::{
    thread,
    time::{Duration, Instant},
};

use zeff_emu_common::replay::ReplayEvent;

use super::*;
use crate::settings::{DebugColors, UiDensity, UiThemePreset};
use crate::tas_project::{
    TasCameraInput, TasColecoControllerInput, TasColecoKeypadKey, TasControllerInput, TasDigest,
    TasInputFrame, TasZapperInput,
};

const WIDE_PARENT: egui::Vec2 = egui::vec2(TWO_PANE_MIN_WIDTH, 760.0);
const COMPACT_PARENT: egui::Vec2 = egui::vec2(360.0, 1_600.0);
const HUNK_LIST_HEIGHT: f32 = 220.0;
const HUNK_COUNT: usize = crate::tas_project::MAX_BRANCH_DIFF_RETAINED_HUNKS;
const HUNK_BASE: u64 = 1_000_000;
const LAST_HUNK_FRAME: u64 = HUNK_BASE + ((HUNK_COUNT - 1) * 2) as u64;

fn test_context() -> egui::Context {
    test_context_with_density(UiDensity::Compact)
}

fn test_context_with_density(density: UiDensity) -> egui::Context {
    let context = egui::Context::default();
    crate::graphics::apply_egui_theme(
        &context,
        UiThemePreset::DefaultDark,
        density,
        1.0,
        DebugColors::default(),
    );
    context.global_style_mut(|style| {
        style.animation_time = 0.0;
        style.scroll_animation = egui::style::ScrollAnimation::none();
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

fn render_project(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<TasEditorAction>) {
    let mut actions = Vec::new();
    let mut live_action = None;
    let output = context.run_ui(raw_input(size, events), |ui| {
        if project_content_ui::uses_two_pane_layout(size.x) {
            project_content_ui::draw_project_content(ui, state, &mut actions, &mut live_action);
        } else {
            let _ =
                content::draw_scrollable_project_content(ui, state, &mut actions, &mut live_action);
        }
    });
    assert!(live_action.is_none());
    (output, actions)
}

fn render_full_content(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<TasEditorAction>) {
    let mut actions = Vec::new();
    let mut file_request = None;
    let mut project_replacement = None;
    let mut live_action = None;
    let output = context.run_ui(raw_input(size, events), |ui| {
        content::draw(
            ui,
            state,
            &mut actions,
            &mut file_request,
            &mut project_replacement,
            &mut live_action,
        );
    });
    assert!(file_request.is_none());
    assert!(project_replacement.is_none());
    assert!(live_action.is_none());
    (output, actions)
}

#[derive(Clone, Copy, Debug)]
struct TextWitness {
    position: egui::Pos2,
    rect: egui::Rect,
    clip_rect: egui::Rect,
}

fn text_witnesses(output: &egui::FullOutput, label: &str) -> Vec<TextWitness> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains(label) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                Some(TextWitness {
                    position: text.pos,
                    rect,
                    clip_rect: shape.clip_rect,
                })
            }
            _ => None,
        })
        .collect()
}

fn exact_text_witness(output: &egui::FullOutput, label: &str) -> TextWitness {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.trim() == label => Some(TextWitness {
                position: text.pos,
                rect: egui::Rect::from_min_size(text.pos, text.galley.size()),
                clip_rect: shape.clip_rect,
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("could not find exact text {label:?}"))
}

fn assert_non_overlapping_controls(controls: &[TextWitness]) {
    for (index, control) in controls.iter().enumerate() {
        assert!(
            control.clip_rect.contains_rect(control.rect),
            "branch-diff control must be fully visible: {control:?}"
        );
        for other in controls.iter().skip(index + 1) {
            assert!(
                !control.rect.intersects(other.rect),
                "branch-diff summary or action controls must not overlap: {control:?} / {other:?}"
            );
        }
    }
}

fn assert_non_overlapping_rects(controls: &[TextWitness], size: egui::Vec2) {
    for (index, control) in controls.iter().enumerate() {
        assert!(
            control.rect.max.x <= size.x + 0.1,
            "branch-diff text must stay within the parent width: {control:?}"
        );
        for other in controls.iter().skip(index + 1) {
            assert!(
                !control.rect.intersects(other.rect),
                "branch-diff wrapped rows must not overlap: {control:?} / {other:?}"
            );
        }
    }
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

fn click_at(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
) -> Vec<TasEditorAction> {
    let (_, actions) = render_project(context, state, size, pointer_event(position, true));
    assert!(
        actions.is_empty(),
        "pressing a branch-diff control emitted an action"
    );
    let (_, actions) = render_project(context, state, size, pointer_event(position, false));
    actions
}

fn click_full_content_at(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
) -> Vec<TasEditorAction> {
    let (_, actions) = render_full_content(context, state, size, pointer_event(position, true));
    assert!(
        actions.is_empty(),
        "pressing a TAS control emitted an action"
    );
    let (_, actions) = render_full_content(context, state, size, pointer_event(position, false));
    actions
}

fn scroll_parent_until_visible(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) -> TextWitness {
    scroll_parent_until_visible_together(context, state, size, &[label])
        .into_iter()
        .next()
        .expect("one visible label must produce one witness")
}

fn scroll_parent_until_visible_together(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    labels: &[&str],
) -> Vec<TextWitness> {
    let compact = !project_content_ui::uses_two_pane_layout(size.x);
    if compact {
        set_compact_body_scroll_offset(context, state, size, 0.0);
    }
    for step in 0..16 {
        let (output, actions) = render_project(context, state, size, Vec::new());
        assert!(actions.is_empty(), "idle project content emitted an action");
        let witnesses = labels
            .iter()
            .filter_map(|label| {
                text_witnesses(&output, label)
                    .into_iter()
                    .find(|witness| witness.clip_rect.contains_rect(witness.rect))
            })
            .collect::<Vec<_>>();
        if witnesses.len() == labels.len() {
            return witnesses;
        }
        if compact {
            set_compact_body_scroll_offset(context, state, size, ((step + 1) * 400) as f32);
            continue;
        }
        let (_, actions) = render_project(
            context,
            state,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(size.x - 24.0, size.y * 0.5)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -160.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(actions.is_empty());
    }
    panic!("could not scroll project content to visible {labels:?}");
}

fn set_compact_body_scroll_offset(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    offset_y: f32,
) {
    let mut actions = Vec::new();
    let mut live_action = None;
    let mut body = None;
    let _ = context.run_ui(raw_input(size, Vec::new()), |ui| {
        body = Some(content::draw_scrollable_project_content(
            ui,
            state,
            &mut actions,
            &mut live_action,
        ));
    });
    assert!(actions.is_empty());
    assert!(live_action.is_none());
    let body = body.expect("compact TAS body scroll area must render");
    let mut scroll = body.state;
    scroll.offset.y = offset_y;
    scroll.store(context, body.id);
}

fn adjust_compact_body_scroll(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    delta_y: f32,
) {
    let mut actions = Vec::new();
    let mut live_action = None;
    let mut body = None;
    let _ = context.run_ui(raw_input(size, Vec::new()), |ui| {
        body = Some(content::draw_scrollable_project_content(
            ui,
            state,
            &mut actions,
            &mut live_action,
        ));
    });
    assert!(actions.is_empty());
    assert!(live_action.is_none());
    let body = body.expect("compact TAS body scroll area must render");
    let mut scroll = body.state;
    scroll.offset.y = (scroll.offset.y + delta_y).max(0.0);
    scroll.store(context, body.id);
}

fn click_after_scrolling(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) -> Vec<TasEditorAction> {
    let witness = scroll_parent_until_visible(context, state, size, label);
    click_at(
        context,
        state,
        size,
        witness.position + egui::vec2(8.0, witness.rect.height() * 0.5),
    )
}

fn align_parent_label(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) {
    for _ in 0..16 {
        let (output, actions) = render_project(context, state, size, Vec::new());
        assert!(actions.is_empty());
        let witness = text_witnesses(&output, label)
            .into_iter()
            .find(|witness| witness.clip_rect.contains_rect(witness.rect))
            .unwrap_or_else(|| panic!("could not find visible {label:?} for parent alignment"));
        let overflow = witness.rect.max.y + HUNK_LIST_HEIGHT + 8.0 - witness.clip_rect.max.y;
        if overflow <= 0.0 {
            return;
        }
        let delta_y = -overflow.min(160.0);
        let (_, actions) = render_project(
            context,
            state,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(size.x - 24.0, size.y * 0.5)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta_y),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(actions.is_empty());
    }
    panic!("could not align parent content for {label:?}");
}

fn touch_scroll_input(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
    delta_y: f32,
) {
    let (_, actions) = render_project(
        context,
        state,
        size,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::Vec2::ZERO,
                phase: egui::TouchPhase::Start,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta_y),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(actions.is_empty());
    let (_, actions) = render_project(
        context,
        state,
        size,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::Vec2::ZERO,
                phase: egui::TouchPhase::End,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(actions.is_empty());
}

fn scroll_input_to_end(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
) -> egui::FullOutput {
    touch_scroll_input(context, state, size, position, -1_000_000.0);
    let (output, actions) = render_project(context, state, size, Vec::new());
    assert!(actions.is_empty());
    output
}

fn scroll_input_back_to_visible(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
    label: &str,
) -> TextWitness {
    for _ in 0..64 {
        let (output, actions) = render_project(context, state, size, Vec::new());
        assert!(actions.is_empty());
        if let Some(summary) = text_witnesses(&output, label)
            .into_iter()
            .find(|witness| witness.clip_rect.contains_rect(witness.rect))
        {
            return summary;
        }
        touch_scroll_input(context, state, size, position, 32.0);
    }
    panic!("could not scroll back to visible {label:?}");
}

fn scroll_input_forward_to_visible(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    position: egui::Pos2,
    label: &str,
) -> TextWitness {
    for _ in 0..64 {
        let (output, actions) = render_project(context, state, size, Vec::new());
        assert!(actions.is_empty());
        if let Some(witness) = text_witnesses(&output, label)
            .into_iter()
            .filter(|witness| witness.clip_rect.contains_rect(witness.rect))
            .max_by(|left, right| left.position.y.total_cmp(&right.position.y))
        {
            return witness;
        }
        touch_scroll_input(context, state, size, position, -32.0);
    }
    panic!("could not scroll forward to visible {label:?}");
}

fn lowest_visible_text(output: &egui::FullOutput, label: &str) -> TextWitness {
    text_witnesses(output, label)
        .into_iter()
        .filter(|witness| witness.clip_rect.contains_rect(witness.rect))
        .max_by(|left, right| left.position.y.total_cmp(&right.position.y))
        .unwrap_or_else(|| panic!("could not find a visible {label:?} in project content"))
}

fn camera_digest() -> TasDigest {
    TasDigest::from_bytes(b"branch-diff-layout-camera-asset")
}

fn camera_digest_label() -> String {
    let digest = camera_digest().to_hex();
    format!(
        "camera=blob:\n{} {} {} {}",
        &digest[..16],
        &digest[16..32],
        &digest[32..48],
        &digest[48..],
    )
}

fn coleco_summary_label() -> &'static str {
    "coleco=p1=u1/r0/d0/l0/fire-l1/fire-r0/key=* p2=u0/r1/d0/l0/fire-l0/fire-r1/key=#"
}

fn special_input() -> TasInputFrame {
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0xA5,
                dpad: 0x5A,
            },
            TasControllerInput {
                buttons: 0x3C,
                dpad: 0xC3,
            },
            TasControllerInput {
                buttons: 0xF0,
                dpad: 0x0F,
            },
            TasControllerInput {
                buttons: 0x96,
                dpad: 0x69,
            },
            TasControllerInput {
                buttons: 0xFF,
                dpad: 0x11,
            },
        ],
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
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            hit: true,
            screen_pos: Some([319, 239]),
        },
        tilt_x_bits: 0x3F12_3456,
        tilt_y_bits: 0xBF65_4321,
        camera: TasCameraInput::Blob(camera_digest()),
    }
}

fn coleco_only_input() -> TasInputFrame {
    TasInputFrame {
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
    }
}

fn prepared_state() -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let (root, mut state) = tests::state_with_project(HUNK_BASE + (HUNK_COUNT as u64) * 2 + 9);
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "target-id-with-a-deliberately-long-stable-identifier-0123456789".to_owned(),
            name: "Target branch with a deliberately long human readable name".to_owned(),
        })
        .unwrap();
    let target = state
        .session
        .as_ref()
        .unwrap()
        .selected_branch_id()
        .to_owned();
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            assert_eq!(
                edit.insert_camera_asset(b"branch-diff-layout-camera-asset".to_vec()),
                camera_digest()
            );
            edit.set_input_range("main", LAST_HUNK_FRAME, 1, coleco_only_input())?;
            for index in 0..HUNK_COUNT {
                let input = if index + 1 == HUNK_COUNT {
                    special_input()
                } else {
                    TasInputFrame {
                        players: [TasControllerInput {
                            buttons: (index as u8).wrapping_add(1),
                            dpad: (index as u8).wrapping_mul(3).wrapping_add(1),
                        }; 5],
                        ..TasInputFrame::default()
                    }
                };
                edit.set_input_range(&target, HUNK_BASE + (index as u64) * 2, 1, input)?;
            }
            edit.insert_frames(&target, HUNK_BASE + (HUNK_COUNT as u64) * 2 + 9, 7)?;
            edit.replace_branch_events(
                "main",
                vec![
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 3,
                        side: 0,
                    },
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 89,
                        side: 0,
                    },
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 197,
                        side: 0,
                    },
                ],
            )?;
            edit.replace_branch_events(
                &target,
                vec![
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 3,
                        side: 1,
                    },
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 89,
                        side: 2,
                    },
                    ReplayEvent::FdsDiskSide {
                        frame: HUNK_BASE + 197,
                        side: 3,
                    },
                ],
            )
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    state.inspector_tab = TasInspectorTab::Tools;
    (root, state)
}

fn wait_for_diff(state: &mut TasEditorWindowState, context: &egui::Context) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let session = state.session.as_ref().unwrap();
        match state.branch_diff_editor.refresh(session, context, true) {
            branch_diff_editor::TasBranchDiffPresentation::Ready(_) => return,
            branch_diff_editor::TasBranchDiffPresentation::Failed(error) => {
                panic!("branch diff failed: {error}")
            }
            branch_diff_editor::TasBranchDiffPresentation::NoTarget => {
                panic!("branch diff unexpectedly has no target")
            }
            branch_diff_editor::TasBranchDiffPresentation::Pending => {
                assert!(
                    Instant::now() < deadline,
                    "background branch diff did not finish"
                );
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn open_branch_diff(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    compact: bool,
) {
    if compact {
        assert!(click_after_scrolling(context, state, size, "Branch, markers & more").is_empty());
    }
    assert!(click_after_scrolling(context, state, size, "Branch diff").is_empty());
}

fn assert_branch_diff_rows_are_actionable(size: egui::Vec2, compact: bool) {
    let (_root, mut state) = prepared_state();
    let context = test_context();
    wait_for_diff(&mut state, &context);
    let diff = state.branch_diff_editor.cached_diff().unwrap();
    assert_eq!(diff.input_hunks.len(), HUNK_COUNT);
    assert_eq!(diff.event_hunks.len(), 1);
    assert!(diff.timeline_tail.is_some());
    open_branch_diff(&context, &mut state, size, compact);

    let target = scroll_parent_until_visible(
        &context,
        &mut state,
        size,
        "Target branch with a deliberately long human readable name",
    );
    assert!(
        target.rect.max.x <= size.x + 0.1,
        "long target branch names and ids must remain inside the {size:?} parent"
    );
    let (output, actions) = render_project(&context, &mut state, size, Vec::new());
    assert!(actions.is_empty());
    let target_id = text_witnesses(
        &output,
        "target-id-with-a-deliberately-long-stable-identifier-0123456789",
    )
    .into_iter()
    .find(|witness| witness.clip_rect.contains_rect(witness.rect))
    .expect("long target branch id must be fully visible");
    assert_eq!(target.rect, target_id.rect);
    let summary_controls = scroll_parent_until_visible_together(
        &context,
        &mut state,
        size,
        &["Omitted hunks: 0", "Timeline tail: target alone has frames"],
    );
    assert_non_overlapping_controls(&summary_controls);
    let input_header = scroll_parent_until_visible(&context, &mut state, size, "Input hunks");
    if compact {
        adjust_compact_body_scroll(&context, &mut state, size, input_header.position.y - 240.0);
    }
    let (output, actions) = render_project(&context, &mut state, size, Vec::new());
    assert!(actions.is_empty());
    let input_header = text_witnesses(&output, "Input hunks")
        .into_iter()
        .find(|witness| witness.clip_rect.contains_rect(witness.rect))
        .expect("input hunk header must remain visible after positioning its list");
    assert!(
        click_at(
            &context,
            &mut state,
            size,
            input_header.position + egui::vec2(8.0, input_header.rect.height() * 0.5),
        )
        .is_empty()
    );
    align_parent_label(&context, &mut state, size, "Input hunks");
    let (output, actions) = render_project(&context, &mut state, size, Vec::new());
    assert!(actions.is_empty());
    let source = text_witnesses(&output, "p1=b00/d00")
        .into_iter()
        .next()
        .expect("first source input summary must render");
    let list_position = egui::pos2(source.rect.center().x, source.clip_rect.center().y);
    let output = scroll_input_to_end(&context, &mut state, size, list_position);
    let final_copy = lowest_visible_text(&output, "Copy source input");
    assert_non_overlapping_controls(&[final_copy]);

    let camera_summary = scroll_input_back_to_visible(
        &context,
        &mut state,
        size,
        list_position,
        &camera_digest_label(),
    );
    assert_non_overlapping_rects(&[camera_summary], size);
    assert!(
        camera_summary.clip_rect.contains_rect(camera_summary.rect),
        "the full camera digest must not be clipped"
    );

    let coleco_summary = scroll_input_back_to_visible(
        &context,
        &mut state,
        size,
        list_position,
        coleco_summary_label(),
    );
    assert_non_overlapping_rects(&[coleco_summary], size);
    assert!(
        coleco_summary.clip_rect.contains_rect(coleco_summary.rect),
        "both Coleco controller summaries must be fully visible"
    );

    let final_copy = scroll_input_forward_to_visible(
        &context,
        &mut state,
        size,
        list_position,
        "Copy source input",
    );
    let cached = state.branch_diff_editor.cached_diff().unwrap().clone();
    let copy = click_at(
        &context,
        &mut state,
        size,
        final_copy.position + egui::vec2(8.0, final_copy.rect.height() * 0.5),
    );
    assert_eq!(
        copy,
        vec![TasEditorAction::InputClipboard(
            input_clipboard::TasInputClipboardAction::copy_constant(
                state.session.as_ref().unwrap().project_content_sha256(),
                "main".to_owned(),
                cached.source_movie_sha256,
                LAST_HUNK_FRAME,
                1,
                coleco_only_input(),
            )
            .unwrap(),
        )]
    );

    let (output, actions) = render_project(&context, &mut state, size, Vec::new());
    assert!(actions.is_empty());
    let final_jump = lowest_visible_text(&output, "Jump to source");
    assert_non_overlapping_controls(&[final_jump]);
    let jump = click_at(
        &context,
        &mut state,
        size,
        final_jump.position + egui::vec2(8.0, final_jump.rect.height() * 0.5),
    );
    assert!(matches!(
        jump.as_slice(),
        [TasEditorAction::JumpToBranchDiffHunk(action)] if action.cursor() == LAST_HUNK_FRAME
    ));
}

#[test]
fn branch_diff_rows_stay_actionable_in_the_actual_wide_inspector() {
    assert_branch_diff_rows_are_actionable(WIDE_PARENT, false);
}

#[test]
fn branch_diff_rows_stay_actionable_in_the_actual_compact_inspector() {
    assert_branch_diff_rows_are_actionable(COMPACT_PARENT, true);
}

#[test]
fn comfortable_full_window_keeps_the_selected_end_boundary_visible_after_jump() {
    const FRAME_COUNT: u64 = 1_000_000;
    const JUMP_FRAME: u64 = 200_000;
    const WINDOW_SIZE: egui::Vec2 = egui::vec2(982.0, 672.0);

    let (_root, mut state) = tests::state_with_project(FRAME_COUNT);
    state.open_separate_window();
    let context = test_context_with_density(UiDensity::Comfortable);
    let (_, actions) = render_full_content(&context, &mut state, WINDOW_SIZE, Vec::new());
    assert!(actions.is_empty());
    let jump = {
        let session = state.session.as_ref().unwrap();
        TasEditorAction::JumpToBranchDiffHunk(branch_diff_editor::TasBranchDiffJumpAction::new(
            session.project_content_sha256(),
            session.selected_branch_id().to_owned(),
            session
                .project()
                .branch_movie_sha256(session.selected_branch_id())
                .unwrap(),
            JUMP_FRAME,
        ))
    };
    state.apply(jump);
    let mut output = render_full_content(&context, &mut state, WINDOW_SIZE, Vec::new()).0;
    for _ in 0..2 {
        output = render_full_content(&context, &mut state, WINDOW_SIZE, Vec::new()).0;
    }
    let jump_row = exact_text_witness(&output, &JUMP_FRAME.to_string());
    assert!(jump_row.clip_rect.contains_rect(jump_row.rect));

    let select_end = exact_text_witness(&output, "Select End");
    let actions =
        click_full_content_at(&context, &mut state, WINDOW_SIZE, select_end.rect.center());
    assert_eq!(
        actions.as_slice(),
        &[TasEditorAction::SelectCursor(FRAME_COUNT)]
    );
    for action in actions {
        state.apply(action);
    }
    let mut output = render_full_content(&context, &mut state, WINDOW_SIZE, Vec::new()).0;
    for _ in 0..2 {
        output = render_full_content(&context, &mut state, WINDOW_SIZE, Vec::new()).0;
    }
    let end = exact_text_witness(&output, "End");
    assert!(
        end.clip_rect.contains_rect(end.rect),
        "selected End row must remain fully visible after settled redraws: {end:?}"
    );
}

#[test]
fn coleco_only_hunk_copy_and_jump_witnesses_preserve_source_input() {
    let (_root, mut state) = tests::state_with_project(4);
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "coleco-target".to_owned(),
            name: "Coleco target".to_owned(),
        })
        .unwrap();
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| edit.set_input_range("main", 1, 1, coleco_only_input()))
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    let context = test_context();
    let diff = {
        wait_for_diff(&mut state, &context);
        state.branch_diff_editor.cached_diff().unwrap().clone()
    };
    assert_eq!(diff.input_hunks.len(), 1);
    let hunk = &diff.input_hunks[0];
    assert_eq!(hunk.start, 1);
    assert_eq!(hunk.length, 1);
    assert_eq!(hunk.source_input, coleco_only_input());
    assert_eq!(hunk.target_input, TasInputFrame::default());
    assert!(
        branch_diff_editor::raw_input_summary(hunk.source_input).contains(coleco_summary_label())
    );

    let (copy, jump) = {
        let session = state.session.as_ref().unwrap();
        (
            TasEditorAction::InputClipboard(
                input_clipboard::TasInputClipboardAction::copy_constant(
                    session.project_content_sha256(),
                    session.selected_branch_id().to_owned(),
                    diff.source_movie_sha256,
                    hunk.start,
                    hunk.length,
                    hunk.source_input,
                )
                .unwrap(),
            ),
            TasEditorAction::JumpToBranchDiffHunk(
                branch_diff_editor::TasBranchDiffJumpAction::new(
                    session.project_content_sha256(),
                    session.selected_branch_id().to_owned(),
                    diff.source_movie_sha256,
                    hunk.start,
                ),
            ),
        )
    };
    state.reduce(copy).unwrap();
    assert!(state.input_clipboard.has_entry());
    state.reduce(jump).unwrap();
    assert_eq!(state.session.as_ref().unwrap().cursor(), hunk.start);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let paste = {
        let session = state.session.as_ref().unwrap();
        TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::paste_at_cursor(
            session.project_content_sha256(),
            session.selected_branch_id().to_owned(),
            session
                .project()
                .branch_movie_sha256(session.selected_branch_id())
                .unwrap(),
            2,
            state.input_clipboard.generation(),
        ))
    };
    state.reduce(paste).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(2),
        coleco_only_input()
    );
}
