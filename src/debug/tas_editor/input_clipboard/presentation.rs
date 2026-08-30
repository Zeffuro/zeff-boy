use anyhow::{Result, bail};

use super::{
    TasInputClipboardAction, TasInputClipboardCopyAction, TasInputClipboardPasteAction,
    TasInputClipboardState, TasInputClipboardTileAction,
};
use crate::{
    debug::tas_editor::{TasEditorAction, branch_diff_editor::raw_input_summary},
    tas_project::{TasEditorSession, TasInputPattern},
};

const PATTERN_ROW_HEIGHT: f32 = 22.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputSelection {
    pub(super) branch_id: String,
    pub(super) start: u64,
    pub(super) end: u64,
}

impl TasInputSelection {
    pub(super) fn new(branch_id: String, cursor: u64) -> Self {
        Self {
            branch_id,
            start: cursor,
            end: cursor,
        }
    }

    pub(super) fn length(&self) -> Option<u64> {
        self.end.checked_sub(self.start)
    }
}

pub(super) fn draw_input_clipboard(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    actions: &mut Vec<TasEditorAction>,
) -> Option<(u64, u64)> {
    state.sync_selection(session);
    let frame_count = session.selected_branch().frame_count();
    ui.collapsing("Input pattern", |ui| {
        draw_selection_controls(ui, session, state, frame_count);
        let selection = state.selection().clone();
        let selection_is_ordered = selection.start <= selection.end;
        let selection_length = selection.length();
        if !selection_is_ordered {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Start is after End; no selection action will run.",
            );
        } else if selection_length == Some(0) {
            ui.small("Selection is empty.");
        } else {
            ui.small(format!(
                "Selected frames {}..{}",
                selection.start, selection.end
            ));
        }
        let can_copy = selection_is_ordered && selection_length.is_some_and(|length| length != 0);
        if ui
            .add_enabled(can_copy, egui::Button::new("Copy selection"))
            .clicked()
        {
            match copy_selection_action(session, &selection) {
                Ok(action) => actions.push(TasEditorAction::InputClipboard(action)),
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
            }
        }

        let Some(entry) = state.entry.as_ref() else {
            ui.small("Copy a retained source input hunk or a non-empty selection.");
            return;
        };
        ui.separator();
        ui.small(format!(
            "Source branch {} [{}] frames {}..{}; {} sparse spans",
            entry.source_branch_id,
            &entry.source_movie_sha256.to_hex()[..12],
            entry.start,
            entry.start.saturating_add(entry.pattern.length()),
            entry.pattern.spans().len()
        ));
        draw_pattern_spans(ui, &entry.pattern);
        match session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
        {
            Ok(target_movie_sha256) => {
                let cursor = session.cursor();
                let cursor_fits = cursor
                    .checked_add(entry.pattern.length())
                    .is_some_and(|end| end <= frame_count);
                ui.small(if cursor_fits {
                    format!("Pattern fits at selected cursor {cursor}.")
                } else {
                    format!("Pattern does not fit at selected cursor {cursor}.")
                });
                if ui
                    .add_enabled(cursor_fits, egui::Button::new("Paste at selected cursor"))
                    .clicked()
                {
                    actions.push(TasEditorAction::InputClipboard(
                        TasInputClipboardAction::PasteAtCursor(TasInputClipboardPasteAction {
                            expected_project_sha256: session.project_content_sha256(),
                            target_branch_id: session.selected_branch_id().to_owned(),
                            target_movie_sha256,
                            cursor,
                            clipboard_generation: state.generation,
                        }),
                    ));
                }
                let tile_fits =
                    selection_is_ordered && selection_length.is_some_and(|length| length != 0);
                if ui
                    .add_enabled(tile_fits, egui::Button::new("Tile across selection"))
                    .clicked()
                {
                    actions.push(TasEditorAction::InputClipboard(
                        TasInputClipboardAction::TileAcrossSelection(TasInputClipboardTileAction {
                            expected_project_sha256: session.project_content_sha256(),
                            target_branch_id: session.selected_branch_id().to_owned(),
                            target_movie_sha256,
                            selection,
                            clipboard_generation: state.generation,
                        }),
                    ));
                }
            }
            Err(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Cannot prepare pattern operation: {error:#}"),
                );
            }
        }
    });
    let selection = state.selection();
    (selection.start <= selection.end)
        .then_some((selection.start, selection.end))
        .filter(|(start, end)| start != end)
}

fn draw_selection_controls(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    frame_count: u64,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Start");
        ui.add(
            egui::DragValue::new(&mut state.selection.as_mut().unwrap().start)
                .range(0..=frame_count),
        );
        if ui.small_button("Start = cursor").clicked() {
            state.selection.as_mut().unwrap().start = session.cursor();
        }
        ui.label("End (exclusive)");
        ui.add(
            egui::DragValue::new(&mut state.selection.as_mut().unwrap().end).range(0..=frame_count),
        );
        if ui.small_button("End = cursor").clicked() {
            state.selection.as_mut().unwrap().end = session.cursor();
        }
    });
}

fn copy_selection_action(
    session: &TasEditorSession,
    selection: &TasInputSelection,
) -> Result<TasInputClipboardAction> {
    let length = selection
        .length()
        .ok_or_else(|| anyhow::anyhow!("input selection start is after its exclusive end"))?;
    if length == 0 {
        bail!("input selection is empty");
    }
    let source = session
        .project()
        .branch(&selection.branch_id)
        .ok_or_else(|| anyhow::anyhow!("input selection branch no longer exists"))?;
    Ok(TasInputClipboardAction::CopyPattern(
        TasInputClipboardCopyAction {
            expected_project_sha256: session.project_content_sha256(),
            source_branch_id: selection.branch_id.clone(),
            source_movie_sha256: session
                .project()
                .branch_movie_sha256(&selection.branch_id)?,
            start: selection.start,
            pattern: source.input_pattern(selection.start, length)?,
            expected_selection: Some(selection.clone()),
        },
    ))
}

fn draw_pattern_spans(ui: &mut egui::Ui, pattern: &TasInputPattern) {
    egui::ScrollArea::vertical()
        .id_salt("tas_input_pattern_spans")
        .max_height(112.0)
        .show_rows(ui, PATTERN_ROW_HEIGHT, pattern.spans().len(), |ui, rows| {
            for row in rows {
                let span = pattern.spans()[row];
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(format!(
                        "{}..{}",
                        span.start,
                        span.start.saturating_add(span.length)
                    ));
                    ui.label(raw_input_summary(span.input));
                });
            }
        });
    if pattern.spans().is_empty() {
        ui.small("All frames in this pattern are neutral.");
    }
}
