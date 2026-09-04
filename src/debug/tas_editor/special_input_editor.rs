use std::collections::BTreeMap;

use anyhow::{Result, bail};
use zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES;

use super::{TasEditorAction, TasEditorWindowState};
use crate::tas_project::{
    TasCameraInput, TasDigest, TasEditorSession, TasInputFrame, TasProjectIdentity, TasZapperInput,
};

pub(super) const NES_ZAPPER_DEVICE: &str = "nes-zapper";
pub(super) const GAME_BOY_MBC7_DEVICE: &str = "game-boy-mbc7";
pub(super) const GAME_BOY_POCKET_CAMERA_DEVICE: &str = "game-boy-pocket-camera";
pub(super) const GBA_TILT_DEVICE: &str = "gba-tilt-sensor";

const ASSET_ROW_HEIGHT: f32 = 22.0;
const ASSET_LIST_HEIGHT: f32 = 132.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TasSpecialInputMutation {
    NesZapper(TasZapperInput),
    RecordedTilt { x_bits: u32, y_bits: u32 },
    PocketCamera(TasCameraInput),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct TasSpecialInputAction {
    expected_project_sha256: TasDigest,
    branch_id: String,
    cursor: u64,
    mutation: TasSpecialInputMutation,
}

impl TasSpecialInputAction {
    pub(super) fn new(
        expected_project_sha256: TasDigest,
        branch_id: String,
        cursor: u64,
        mutation: TasSpecialInputMutation,
    ) -> Self {
        Self {
            expected_project_sha256,
            branch_id,
            cursor,
            mutation,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TasSpecialInputCapabilities {
    pub(super) nes_zapper: bool,
    pub(super) mbc7_tilt: bool,
    pub(super) gba_tilt: bool,
    pub(super) pocket_camera: bool,
}

impl TasSpecialInputCapabilities {
    fn recorded_tilt_device(self) -> Option<(&'static str, &'static str)> {
        if self.mbc7_tilt {
            Some((GAME_BOY_MBC7_DEVICE, "Game Boy MBC7 recorded tilt"))
        } else if self.gba_tilt {
            Some((GBA_TILT_DEVICE, "GBA cartridge recorded tilt"))
        } else {
            None
        }
    }
}

pub(super) fn special_input_capabilities(
    identity: &TasProjectIdentity,
) -> TasSpecialInputCapabilities {
    let has_device = |expected: &str| {
        identity
            .devices
            .iter()
            .any(|device| device.device == expected)
    };
    TasSpecialInputCapabilities {
        nes_zapper: matches!(identity.system.as_str(), "nes") && has_device(NES_ZAPPER_DEVICE),
        mbc7_tilt: matches!(identity.system.as_str(), "gb" | "game_boy")
            && has_device(GAME_BOY_MBC7_DEVICE),
        gba_tilt: identity.system == "gba" && has_device(GBA_TILT_DEVICE),
        pocket_camera: matches!(identity.system.as_str(), "gb" | "game_boy")
            && has_device(GAME_BOY_POCKET_CAMERA_DEVICE),
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_special_input_action(
        &mut self,
        action: TasSpecialInputAction,
    ) -> Result<String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if session.project_content_sha256() != action.expected_project_sha256 {
            bail!("TAS project changed after this special-input edit was drafted; retry it");
        }
        if session.selected_branch_id() != action.branch_id {
            bail!(
                "selected TAS branch changed after this special-input edit was drafted; retry it"
            );
        }
        if action.cursor >= session.selected_branch().frame_count() {
            bail!("cannot edit special input at the end cursor");
        }

        let capabilities = special_input_capabilities(session.project().identity());
        let mut replacement = session.selected_branch().input_at(action.cursor);
        let previous_camera = replacement.camera;
        let label = apply_mutation(&mut replacement, action.mutation, capabilities)?;
        if let TasCameraInput::Blob(digest) = replacement.camera {
            let bytes =
                session.project().assets().get(&digest).ok_or_else(|| {
                    anyhow::anyhow!("TAS input references a missing camera asset")
                })?;
            if !camera_asset_is_authorable(bytes) && replacement.camera != previous_camera {
                bail!(
                    "Pocket Camera input requires an exact {POCKET_CAMERA_FRAME_BYTES}-byte (128x112) project asset"
                );
            }
        }
        let branch_id = action.branch_id;
        let cursor = action.cursor;
        let outcome = session.edit_transaction(move |edit| {
            edit.set_input_range(&branch_id, cursor, 1, replacement)
        })?;
        if outcome.changed {
            self.execution_preview.clear();
            let detached = self.detach_incompatible_execution().map_or_else(
                String::new,
                |error| format!("; private execution detached because the edited project is outside its profile: {error:#}"),
            );
            Ok(format!("Updated {label} at frame {cursor}{detached}"))
        } else {
            Ok(format!("No {label} change at frame {cursor}"))
        }
    }
}

fn apply_mutation(
    input: &mut TasInputFrame,
    mutation: TasSpecialInputMutation,
    capabilities: TasSpecialInputCapabilities,
) -> Result<&'static str> {
    match mutation {
        TasSpecialInputMutation::NesZapper(zapper) => {
            ensure_editable_or_clearing(
                capabilities.nes_zapper,
                input.zapper != TasZapperInput::default(),
                zapper == TasZapperInput::default(),
                "NES Zapper",
            )?;
            input.zapper = zapper;
            Ok("NES Zapper recorded input")
        }
        TasSpecialInputMutation::RecordedTilt { x_bits, y_bits } => {
            ensure_editable_or_clearing(
                capabilities.recorded_tilt_device().is_some(),
                (input.tilt_x_bits, input.tilt_y_bits) != (0, 0),
                (x_bits, y_bits) == (0, 0),
                "recorded tilt",
            )?;
            input.tilt_x_bits = x_bits;
            input.tilt_y_bits = y_bits;
            Ok(capabilities
                .recorded_tilt_device()
                .map_or("recorded tilt", |(_, label)| label))
        }
        TasSpecialInputMutation::PocketCamera(camera) => {
            ensure_editable_or_clearing(
                capabilities.pocket_camera,
                input.camera != TasCameraInput::None,
                camera == TasCameraInput::None,
                "Game Boy Pocket Camera",
            )?;
            input.camera = camera;
            Ok("Game Boy Pocket Camera recorded frame")
        }
    }
}

fn ensure_editable_or_clearing(
    capability: bool,
    currently_nondefault: bool,
    replacement_is_default: bool,
    label: &str,
) -> Result<()> {
    if capability || (currently_nondefault && replacement_is_default) {
        Ok(())
    } else {
        bail!("project identity does not declare the {label} device")
    }
}

pub(super) fn camera_asset_is_authorable(bytes: &[u8]) -> bool {
    bytes.len() == POCKET_CAMERA_FRAME_BYTES
}

pub(super) fn ensure_nondefault_input_authorable(
    identity: &TasProjectIdentity,
    assets: &BTreeMap<TasDigest, Vec<u8>>,
    input: TasInputFrame,
) -> Result<()> {
    let capabilities = special_input_capabilities(identity);
    if input.zapper != TasZapperInput::default() && !capabilities.nes_zapper {
        bail!("project identity does not declare the NES Zapper device");
    }
    if (input.tilt_x_bits, input.tilt_y_bits) != (0, 0)
        && capabilities.recorded_tilt_device().is_none()
    {
        bail!("project identity does not declare a supported recorded tilt device");
    }
    if let TasCameraInput::Blob(digest) = input.camera {
        if !capabilities.pocket_camera {
            bail!("project identity does not declare the Game Boy Pocket Camera device");
        }
        let bytes = assets
            .get(&digest)
            .ok_or_else(|| anyhow::anyhow!("TAS input references a missing camera asset"))?;
        if !camera_asset_is_authorable(bytes) {
            bail!(
                "Pocket Camera input requires an exact {POCKET_CAMERA_FRAME_BYTES}-byte (128x112) project asset"
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct DrawContext<'a> {
    project_sha256: TasDigest,
    branch_id: &'a str,
    cursor: u64,
}

pub(super) fn draw_special_input_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
) {
    let cursor = session.cursor();
    let frame_count = session.selected_branch().frame_count();
    let input = session.selected_branch().input_at(cursor);
    let capabilities = special_input_capabilities(session.project().identity());
    let has_stored_special = input.zapper != TasZapperInput::default()
        || (input.tilt_x_bits, input.tilt_y_bits) != (0, 0)
        || input.camera != TasCameraInput::None;
    let has_capability = capabilities != TasSpecialInputCapabilities::default();

    ui.collapsing("Recorded special input", |ui| {
        ui.small(
            "Edits materialized movie data only; no host device is sampled. Private execution may reject or detach for unsupported profiles.",
        );
        if cursor >= frame_count {
            ui.label("Move the cursor before branch end to edit a recorded input frame.");
            return;
        }
        if !has_capability && !has_stored_special {
            ui.label("The project identity declares no editable special-input device.");
            return;
        }

        let context = DrawContext {
            project_sha256: session.project_content_sha256(),
            branch_id: session.selected_branch_id(),
            cursor,
        };
        if capabilities.nes_zapper || input.zapper != TasZapperInput::default() {
            draw_zapper(ui, context, input.zapper, capabilities.nes_zapper, actions);
        }
        if let Some((device, label)) = capabilities.recorded_tilt_device() {
            draw_tilt(
                ui,
                context,
                input,
                TiltPresentation {
                    device,
                    label,
                    editable: true,
                },
                actions,
            );
        } else if (input.tilt_x_bits, input.tilt_y_bits) != (0, 0) {
            draw_tilt(
                ui,
                context,
                input,
                TiltPresentation {
                    device: "declared tilt",
                    label: "Recorded tilt",
                    editable: false,
                },
                actions,
            );
        }
        if capabilities.pocket_camera || input.camera != TasCameraInput::None {
            draw_camera(ui, session, context, input.camera, capabilities.pocket_camera, actions);
        }
    });
}

fn draw_zapper(
    ui: &mut egui::Ui,
    context: DrawContext<'_>,
    current: TasZapperInput,
    editable: bool,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.separator();
    ui.label("NES Zapper recorded input");
    if !editable {
        ui.label(format!(
            "Stored without a declared {NES_ZAPPER_DEVICE:?} device: enabled={}, trigger={}, hit={}, position={:?}",
            current.enabled, current.trigger, current.hit, current.screen_pos
        ));
        clear_button(
            ui,
            context,
            "Clear unsupported Zapper input",
            TasSpecialInputMutation::NesZapper(TasZapperInput::default()),
            actions,
        );
        return;
    }

    let mut edited = current;
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut edited.enabled, "Enabled");
        ui.checkbox(&mut edited.trigger, "Trigger pressed");
        ui.checkbox(&mut edited.hit, "Recorded light hit");
    });
    let mut has_position = edited.screen_pos.is_some();
    ui.checkbox(&mut has_position, "Recorded screen position");
    edited.screen_pos = if has_position {
        let [mut x, mut y] = edited.screen_pos.unwrap_or([0, 0]);
        ui.horizontal_wrapped(|ui| {
            ui.label("X (raw u16)");
            ui.add(egui::DragValue::new(&mut x).range(0..=u16::MAX));
            ui.label("Y (raw u16)");
            ui.add(egui::DragValue::new(&mut y).range(0..=u16::MAX));
        });
        Some([x, y])
    } else {
        None
    };
    if edited != current {
        push_action(actions, context, TasSpecialInputMutation::NesZapper(edited));
    }
}

#[derive(Clone, Copy)]
struct TiltPresentation<'a> {
    device: &'a str,
    label: &'a str,
    editable: bool,
}

fn draw_tilt(
    ui: &mut egui::Ui,
    context: DrawContext<'_>,
    input: TasInputFrame,
    presentation: TiltPresentation<'_>,
    actions: &mut Vec<TasEditorAction>,
) {
    let TiltPresentation {
        device,
        label,
        editable,
    } = presentation;
    ui.separator();
    ui.label(format!("{label} — exact IEEE-754 payload bits"));
    if !editable {
        ui.monospace(format!(
            "Stored without a declared {device:?} device: X=0x{:08X}, Y=0x{:08X}",
            input.tilt_x_bits, input.tilt_y_bits
        ));
        clear_button(
            ui,
            context,
            "Clear unsupported recorded tilt",
            TasSpecialInputMutation::RecordedTilt {
                x_bits: 0,
                y_bits: 0,
            },
            actions,
        );
        return;
    }

    let (mut x_bits, mut y_bits) = (input.tilt_x_bits, input.tilt_y_bits);
    ui.horizontal_wrapped(|ui| {
        ui.label("X bits");
        ui.add(egui::DragValue::new(&mut x_bits).hexadecimal(8, false, true));
        ui.label(format!("({:?})", f32::from_bits(x_bits)));
        ui.label("Y bits");
        ui.add(egui::DragValue::new(&mut y_bits).hexadecimal(8, false, true));
        ui.label(format!("({:?})", f32::from_bits(y_bits)));
    });
    if (x_bits, y_bits) != (input.tilt_x_bits, input.tilt_y_bits) {
        push_action(
            actions,
            context,
            TasSpecialInputMutation::RecordedTilt { x_bits, y_bits },
        );
    }
}

fn draw_camera(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    context: DrawContext<'_>,
    current: TasCameraInput,
    editable: bool,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.separator();
    ui.label("Game Boy Pocket Camera recorded frame asset");
    if !editable {
        let TasCameraInput::Blob(digest) = current else {
            return;
        };
        ui.monospace(format!(
            "Stored without a declared {GAME_BOY_POCKET_CAMERA_DEVICE:?} device: {}",
            digest.to_hex()
        ));
        clear_button(
            ui,
            context,
            "Clear unsupported Pocket Camera frame",
            TasSpecialInputMutation::PocketCamera(TasCameraInput::None),
            actions,
        );
        return;
    }

    let current_label = match current {
        TasCameraInput::None => "None".to_owned(),
        TasCameraInput::Blob(digest) => format!("{}…", &digest.to_hex()[..16]),
    };
    ui.label(format!("Selected asset: {current_label}"));
    if ui
        .selectable_label(current == TasCameraInput::None, "None")
        .clicked()
    {
        push_action(
            actions,
            context,
            TasSpecialInputMutation::PocketCamera(TasCameraInput::None),
        );
    }
    let assets = session.project().assets();
    egui::ScrollArea::vertical()
        .id_salt("tas_special_camera_assets")
        .max_height(ASSET_LIST_HEIGHT)
        .show_rows(ui, ASSET_ROW_HEIGHT, assets.len(), |ui, rows| {
            for (&digest, bytes) in assets.iter().skip(rows.start).take(rows.len()) {
                let selected = current == TasCameraInput::Blob(digest);
                let exact_size = camera_asset_is_authorable(bytes);
                let suffix = if exact_size { "" } else { " — wrong size" };
                let label = format!(
                    "{}… ({} bytes){suffix}",
                    &digest.to_hex()[..16],
                    bytes.len()
                );
                if ui
                    .add_enabled(exact_size, egui::Button::selectable(selected, label))
                    .on_disabled_hover_text(format!(
                        "Recorded Pocket Camera frames require exactly {POCKET_CAMERA_FRAME_BYTES} bytes (128x112)"
                    ))
                    .clicked()
                {
                    push_action(
                        actions,
                        context,
                        TasSpecialInputMutation::PocketCamera(TasCameraInput::Blob(digest)),
                    );
                }
            }
        });
    if assets.is_empty() {
        ui.small("No camera assets are stored in this project.");
    }
}

fn clear_button(
    ui: &mut egui::Ui,
    context: DrawContext<'_>,
    label: &str,
    mutation: TasSpecialInputMutation,
    actions: &mut Vec<TasEditorAction>,
) {
    if ui.button(label).clicked() {
        push_action(actions, context, mutation);
    }
}

fn push_action(
    actions: &mut Vec<TasEditorAction>,
    context: DrawContext<'_>,
    mutation: TasSpecialInputMutation,
) {
    actions.push(TasEditorAction::SpecialInput(TasSpecialInputAction::new(
        context.project_sha256,
        context.branch_id.to_owned(),
        context.cursor,
        mutation,
    )));
}
