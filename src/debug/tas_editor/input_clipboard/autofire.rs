use anyhow::{Result, bail};

use super::{TasInputClipboardAction, TasInputClipboardState};
use crate::{
    debug::tas_editor::{
        TasEditorAction,
        input_columns::{DigitalField, applicable_player_count, digital_columns},
        timeline_selection::TasInputSelection,
    },
    tas_project::{TasDigest, TasEditorSession, TasInputPattern},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct TasDigitalAutofireAction {
    pub(in super::super) expected_project_sha256: TasDigest,
    pub(in super::super) target_branch_id: String,
    pub(in super::super) target_movie_sha256: TasDigest,
    pub(in super::super) selection: TasInputSelection,
    pub(in super::super) player: usize,
    pub(in super::super) buttons_mask: u8,
    pub(in super::super) dpad_mask: u8,
    pub(in super::super) period: u8,
    pub(in super::super) on_frames: u8,
}

pub(super) struct TasDigitalAutofireUiState {
    pub(super) player: usize,
    pub(super) column: usize,
    pub(super) period: u8,
    pub(super) on_frames: u8,
}

impl TasDigitalAutofireUiState {
    pub(super) fn new() -> Self {
        Self {
            player: 0,
            column: 4,
            period: 2,
            on_frames: 1,
        }
    }
}

pub(super) fn build_pattern(
    session: &TasEditorSession,
    action: &TasDigitalAutofireAction,
) -> Result<TasInputPattern> {
    validate_action(session, action)?;
    session
        .selected_branch()
        .input_pattern(action.selection.start, action.selection.length())?
        .with_digital_autofire(
            action.player,
            action.buttons_mask,
            action.dpad_mask,
            action.period,
            action.on_frames,
        )
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.strong("Digital autofire");
    let system = &session.project().identity().system;
    if matches!(system.as_str(), "coleco" | "colecovision") {
        ui.small("Digital autofire is unavailable for ColecoVision projects.");
        return;
    }
    let columns = digital_columns(system);
    let player_count = applicable_player_count(session);
    state.autofire.player = state.autofire.player.min(player_count.saturating_sub(1));
    state.autofire.column = state.autofire.column.min(columns.len().saturating_sub(1));
    state.autofire.period = state.autofire.period.clamp(1, 60);
    state.autofire.on_frames = state.autofire.on_frames.min(state.autofire.period);

    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("tas_digital_autofire_player")
            .selected_text(format!("Player {}", state.autofire.player + 1))
            .show_ui(ui, |ui| {
                for player in 0..player_count {
                    ui.selectable_value(
                        &mut state.autofire.player,
                        player,
                        format!("Player {}", player + 1),
                    );
                }
            });
        egui::ComboBox::from_id_salt("tas_digital_autofire_control")
            .selected_text(
                columns
                    .get(state.autofire.column)
                    .map_or("—", |column| column.label),
            )
            .show_ui(ui, |ui| {
                for (index, column) in columns.iter().enumerate() {
                    ui.selectable_value(&mut state.autofire.column, index, column.label);
                }
            });
        ui.label("Period");
        ui.add(egui::DragValue::new(&mut state.autofire.period).range(1..=60));
        ui.label("frames");
        state.autofire.on_frames = state.autofire.on_frames.min(state.autofire.period);
        ui.label("ON");
        let period = state.autofire.period;
        ui.add(egui::DragValue::new(&mut state.autofire.on_frames).range(0..=period));
        ui.label("frames");
    });

    ui.small(format!(
        "Preview: ON × {} · OFF × {} · {}-frame cycle from the first selected frame.",
        state.autofire.on_frames,
        state.autofire.period - state.autofire.on_frames,
        state.autofire.period
    ));

    let Some(selection) = selection else {
        ui.small("Select a contiguous timeline range to apply digital autofire.");
        return;
    };
    let Some(column) = columns.get(state.autofire.column).copied() else {
        ui.small("This project declares no digital controls for autofire.");
        return;
    };
    if !ui.button("Apply digital autofire to selection").clicked() {
        return;
    }
    match session
        .project()
        .branch_movie_sha256(session.selected_branch_id())
    {
        Ok(target_movie_sha256) => {
            let (buttons_mask, dpad_mask) = match column.field {
                DigitalField::Buttons => (column.mask, 0),
                DigitalField::Dpad => (0, column.mask),
            };
            actions.push(TasEditorAction::InputClipboard(
                TasInputClipboardAction::ApplyDigitalAutofire(TasDigitalAutofireAction {
                    expected_project_sha256: session.project_content_sha256(),
                    target_branch_id: session.selected_branch_id().to_owned(),
                    target_movie_sha256,
                    selection: selection.clone(),
                    player: state.autofire.player,
                    buttons_mask,
                    dpad_mask,
                    period: state.autofire.period,
                    on_frames: state.autofire.on_frames,
                }),
            ));
        }
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Cannot prepare digital autofire: {error:#}"),
            );
        }
    }
}

fn validate_action(session: &TasEditorSession, action: &TasDigitalAutofireAction) -> Result<()> {
    if session.project_content_sha256() != action.expected_project_sha256 {
        bail!("TAS project changed after this autofire was requested; retry it");
    }
    if session.selected_branch_id() != action.target_branch_id {
        bail!("selected TAS branch changed after this autofire was requested; retry it");
    }
    if session
        .project()
        .branch_movie_sha256(&action.target_branch_id)?
        != action.target_movie_sha256
    {
        bail!("autofire target branch changed after this autofire was requested; retry it");
    }
    if action.selection.branch_id != action.target_branch_id {
        bail!("autofire selection branch changed after this autofire was requested; retry it");
    }
    if action.selection.start >= action.selection.end {
        bail!("select at least one input frame before applying autofire");
    }
    if action.selection.end > session.selected_branch().frame_count() {
        bail!("autofire selection changed after this autofire was requested; retry it");
    }
    if matches!(
        session.project().identity().system.as_str(),
        "coleco" | "colecovision"
    ) {
        bail!("digital autofire does not support ColecoVision control encoding");
    }
    if action.player >= applicable_player_count(session) {
        bail!("the TAS project does not declare that autofire player");
    }
    let columns = digital_columns(&session.project().identity().system);
    let buttons = columns
        .iter()
        .filter(|column| column.field == DigitalField::Buttons)
        .fold(0_u8, |mask, column| mask | column.mask);
    let dpad = columns
        .iter()
        .filter(|column| column.field == DigitalField::Dpad)
        .fold(0_u8, |mask, column| mask | column.mask);
    if action.buttons_mask & !buttons != 0 || action.dpad_mask & !dpad != 0 {
        bail!("the TAS project does not declare that digital autofire control");
    }
    if action.buttons_mask == 0 && action.dpad_mask == 0 {
        bail!("select at least one digital control for autofire");
    }
    if !(1..=60).contains(&action.period) {
        bail!("autofire period must be between 1 and 60 frames");
    }
    if action.on_frames > action.period {
        bail!("autofire ON frames must not exceed its period");
    }
    Ok(())
}
