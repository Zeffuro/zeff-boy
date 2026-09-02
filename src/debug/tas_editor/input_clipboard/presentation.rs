use anyhow::{Result, bail};

use super::{
    TasInputClipboardAction, TasInputClipboardCopyAction, TasInputClipboardPasteAction,
    TasInputClipboardState, TasInputClipboardTileAction,
};
use crate::{
    debug::tas_editor::{
        TasEditorAction, branch_diff_editor::raw_input_summary,
        timeline_selection::TasInputSelection,
    },
    tas_project::{MAX_PROJECT_FRAMES, TasEditorSession, TasInputPattern},
};

const PATTERN_ROW_HEIGHT: f32 = 22.0;

pub(super) fn draw_input_clipboard(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<TasEditorAction>,
) {
    let frame_count = session.selected_branch().frame_count();
    ui.collapsing("Input pattern", |ui| {
        let selection = selection.cloned();
        if let Some(selection) = selection.as_ref() {
            ui.small(format!(
                "Selected frames {}..{}",
                selection.start, selection.end
            ));
        } else {
            ui.small("Select frames in the timeline to copy or tile them.");
        }
        let can_copy = selection.is_some();
        if ui
            .add_enabled(can_copy, egui::Button::new("Copy selection"))
            .clicked()
        {
            match copy_selection_action(
                session,
                selection
                    .as_ref()
                    .expect("copy requires a timeline selection"),
            ) {
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
                let insertion_fits = cursor
                    .checked_add(entry.pattern.length())
                    .is_some_and(|end| end <= MAX_PROJECT_FRAMES);
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
                if ui
                    .add_enabled(
                        insertion_fits,
                        egui::Button::new("Insert copied frames at selected cursor"),
                    )
                    .on_hover_text("Insert frames without replacing existing movie input")
                    .clicked()
                {
                    actions.push(TasEditorAction::InputClipboard(
                        TasInputClipboardAction::InsertAtCursor(TasInputClipboardPasteAction {
                            expected_project_sha256: session.project_content_sha256(),
                            target_branch_id: session.selected_branch_id().to_owned(),
                            target_movie_sha256,
                            cursor,
                            clipboard_generation: state.generation,
                        }),
                    ));
                }
                let tile_fits = selection.is_some();
                if ui
                    .add_enabled(tile_fits, egui::Button::new("Tile across selection"))
                    .clicked()
                {
                    actions.push(TasEditorAction::InputClipboard(
                        TasInputClipboardAction::TileAcrossSelection(TasInputClipboardTileAction {
                            expected_project_sha256: session.project_content_sha256(),
                            target_branch_id: session.selected_branch_id().to_owned(),
                            target_movie_sha256,
                            selection: selection
                                .clone()
                                .expect("tiling requires a timeline selection"),
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
}

fn copy_selection_action(
    session: &TasEditorSession,
    selection: &TasInputSelection,
) -> Result<TasInputClipboardAction> {
    let length = selection.length();
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
