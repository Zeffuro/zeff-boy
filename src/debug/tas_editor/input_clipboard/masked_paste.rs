use anyhow::{Result, bail};

use super::{
    TasInputClipboardAction, TasInputClipboardState, TasInputPatternPasteWitness,
    digital_transform::{has_controls, validate_mask},
};
use crate::{
    debug::tas_editor::{
        TasEditorAction, TasEditorWindowState, timeline_selection::TasInputSelection,
    },
    tas_project::{TasDigest, TasDigitalInputMask, TasEditorSession},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) enum TasMaskedPasteDestination {
    Cursor(u64),
    Selection(TasInputSelection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct TasMaskedPasteAction {
    pub(in super::super) expected_project_sha256: TasDigest,
    pub(in super::super) target_branch_id: String,
    pub(in super::super) target_movie_sha256: TasDigest,
    pub(in super::super) clipboard_generation: u64,
    pub(in super::super) mask: TasDigitalInputMask,
    pub(in super::super) destination: TasMaskedPasteDestination,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    mask: TasDigitalInputMask,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.strong("Paste checked controls");
    let Some(entry) = state.entry.as_ref() else {
        ui.small("Copy input to paste or tile only the checked controls.");
        return;
    };
    ui.small("Only checked digital controls are copied. Other input and all replay events stay in place.");
    let cursor = session.cursor();
    let fits = cursor
        .checked_add(entry.pattern.length())
        .is_some_and(|end| end <= session.selected_branch().frame_count());
    let enabled = has_controls(mask);
    let paste = ui
        .add_enabled(
            enabled && fits,
            egui::Button::new("Paste checked controls at cursor"),
        )
        .on_disabled_hover_text(if enabled {
            "The copied input must fit within this branch"
        } else {
            "Check at least one control to paste"
        })
        .clicked();
    let tile = ui
        .add_enabled(
            enabled && selection.is_some(),
            egui::Button::new("Tile checked controls across selection"),
        )
        .on_disabled_hover_text(if enabled {
            "Select a contiguous timeline range to tile"
        } else {
            "Check at least one control to tile"
        })
        .clicked();
    let destination = if paste {
        TasMaskedPasteDestination::Cursor(cursor)
    } else if tile {
        TasMaskedPasteDestination::Selection(
            selection.expect("tiling requires a selection").clone(),
        )
    } else {
        return;
    };
    match session
        .project()
        .branch_movie_sha256(session.selected_branch_id())
    {
        Ok(target_movie_sha256) => actions.push(TasEditorAction::InputClipboard(
            TasInputClipboardAction::PasteMaskedControls(TasMaskedPasteAction {
                expected_project_sha256: session.project_content_sha256(),
                target_branch_id: session.selected_branch_id().to_owned(),
                target_movie_sha256,
                clipboard_generation: state.generation,
                mask,
                destination,
            }),
        )),
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Cannot prepare control paste: {error:#}"),
            );
        }
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_masked_paste(&mut self, action: TasMaskedPasteAction) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if session.project_content_sha256() != action.expected_project_sha256
            || session.selected_branch_id() != action.target_branch_id
            || session
                .project()
                .branch_movie_sha256(&action.target_branch_id)?
                != action.target_movie_sha256
        {
            bail!("TAS project or branch changed after this control paste was requested; retry it");
        }
        validate_mask(session, action.mask)?;
        let entry = self.input_clipboard.entry(action.clipboard_generation)?;
        let (start, source, expected_cursor, operation) = match &action.destination {
            TasMaskedPasteDestination::Cursor(cursor) => {
                if session.cursor() != *cursor {
                    bail!(
                        "selected TAS cursor changed after this control paste was requested; retry it"
                    );
                }
                (
                    *cursor,
                    entry.pattern.clone(),
                    Some(*cursor),
                    "Pasted checked controls",
                )
            }
            TasMaskedPasteDestination::Selection(selection) => {
                if self.timeline_selection.snapshot(session).as_ref() != Some(selection)
                    || selection.branch_id != action.target_branch_id
                {
                    bail!(
                        "input selection changed after this control paste was requested; retry it"
                    );
                }
                if selection.start >= selection.end
                    || selection.end > session.selected_branch().frame_count()
                {
                    bail!("select a valid non-empty input range before tiling controls");
                }
                (
                    selection.start,
                    entry.pattern.tile_to_length(selection.length())?,
                    None,
                    "Tiled checked controls",
                )
            }
        };
        let pattern = session
            .selected_branch()
            .input_pattern(start, source.length())?
            .with_digital_overlay(&source, action.mask)?;
        self.apply_pattern_at_cursor(
            TasInputPatternPasteWitness {
                expected_project_sha256: action.expected_project_sha256,
                target_branch_id: action.target_branch_id,
                target_movie_sha256: action.target_movie_sha256,
                expected_cursor,
                operation,
                no_change_message: "Control paste made no change",
            },
            start,
            pattern,
            Vec::new(),
            false,
        )
    }
}
