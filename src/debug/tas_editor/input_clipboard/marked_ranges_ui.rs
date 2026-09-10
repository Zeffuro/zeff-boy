use super::{
    TasInputClipboardAction, TasInputClipboardState,
    marked_ranges::{TasMarkedRangesAction, TasMarkedRangesSnapshot},
};
use crate::{
    debug::tas_editor::{TasEditorAction, timeline_selection::TasInputSelection},
    tas_project::{TasDigitalInputMask, TasDigitalTransform, TasEditorSession},
};

const RANGE_ROW_HEIGHT: f32 = 24.0;
const MAX_RANGE_LIST_HEIGHT: f32 = 120.0;

pub(super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    edit: (TasDigitalInputMask, TasDigitalTransform),
    actions: &mut Vec<TasEditorAction>,
) {
    let (mask, transform) = edit;
    let Some(snapshot) = draw_management(ui, session, state, selection, actions) else {
        return;
    };
    if matches!(transform, TasDigitalTransform::Reverse) {
        ui.small("Reverse each range separately.");
    }
    let range_count = snapshot.ranges.len();
    let enabled = range_count != 0 && super::digital_transform::has_controls(mask);
    let apply_label = format!(
        "Apply {} to {range_count} marked ranges",
        transform_label(transform)
    );
    if ui
        .add_enabled(enabled, egui::Button::new(apply_label))
        .on_disabled_hover_text(if range_count == 0 {
            "Add at least one marked edit range"
        } else {
            "Check at least one control to edit"
        })
        .clicked()
    {
        match session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
        {
            Ok(target_movie_sha256) => actions.push(TasEditorAction::InputClipboard(
                TasInputClipboardAction::MarkedRanges(TasMarkedRangesAction::Apply {
                    snapshot,
                    target_movie_sha256,
                    mask,
                    transform,
                }),
            )),
            Err(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Cannot prepare marked control edit: {error:#}"),
                );
            }
        }
    }
}

pub(super) fn draw_management(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<TasEditorAction>,
) -> Option<TasMarkedRangesSnapshot> {
    let snapshot = match state.marked_ranges.snapshot(session) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Cannot read marked edit ranges: {error:#}"),
            );
            return None;
        }
    };
    let range_count = snapshot.ranges.len();
    let frame_count = snapshot.ranges.iter().fold(0_u64, |total, (start, end)| {
        total.saturating_add(end.saturating_sub(*start))
    });

    ui.strong("Marked edit ranges");
    ui.small(format!(
        "{range_count} ranges · {frame_count} frames. Each marked range is transformed independently; gaps stay unchanged."
    ));
    let add = ui
        .add_enabled(
            selection.is_some(),
            egui::Button::new("Add selection to edit ranges"),
        )
        .on_disabled_hover_text("Select a contiguous timeline range to add")
        .clicked();
    if add {
        actions.push(TasEditorAction::InputClipboard(
            TasInputClipboardAction::MarkedRanges(TasMarkedRangesAction::Add {
                snapshot: snapshot.clone(),
                selection: selection.expect("enabled add requires a selection").clone(),
            }),
        ));
    }

    if range_count != 0 {
        let row_height = RANGE_ROW_HEIGHT
            .max(ui.spacing().interact_size.y)
            .max(ui.text_style_height(&egui::TextStyle::Body));
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("tas_marked_control_ranges")
            .max_height(MAX_RANGE_LIST_HEIGHT)
            .show_rows(ui, row_height, range_count, |ui, rows| {
                for index in rows {
                    let (start, end) = snapshot.ranges[index];
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(format!("Remove range {index}"))
                            .on_hover_text(format!(
                                "Remove frames {start}..{end} from the marked list"
                            ))
                            .clicked()
                        {
                            remove = Some(index);
                        }
                        let label_width = ui.available_width();
                        ui.add_sized(
                            [label_width, row_height],
                            egui::Label::new(format!("Frames {start}..{end}")).truncate(),
                        );
                    });
                }
            });
        if let Some(index) = remove {
            actions.push(TasEditorAction::InputClipboard(
                TasInputClipboardAction::MarkedRanges(TasMarkedRangesAction::Remove {
                    snapshot: snapshot.clone(),
                    index,
                }),
            ));
        }
        if ui.button("Clear marked edit ranges").clicked() {
            actions.push(TasEditorAction::InputClipboard(
                TasInputClipboardAction::MarkedRanges(TasMarkedRangesAction::Clear {
                    snapshot: snapshot.clone(),
                }),
            ));
        }
    }
    Some(snapshot)
}

fn transform_label(transform: TasDigitalTransform) -> &'static str {
    match transform {
        TasDigitalTransform::Clear => "Clear",
        TasDigitalTransform::Invert => "Invert",
        TasDigitalTransform::Reverse => "Reverse",
    }
}
