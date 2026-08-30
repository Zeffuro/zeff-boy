use super::input_columns::{DigitalField, applicable_player_count, digital_columns};
use super::{ROW_HEIGHT, TasEditorAction};
use crate::tas_project::TasEditorSession;

pub(super) fn draw_timeline(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
    selected_input_range: Option<(u64, u64)>,
    max_height: f32,
    follow_cursor: bool,
) {
    let system = session.project().identity().system.as_str();
    let columns = digital_columns(system);
    let player_count = applicable_player_count(session);
    let cursor = session.cursor();
    let row_count = usize::try_from(session.selected_branch().frame_count())
        .expect("TAS frame limit fits native usize");

    egui::ScrollArea::horizontal()
        .id_salt("tas_editor_timeline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(88.0 + player_count as f32 * (24.0 + columns.len() as f32 * 22.0));
            ui.horizontal(|ui| {
                ui.add_sized([80.0, ROW_HEIGHT], egui::Label::new("Frame"));
                for player in 0..player_count {
                    ui.separator();
                    ui.add_sized(
                        [24.0, ROW_HEIGHT],
                        egui::Label::new(format!("P{}", player + 1)),
                    );
                    for column in &columns {
                        ui.add_sized([22.0, ROW_HEIGHT], egui::Label::new(column.label));
                    }
                }
            });
            ui.separator();
            let mut rows_scroll = egui::ScrollArea::vertical()
                .id_salt("tas_editor_timeline_rows")
                .max_height(max_height)
                .min_scrolled_height(max_height)
                .auto_shrink([false, false]);
            if follow_cursor {
                let offset = (cursor as f32 * ROW_HEIGHT - max_height * 0.45).max(0.0);
                rows_scroll = rows_scroll.vertical_scroll_offset(offset);
            }
            rows_scroll.show_rows(ui, ROW_HEIGHT, row_count, |ui, rows| {
                for row in rows {
                    let frame = row as u64;
                    let input = session.selected_branch().input_at(frame);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized(
                                [80.0, ROW_HEIGHT],
                                egui::Button::selectable(
                                    cursor == frame
                                        || selected_input_range.is_some_and(|(start, end)| {
                                            (start..end).contains(&frame)
                                        }),
                                    format!("{frame:>8}"),
                                ),
                            )
                            .clicked()
                        {
                            actions.push(TasEditorAction::SelectCursor(frame));
                        }
                        for (player, controller) in
                            input.players.iter().take(player_count).enumerate()
                        {
                            ui.separator();
                            ui.add_sized(
                                [24.0, ROW_HEIGHT],
                                egui::Label::new(format!("P{}", player + 1)),
                            );
                            for column in &columns {
                                let value = match column.field {
                                    DigitalField::Buttons => controller.buttons,
                                    DigitalField::Dpad => controller.dpad,
                                };
                                if ui
                                    .add_sized(
                                        [22.0, ROW_HEIGHT],
                                        egui::Button::selectable(
                                            value & column.mask != 0,
                                            column.label,
                                        ),
                                    )
                                    .clicked()
                                {
                                    queue_digital_toggle(
                                        actions,
                                        cursor,
                                        frame,
                                        player,
                                        column.field,
                                        column.mask,
                                    );
                                }
                            }
                        }
                    });
                }
            });
        });
}

pub(super) fn queue_digital_toggle(
    actions: &mut Vec<TasEditorAction>,
    selected_cursor: u64,
    frame: u64,
    player: usize,
    field: DigitalField,
    mask: u8,
) {
    if selected_cursor != frame {
        actions.push(TasEditorAction::SelectCursor(frame));
    }
    actions.push(TasEditorAction::ToggleDigital {
        cursor: frame,
        player,
        field,
        mask,
    });
}
