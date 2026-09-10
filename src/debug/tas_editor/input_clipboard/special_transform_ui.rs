use super::{
    TasInputClipboardAction, TasInputClipboardState, marked_ranges_ui,
    special_ranges::{TasSpecialRangeAction, TasSpecialRangeTarget},
};
use crate::{
    debug::tas_editor::{
        TasEditorAction,
        special_input_editor::{TasSpecialInputCapabilities, special_input_capabilities},
        timeline_selection::TasInputSelection,
    },
    tas_project::{TasEditorSession, TasSpecialInputMask, TasSpecialTransform},
};

pub(super) struct TasSpecialTransformUiState {
    mask: TasSpecialInputMask,
    transform: TasSpecialTransform,
}

impl TasSpecialTransformUiState {
    pub(super) fn new() -> Self {
        Self {
            mask: TasSpecialInputMask::default(),
            transform: TasSpecialTransform::Clear,
        }
    }
}

pub(in super::super) fn draw(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.collapsing("Edit recorded special input ranges", |ui| {
        let capabilities = special_input_capabilities(session.project().identity());
        let settings = &mut state.special_transform;
        sanitize_mask(&mut settings.mask, capabilities);
        if !has_special_capability(capabilities) {
            ui.small("This project declares no recorded special input channels.");
            return;
        }

        if capabilities.nes_zapper {
            ui.checkbox(&mut settings.mask.zapper, "Zapper");
        }
        if capabilities.mbc7_tilt || capabilities.gba_tilt {
            ui.horizontal_wrapped(|ui| {
                ui.small("Recorded tilt");
                ui.checkbox(&mut settings.mask.tilt_x, "X");
                ui.checkbox(&mut settings.mask.tilt_y, "Y");
            });
        }
        if capabilities.pocket_camera {
            ui.checkbox(&mut settings.mask.camera, "Camera");
            ui.small("Camera edits move or remove sample updates. The last sample stays active until another update; stored assets are kept.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Operation");
            egui::ComboBox::from_id_salt("tas_special_transform")
                .selected_text(transform_label(settings.transform))
                .show_ui(ui, |ui| {
                    for transform in [TasSpecialTransform::Clear, TasSpecialTransform::Reverse] {
                        ui.selectable_value(
                            &mut settings.transform,
                            transform,
                            transform_label(transform),
                        );
                    }
                });
        });
        ui.small(match settings.transform {
            TasSpecialTransform::Clear => {
                "Clear sets selected axes to zero and removes selected Zapper and Camera values."
            }
            TasSpecialTransform::Reverse => "Reverse each range separately.",
        });
        ui.small("Unchecked channels, replay events, and gaps stay at their original frames.");

        let mask = settings.mask;
        let transform = settings.transform;
        let selection_target = selection.cloned();
        let selection_apply_label = format!("Apply {} to selection", transform_label(transform));
        if ui
            .add_enabled(
                !mask_is_empty(mask) && selection_target.is_some(),
                egui::Button::new(selection_apply_label),
            )
            .on_disabled_hover_text(if mask_is_empty(mask) {
                "Check at least one recorded special input channel"
            } else {
                "Select a contiguous timeline range to edit"
            })
            .clicked()
        {
            queue_apply(
                ui,
                session,
                actions,
                TasSpecialRangeTarget::Selection(
                    selection_target.expect("enabled special selection apply requires a selection"),
                ),
                mask,
                transform,
            );
        }

        let snapshot = ui
            .push_id("tas_special_transform_marked_ranges", |ui| {
                marked_ranges_ui::draw_management(ui, session, state, selection, actions)
            })
            .inner;
        let Some(snapshot) = snapshot else {
            return;
        };
        let range_count = snapshot.ranges.len();
        let marked_apply_label = format!(
            "Apply {} to {range_count} marked ranges",
            transform_label(transform)
        );
        if ui
            .add_enabled(
                !mask_is_empty(mask) && range_count != 0,
                egui::Button::new(marked_apply_label),
            )
            .on_disabled_hover_text(if mask_is_empty(mask) {
                "Check at least one recorded special input channel"
            } else {
                "Add at least one marked edit range"
            })
            .clicked()
        {
            queue_apply(
                ui,
                session,
                actions,
                TasSpecialRangeTarget::Marked(snapshot),
                mask,
                transform,
            );
        }
    });
}

fn sanitize_mask(mask: &mut TasSpecialInputMask, capabilities: TasSpecialInputCapabilities) {
    if !capabilities.nes_zapper {
        mask.zapper = false;
    }
    if !capabilities.mbc7_tilt && !capabilities.gba_tilt {
        mask.tilt_x = false;
        mask.tilt_y = false;
    }
    if !capabilities.pocket_camera {
        mask.camera = false;
    }
}

fn has_special_capability(capabilities: TasSpecialInputCapabilities) -> bool {
    capabilities.nes_zapper
        || capabilities.mbc7_tilt
        || capabilities.gba_tilt
        || capabilities.pocket_camera
}

fn mask_is_empty(mask: TasSpecialInputMask) -> bool {
    !mask.zapper && !mask.tilt_x && !mask.tilt_y && !mask.camera
}

fn queue_apply(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    actions: &mut Vec<TasEditorAction>,
    target: TasSpecialRangeTarget,
    mask: TasSpecialInputMask,
    transform: TasSpecialTransform,
) {
    match session
        .project()
        .branch_movie_sha256(session.selected_branch_id())
    {
        Ok(target_movie_sha256) => actions.push(TasEditorAction::InputClipboard(
            TasInputClipboardAction::ApplySpecialTransform(TasSpecialRangeAction {
                expected_project_sha256: session.project_content_sha256(),
                target_branch_id: session.selected_branch_id().to_owned(),
                target_movie_sha256,
                target,
                mask,
                transform,
            }),
        )),
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Cannot prepare special input edit: {error:#}"),
            );
        }
    }
}

fn transform_label(transform: TasSpecialTransform) -> &'static str {
    match transform {
        TasSpecialTransform::Clear => "Clear",
        TasSpecialTransform::Reverse => "Reverse",
    }
}
