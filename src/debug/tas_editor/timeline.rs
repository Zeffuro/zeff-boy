use std::collections::BTreeMap;

use super::action::TasTimelineNavigation;
use super::coleco_input::{
    COLECO_CONTROLS, COLECO_KEYPAD_KEYS, control_pressed, is_coleco_project, keypad_label,
};
use super::input_columns::{DigitalField, applicable_player_count, digital_columns};
use super::{ROW_HEIGHT, TasEditorAction};
use crate::tas_project::{TasBranchOrigin, TasEditorSession, TasInputFrame};

const FRAME_GUTTER_WIDTH: f32 = 80.0;
const BRANCH_GUTTER_WIDTH: f32 = 32.0;

pub(super) struct TimelineView {
    pub(super) selected_input_range: Option<(u64, u64)>,
    pub(super) execution_boundary: Option<u64>,
    pub(super) max_height: f32,
    pub(super) follow_cursor: bool,
    pub(super) go_to_selection_available: bool,
}

pub(super) fn draw_timeline(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
    view: TimelineView,
) {
    queue_keyboard_navigation(ui, actions, view.go_to_selection_available);
    let system = session.project().identity().system.as_str();
    let columns = digital_columns(system);
    let player_count = applicable_player_count(session);
    let is_coleco = is_coleco_project(session);
    let cursor = session.cursor();
    let row_count = usize::try_from(session.selected_branch().frame_count())
        .expect("TAS frame limit fits native usize");
    let visible_row_count = row_count.saturating_add(1);
    let branch_points = TimelineBranchPoints::for_selected_branch(session);

    egui::ScrollArea::horizontal()
        .id_salt("tas_editor_timeline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(
                FRAME_GUTTER_WIDTH
                    + BRANCH_GUTTER_WIDTH
                    + 8.0
                    + if is_coleco {
                        2.0 * (24.0 + COLECO_CONTROLS.len() as f32 * 22.0 + 48.0)
                    } else {
                        player_count as f32 * (24.0 + columns.len() as f32 * 22.0)
                    },
            );
            ui.horizontal(|ui| {
                ui.add_sized([FRAME_GUTTER_WIDTH, ROW_HEIGHT], egui::Label::new("Frame"));
                ui.add_sized(
                    [BRANCH_GUTTER_WIDTH, ROW_HEIGHT],
                    egui::Label::new("Fork").truncate(),
                )
                .on_hover_text("Child branches fork at these movie boundaries");
                draw_input_headers(ui, player_count, &columns, is_coleco);
            });
            ui.separator();
            let mut rows_scroll = egui::ScrollArea::vertical()
                .id_salt("tas_editor_timeline_rows")
                .max_height(view.max_height)
                .min_scrolled_height(view.max_height)
                .auto_shrink([false, false]);
            if view.follow_cursor {
                let offset = (cursor as f32 * ROW_HEIGHT - view.max_height * 0.45).max(0.0);
                rows_scroll = rows_scroll.vertical_scroll_offset(offset);
            }
            rows_scroll.show_rows(ui, ROW_HEIGHT, visible_row_count, |ui, rows| {
                for row in rows {
                    let frame = row as u64;
                    paint_execution_boundary(ui, view.execution_boundary, frame);
                    if row == row_count {
                        draw_end_row(ui, actions, cursor, frame, branch_points.at(frame));
                        continue;
                    }
                    let input = session.selected_branch().input_at(frame);
                    ui.horizontal(|ui| {
                        let response = ui
                            .add_sized(
                                [FRAME_GUTTER_WIDTH, ROW_HEIGHT],
                                egui::Button::selectable(
                                    cursor == frame
                                        || view.selected_input_range.is_some_and(|(start, end)| {
                                            (start..end).contains(&frame)
                                        }),
                                    format!("{frame:>8}"),
                                ),
                            )
                            .interact(egui::Sense::click_and_drag());
                        queue_frame_gutter_interaction(
                            ui,
                            response,
                            frame,
                            actions,
                            view.execution_boundary
                                .is_some_and(|boundary| boundary != frame),
                        );
                        draw_branch_point_indicator(ui, frame, branch_points.at(frame));
                        if is_coleco {
                            draw_coleco_input_row(ui, &input, frame, actions);
                        } else {
                            draw_digital_input_row(
                                ui,
                                &input,
                                frame,
                                player_count,
                                &columns,
                                actions,
                            );
                        }
                    });
                }
            });
        });
}

fn draw_input_headers(
    ui: &mut egui::Ui,
    player_count: usize,
    columns: &[super::input_columns::DigitalColumn],
    is_coleco: bool,
) {
    if is_coleco {
        for player in 0..2 {
            ui.separator();
            ui.add_sized(
                [24.0, ROW_HEIGHT],
                egui::Label::new(format!("P{}", player + 1)),
            );
            for (_, label) in COLECO_CONTROLS {
                ui.add_sized([22.0, ROW_HEIGHT], egui::Label::new(label));
            }
            ui.add_sized([48.0, ROW_HEIGHT], egui::Label::new("Key"));
        }
        return;
    }
    for player in 0..player_count {
        ui.separator();
        ui.add_sized(
            [24.0, ROW_HEIGHT],
            egui::Label::new(format!("P{}", player + 1)),
        );
        for column in columns {
            ui.add_sized([22.0, ROW_HEIGHT], egui::Label::new(column.label));
        }
    }
}

fn draw_digital_input_row(
    ui: &mut egui::Ui,
    input: &TasInputFrame,
    frame: u64,
    player_count: usize,
    columns: &[super::input_columns::DigitalColumn],
    actions: &mut Vec<TasEditorAction>,
) {
    for (player, controller) in input.players.iter().take(player_count).enumerate() {
        ui.separator();
        ui.add_sized(
            [24.0, ROW_HEIGHT],
            egui::Label::new(format!("P{}", player + 1)),
        );
        for column in columns {
            let value = match column.field {
                DigitalField::Buttons => controller.buttons,
                DigitalField::Dpad => controller.dpad,
            };
            if ui
                .add_sized(
                    [22.0, ROW_HEIGHT],
                    egui::Button::selectable(value & column.mask != 0, column.label),
                )
                .clicked()
            {
                queue_digital_toggle(actions, frame, player, column.field, column.mask);
            }
        }
    }
}

fn draw_coleco_input_row(
    ui: &mut egui::Ui,
    input: &TasInputFrame,
    frame: u64,
    actions: &mut Vec<TasEditorAction>,
) {
    for (player, controller) in input.coleco.iter().enumerate() {
        ui.separator();
        ui.add_sized(
            [24.0, ROW_HEIGHT],
            egui::Label::new(format!("P{}", player + 1)),
        );
        for (control, label) in COLECO_CONTROLS {
            if ui
                .add_sized(
                    [22.0, ROW_HEIGHT],
                    egui::Button::selectable(control_pressed(*controller, control), label),
                )
                .clicked()
            {
                actions.push(TasEditorAction::SelectTimelineFrame {
                    frame,
                    extend_selection: false,
                });
                actions.push(TasEditorAction::ToggleColecoControl {
                    cursor: frame,
                    player,
                    control,
                });
            }
        }
        let mut keypad = controller.keypad;
        egui::ComboBox::from_id_salt(("tas_editor_coleco_keypad", frame, player))
            .selected_text(keypad_label(keypad))
            .width(44.0)
            .show_ui(ui, |ui| {
                for key in COLECO_KEYPAD_KEYS {
                    ui.selectable_value(&mut keypad, key, keypad_label(key));
                }
            });
        if keypad != controller.keypad {
            actions.push(TasEditorAction::SelectTimelineFrame {
                frame,
                extend_selection: false,
            });
            actions.push(TasEditorAction::SetColecoKeypad {
                cursor: frame,
                player,
                key: keypad,
            });
        }
    }
}

fn queue_keyboard_navigation(
    ui: &egui::Ui,
    actions: &mut Vec<TasEditorAction>,
    go_to_selection_available: bool,
) {
    if !ui.is_enabled() || ui.ctx().memory(|memory| memory.focused().is_some()) {
        return;
    }
    let (navigation, extend_selection, enter) = ui.input(|input| {
        let navigation = [
            (egui::Key::ArrowUp, TasTimelineNavigation::Previous),
            (egui::Key::ArrowDown, TasTimelineNavigation::Next),
            (egui::Key::Home, TasTimelineNavigation::Start),
            (egui::Key::End, TasTimelineNavigation::End),
        ]
        .into_iter()
        .find_map(|(key, navigation)| input.key_pressed(key).then_some(navigation));
        (
            navigation,
            input.modifiers.shift,
            input.key_pressed(egui::Key::Enter),
        )
    });
    if let Some(navigation) = navigation {
        actions.push(TasEditorAction::NavigateTimelineSelection {
            navigation,
            extend_selection,
        });
    }
    if enter && go_to_selection_available {
        actions.push(TasEditorAction::RequestLiveGoToSelection);
    }
}

fn queue_frame_gutter_interaction(
    ui: &egui::Ui,
    response: egui::Response,
    frame: u64,
    actions: &mut Vec<TasEditorAction>,
    go_to_selection_available: bool,
) {
    let drag_anchor_id = egui::Id::new("tas_editor_timeline_gutter_drag_anchor");
    let pointer_down = ui.input(|input| input.pointer.primary_down());
    if !pointer_down {
        ui.ctx()
            .data_mut(|data| data.remove_temp::<u64>(drag_anchor_id));
    }
    if response.drag_started() {
        ui.ctx()
            .data_mut(|data| data.insert_temp(drag_anchor_id, frame));
        actions.push(TasEditorAction::SelectTimelineRange {
            anchor: frame,
            active: frame,
        });
        return;
    }
    let drag_anchor = ui.ctx().data(|data| data.get_temp::<u64>(drag_anchor_id));
    if pointer_down
        && response.hovered()
        && let Some(anchor) = drag_anchor
    {
        actions.push(TasEditorAction::SelectTimelineRange {
            anchor,
            active: frame,
        });
        return;
    }
    if response.double_clicked() {
        actions.push(TasEditorAction::SelectTimelineFrame {
            frame,
            extend_selection: false,
        });
        if go_to_selection_available {
            actions.push(TasEditorAction::RequestLiveGoToSelection);
        }
    } else if response.clicked() {
        actions.push(TasEditorAction::SelectTimelineFrame {
            frame,
            extend_selection: ui.input(|input| input.modifiers.shift),
        });
    }
}

fn paint_execution_boundary(ui: &egui::Ui, execution_boundary: Option<u64>, boundary: u64) {
    if execution_boundary != Some(boundary) {
        return;
    }
    let y = ui.cursor().min.y;
    let color = ui.visuals().selection.stroke.color;
    let clip = ui.clip_rect();
    ui.painter().line_segment(
        [egui::pos2(clip.left(), y), egui::pos2(clip.right(), y)],
        egui::Stroke::new(2.0, color),
    );
    ui.painter().text(
        egui::pos2(clip.left() + 3.0, y + 1.0),
        egui::Align2::LEFT_TOP,
        format!("▶ B({boundary})"),
        egui::FontId::monospace(10.0),
        color,
    );
}

fn draw_end_row(
    ui: &mut egui::Ui,
    actions: &mut Vec<TasEditorAction>,
    cursor: u64,
    frame_count: u64,
    branch_point: Option<&TimelineBranchPoint>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                [FRAME_GUTTER_WIDTH, ROW_HEIGHT],
                egui::Button::selectable(cursor == frame_count, "End"),
            )
            .clicked()
        {
            actions.push(TasEditorAction::SelectCursor(frame_count));
        }
        draw_branch_point_indicator(ui, frame_count, branch_point);
        ui.label(format!(
            "B({frame_count}) · next frame {frame_count} · Record appends here"
        ));
    });
}

struct TimelineBranchPoints {
    by_boundary: BTreeMap<u64, TimelineBranchPoint>,
}

struct TimelineBranchPoint {
    child_branches: Vec<String>,
}

impl TimelineBranchPoints {
    fn for_selected_branch(session: &TasEditorSession) -> Self {
        let project = session.project();
        Self::from_branches(
            session.selected_branch_id(),
            session.selected_branch().frame_count(),
            project
                .branches()
                .iter()
                .map(|branch| (branch.id(), branch.name(), branch.parent())),
        )
    }

    fn from_branches<'a>(
        selected_branch_id: &str,
        selected_frame_count: u64,
        branches: impl Iterator<Item = (&'a str, &'a str, Option<&'a TasBranchOrigin>)>,
    ) -> Self {
        let mut by_boundary = BTreeMap::<u64, TimelineBranchPoint>::new();
        for (id, name, parent) in branches {
            let Some(parent) = parent else {
                continue;
            };
            if parent.branch_id != selected_branch_id || parent.fork_cursor > selected_frame_count {
                continue;
            }
            let label = if id == name {
                name.to_owned()
            } else {
                format!("{name} ({id})")
            };
            by_boundary
                .entry(parent.fork_cursor)
                .or_insert_with(|| TimelineBranchPoint {
                    child_branches: Vec::new(),
                })
                .child_branches
                .push(label);
        }
        for point in by_boundary.values_mut() {
            point.child_branches.sort_unstable();
        }
        Self { by_boundary }
    }

    fn at(&self, boundary: u64) -> Option<&TimelineBranchPoint> {
        self.by_boundary.get(&boundary)
    }
}

fn draw_branch_point_indicator(
    ui: &mut egui::Ui,
    boundary: u64,
    branch_point: Option<&TimelineBranchPoint>,
) {
    let Some(branch_point) = branch_point else {
        ui.allocate_exact_size(
            egui::vec2(BRANCH_GUTTER_WIDTH, ROW_HEIGHT),
            egui::Sense::hover(),
        );
        return;
    };
    let text = if branch_point.child_branches.len() == 1 {
        "↳".to_owned()
    } else {
        format!("↳{}", branch_point.child_branches.len())
    };
    ui.add_sized(
        [BRANCH_GUTTER_WIDTH, ROW_HEIGHT],
        egui::Label::new(egui::RichText::new(text).strong()).sense(egui::Sense::hover()),
    )
    .on_hover_text(format!(
        "Branch point B({boundary})\n{}",
        branch_point.child_branches.join("\n")
    ));
}

pub(super) fn queue_digital_toggle(
    actions: &mut Vec<TasEditorAction>,
    frame: u64,
    player: usize,
    field: DigitalField,
    mask: u8,
) {
    actions.push(TasEditorAction::SelectTimelineFrame {
        frame,
        extend_selection: false,
    });
    actions.push(TasEditorAction::ToggleDigital {
        cursor: frame,
        player,
        field,
        mask,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tas_project::TasDigest;

    fn key_input(key: egui::Key) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..egui::RawInput::default()
        }
    }

    #[test]
    fn disabled_timeline_does_not_queue_keyboard_navigation() {
        let context = egui::Context::default();
        let mut actions = Vec::new();
        let _ = context.run_ui(key_input(egui::Key::ArrowDown), |ui| {
            ui.add_enabled_ui(false, |ui| {
                queue_keyboard_navigation(ui, &mut actions, true);
            });
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn focused_text_field_suppresses_timeline_keyboard_navigation() {
        let context = egui::Context::default();
        let field_id = egui::Id::new("timeline_keyboard_navigation_test_field");
        let mut text = String::new();
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(field_id))
                .request_focus();
        });
        assert_eq!(context.memory(|memory| memory.focused()), Some(field_id));

        let mut actions = Vec::new();
        let _ = context.run_ui(key_input(egui::Key::Home), |ui| {
            queue_keyboard_navigation(ui, &mut actions, true);
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn branch_points_group_children_at_exact_boundaries_including_end() {
        let first = TasBranchOrigin {
            branch_id: "main".to_owned(),
            branch_movie_sha256: TasDigest([1; 32]),
            fork_cursor: 4,
        };
        let second = TasBranchOrigin {
            branch_id: "main".to_owned(),
            branch_movie_sha256: TasDigest([2; 32]),
            fork_cursor: 4,
        };
        let end = TasBranchOrigin {
            branch_id: "main".to_owned(),
            branch_movie_sha256: TasDigest([3; 32]),
            fork_cursor: 12,
        };
        let other_parent = TasBranchOrigin {
            branch_id: "other".to_owned(),
            branch_movie_sha256: TasDigest([4; 32]),
            fork_cursor: 4,
        };
        let past_end = TasBranchOrigin {
            branch_id: "main".to_owned(),
            branch_movie_sha256: TasDigest([5; 32]),
            fork_cursor: 13,
        };

        let points = TimelineBranchPoints::from_branches(
            "main",
            12,
            [
                ("main", "Main", None),
                ("route-z", "Zeta", Some(&first)),
                ("route-a", "Alpha", Some(&second)),
                ("last", "Last Route", Some(&end)),
                ("other", "Other", Some(&other_parent)),
                ("past", "Past", Some(&past_end)),
            ]
            .into_iter(),
        );

        assert_eq!(
            points.at(4).unwrap().child_branches,
            ["Alpha (route-a)", "Zeta (route-z)"]
        );
        assert_eq!(points.at(12).unwrap().child_branches, ["Last Route (last)"]);
        assert!(points.at(13).is_none());
    }
}
