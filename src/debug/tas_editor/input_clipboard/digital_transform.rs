use anyhow::{Result, bail};

use super::{TasInputClipboardAction, TasInputClipboardState, TasInputPatternPasteWitness};
use crate::{
    debug::tas_editor::{
        TasEditorAction, TasEditorWindowState,
        input_columns::{DigitalField, applicable_player_count, digital_columns},
        timeline_selection::TasInputSelection,
    },
    tas_project::{
        TasControllerInput, TasDigest, TasDigitalInputMask, TasDigitalTransform, TasEditorSession,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct TasDigitalTransformAction {
    pub(in super::super) expected_project_sha256: TasDigest,
    pub(in super::super) target_branch_id: String,
    pub(in super::super) target_movie_sha256: TasDigest,
    pub(in super::super) selection: TasInputSelection,
    pub(in super::super) mask: TasDigitalInputMask,
    pub(in super::super) transform: TasDigitalTransform,
}

pub(super) struct TasDigitalTransformUiState {
    mask: TasDigitalInputMask,
    transform: TasDigitalTransform,
    player: usize,
}

impl TasDigitalTransformUiState {
    pub(super) fn new() -> Self {
        let mut mask = TasDigitalInputMask::default();
        mask.players[0].buttons = 1;
        Self {
            mask,
            transform: TasDigitalTransform::Clear,
            player: 0,
        }
    }
}

fn allowed_controls(session: &TasEditorSession) -> TasControllerInput {
    let mut allowed = TasControllerInput::default();
    for column in digital_columns(&session.project().identity().system) {
        match column.field {
            DigitalField::Buttons => allowed.buttons |= column.mask,
            DigitalField::Dpad => allowed.dpad |= column.mask,
        }
    }
    allowed
}

pub(super) fn has_controls(mask: TasDigitalInputMask) -> bool {
    mask.players
        .iter()
        .any(|player| player.buttons != 0 || player.dpad != 0)
}

pub(in super::super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.collapsing("Edit selected controls", |ui| {
        let system = &session.project().identity().system;
        if matches!(system.as_str(), "coleco" | "colecovision") {
            ui.small("Selected-control edits are unavailable for ColecoVision projects.");
            return;
        }
        let player_count = applicable_player_count(session);
        let columns = digital_columns(system);
        let allowed = allowed_controls(session);
        let settings = &mut state.digital_transform;
        for (player, mask) in settings.mask.players.iter_mut().enumerate() {
            if player >= player_count {
                *mask = TasControllerInput::default();
                continue;
            }
            mask.buttons &= allowed.buttons;
            mask.dpad &= allowed.dpad;
        }
        settings.player = settings.player.min(player_count.saturating_sub(1));
        egui::ComboBox::from_id_salt("tas_digital_transform_player")
            .selected_text(format!("Player {}", settings.player + 1))
            .show_ui(ui, |ui| {
                for player in 0..player_count {
                    ui.selectable_value(
                        &mut settings.player,
                        player,
                        format!("Player {}", player + 1),
                    );
                }
            });
        let mask = &mut settings.mask.players[settings.player];
        for (field, label, bits) in [
            (DigitalField::Dpad, "Directions", &mut mask.dpad),
            (DigitalField::Buttons, "Buttons", &mut mask.buttons),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.small(label);
                for column in columns.iter().filter(|column| column.field == field) {
                    let mut checked = *bits & column.mask != 0;
                    if ui.checkbox(&mut checked, column.label).changed() {
                        if checked {
                            *bits |= column.mask;
                        } else {
                            *bits &= !column.mask;
                        }
                    }
                }
            });
        }
        let counts = settings
            .mask
            .players
            .iter()
            .enumerate()
            .filter(|(_, mask)| mask.buttons != 0 || mask.dpad != 0)
            .map(|(player, mask)| {
                let count = mask.buttons.count_ones() + mask.dpad.count_ones();
                format!("P{}: {count}", player + 1)
            })
            .collect::<Vec<_>>()
            .join(" · ");
        ui.small(format!(
            "Controls checked: {}",
            if counts.is_empty() { "none" } else { &counts }
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label("Operation");
            egui::ComboBox::from_id_salt("tas_digital_transform")
                .selected_text(transform_label(settings.transform))
                .show_ui(ui, |ui| {
                    for operation in [
                        TasDigitalTransform::Clear,
                        TasDigitalTransform::Invert,
                        TasDigitalTransform::Reverse,
                    ] {
                        ui.selectable_value(
                            &mut settings.transform,
                            operation,
                            transform_label(operation),
                        );
                    }
                });
        });
        ui.small(match settings.transform {
            TasDigitalTransform::Clear => {
                "Clear releases the checked controls throughout the selection."
            }
            TasDigitalTransform::Invert => {
                "Invert swaps pressed and released for the checked controls."
            }
            TasDigitalTransform::Reverse => {
                "Reverse reorders only the checked controls within the selection."
            }
        });
        ui.small("Other controls, sensor input and replay events stay at their original frames.");
        let mask = settings.mask;
        let transform = settings.transform;
        if let Some(selection) = selection {
            if ui
                .add_enabled(
                    has_controls(mask),
                    egui::Button::new("Apply control edit to selection"),
                )
                .on_disabled_hover_text("Check at least one control to edit")
                .clicked()
            {
                match session
                    .project()
                    .branch_movie_sha256(session.selected_branch_id())
                {
                    Ok(target_movie_sha256) => actions.push(TasEditorAction::InputClipboard(
                        TasInputClipboardAction::ApplyDigitalTransform(TasDigitalTransformAction {
                            expected_project_sha256: session.project_content_sha256(),
                            target_branch_id: session.selected_branch_id().to_owned(),
                            target_movie_sha256,
                            selection: selection.clone(),
                            mask,
                            transform,
                        }),
                    )),
                    Err(error) => {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("Cannot prepare control edit: {error:#}"),
                        );
                    }
                }
            }
        } else {
            ui.small("Select a contiguous timeline range to edit its controls.");
        }
        ui.separator();
        super::marked_ranges_ui::draw(ui, session, state, selection, (mask, transform), actions);
        ui.separator();
        super::masked_paste::draw(ui, session, state, selection, mask, actions);
    });
}

fn transform_label(transform: TasDigitalTransform) -> &'static str {
    match transform {
        TasDigitalTransform::Clear => "Clear",
        TasDigitalTransform::Invert => "Invert",
        TasDigitalTransform::Reverse => "Reverse",
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_digital_transform(
        &mut self,
        action: TasDigitalTransformAction,
    ) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if self.timeline_selection.snapshot(session).as_ref() != Some(&action.selection) {
            bail!("input selection changed after this control edit was requested; retry it");
        }
        validate_action(session, &action)?;
        let pattern = session
            .selected_branch()
            .input_pattern(action.selection.start, action.selection.length())?
            .with_digital_transform(action.mask, action.transform)?;
        let operation = match action.transform {
            TasDigitalTransform::Clear => "Cleared selected controls",
            TasDigitalTransform::Invert => "Inverted selected controls",
            TasDigitalTransform::Reverse => "Reversed selected controls",
        };
        self.apply_pattern_at_cursor(
            TasInputPatternPasteWitness {
                expected_project_sha256: action.expected_project_sha256,
                target_branch_id: action.target_branch_id,
                target_movie_sha256: action.target_movie_sha256,
                expected_cursor: None,
                operation,
                no_change_message: "Control edit made no change",
            },
            action.selection.start,
            pattern,
            Vec::new(),
            false,
        )
    }
}

fn validate_action(session: &TasEditorSession, action: &TasDigitalTransformAction) -> Result<()> {
    if session.project_content_sha256() != action.expected_project_sha256
        || session.selected_branch_id() != action.target_branch_id
        || session
            .project()
            .branch_movie_sha256(&action.target_branch_id)?
            != action.target_movie_sha256
        || action.selection.branch_id != action.target_branch_id
    {
        bail!("TAS project or branch changed after this control edit was requested; retry it");
    }
    if action.selection.start >= action.selection.end
        || action.selection.end > session.selected_branch().frame_count()
    {
        bail!("select a valid non-empty input range before editing controls");
    }
    validate_mask(session, action.mask)
}

pub(super) fn validate_mask(session: &TasEditorSession, mask: TasDigitalInputMask) -> Result<()> {
    if matches!(
        session.project().identity().system.as_str(),
        "coleco" | "colecovision"
    ) {
        bail!("selected-control edits do not support ColecoVision control encoding");
    }
    let allowed = allowed_controls(session);
    let player_count = applicable_player_count(session);
    for (player, mask) in mask.players.iter().enumerate() {
        if (player >= player_count && *mask != TasControllerInput::default())
            || mask.buttons & !allowed.buttons != 0
            || mask.dpad & !allowed.dpad != 0
        {
            bail!("the TAS project does not declare one of the selected controls");
        }
    }
    if !has_controls(mask) {
        bail!("check at least one control to edit");
    }
    Ok(())
}
