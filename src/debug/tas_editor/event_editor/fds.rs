use anyhow::{Result, bail};
use zeff_emu_common::media::{MediaEvent, MediaObjectId, MediaSlotId};
use zeff_emu_common::replay::ReplayEvent;

use super::{TasEventAction, TasEventEditorState, TasEventMutation};
use crate::debug::tas_editor::TasEditorAction;
use crate::tas_project::{TasDigest, TasFirmwareIdentity, TasProjectIdentity};

const FDS_BIOS_FIRMWARE_ID: &str = "nintendo.fds.bios";
const FDS_DRIVE_SLOT_ID: &str = "fds.drive0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::debug::tas_editor) enum FdsMediaMutation {
    SetWriteProtected { write_protected: bool },
    Eject,
    Insert { side: u8, write_protected: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FdsDraftKind {
    SelectSide,
    SetWriteProtected,
    Eject,
    Insert,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FdsDraft {
    pub(super) canonical_index: Option<usize>,
    expected_event: Option<ReplayEvent>,
    frame: u64,
    kind: FdsDraftKind,
    side: u8,
    write_protected: bool,
}

impl FdsDraft {
    pub(super) fn new(frame: u64) -> Self {
        Self {
            canonical_index: None,
            expected_event: None,
            frame,
            kind: FdsDraftKind::SelectSide,
            side: 0,
            write_protected: false,
        }
    }

    fn mutation(&self) -> Option<FdsMediaMutation> {
        match self.kind {
            FdsDraftKind::SelectSide => None,
            FdsDraftKind::SetWriteProtected => Some(FdsMediaMutation::SetWriteProtected {
                write_protected: self.write_protected,
            }),
            FdsDraftKind::Eject => Some(FdsMediaMutation::Eject),
            FdsDraftKind::Insert => Some(FdsMediaMutation::Insert {
                side: self.side,
                write_protected: self.write_protected,
            }),
        }
    }

    pub(super) fn from_event(canonical_index: usize, event: ReplayEvent) -> Option<Self> {
        let (frame, kind, side, write_protected) = match &event {
            ReplayEvent::FdsDiskSide { frame, side } => {
                (*frame, FdsDraftKind::SelectSide, *side, false)
            }
            ReplayEvent::Media {
                frame,
                event:
                    MediaEvent::SetWriteProtected {
                        write_protected, ..
                    },
                ..
            } => (*frame, FdsDraftKind::SetWriteProtected, 0, *write_protected),
            ReplayEvent::Media {
                frame,
                event: MediaEvent::Eject { .. },
                ..
            } => (*frame, FdsDraftKind::Eject, 0, false),
            ReplayEvent::Media {
                frame,
                event:
                    MediaEvent::Insert {
                        side: Some(side),
                        write_protected,
                        ..
                    },
                ..
            } => (*frame, FdsDraftKind::Insert, *side, *write_protected),
            _ => return None,
        };
        Some(Self {
            canonical_index: Some(canonical_index),
            expected_event: Some(event),
            frame,
            kind,
            side,
            write_protected,
        })
    }
}

pub(in crate::debug::tas_editor) fn can_author_fds_events(identity: &TasProjectIdentity) -> bool {
    identity.system == "nes"
        && identity.firmware.iter().any(|firmware| {
            matches!(
                firmware,
                TasFirmwareIdentity::External { firmware_id, .. }
                    if firmware_id == FDS_BIOS_FIRMWARE_ID
            )
        })
}

pub(in crate::debug::tas_editor) fn project_media_id(
    identity: &TasProjectIdentity,
) -> MediaObjectId {
    MediaObjectId::new(format!(
        "sha256:{}",
        identity.effective_media_sha256.to_hex()
    ))
}

pub(super) fn media_event(
    frame: u64,
    mutation: FdsMediaMutation,
    project_media_id: MediaObjectId,
) -> ReplayEvent {
    let slot = MediaSlotId::from(FDS_DRIVE_SLOT_ID);
    let event = match mutation {
        FdsMediaMutation::SetWriteProtected { write_protected } => MediaEvent::SetWriteProtected {
            slot,
            write_protected,
        },
        FdsMediaMutation::Eject => MediaEvent::Eject { slot },
        FdsMediaMutation::Insert {
            side,
            write_protected,
        } => MediaEvent::Insert {
            slot,
            media_id: project_media_id,
            side: Some(side),
            write_protected,
        },
    };
    ReplayEvent::Media {
        frame,
        sequence: 0,
        event,
    }
}

pub(in crate::debug::tas_editor) fn is_editable_event(
    event: &ReplayEvent,
    project_media_id: &MediaObjectId,
) -> bool {
    match event {
        ReplayEvent::FdsDiskSide { .. } => true,
        ReplayEvent::Media {
            sequence: 0,
            event: MediaEvent::SetWriteProtected { slot, .. } | MediaEvent::Eject { slot },
            ..
        } => slot.as_ref() == FDS_DRIVE_SLOT_ID,
        ReplayEvent::Media {
            sequence: 0,
            event:
                MediaEvent::Insert {
                    slot,
                    media_id,
                    side: Some(_),
                    ..
                },
            ..
        } => slot.as_ref() == FDS_DRIVE_SLOT_ID && media_id == project_media_id,
        _ => false,
    }
}

pub(in crate::debug::tas_editor) fn validate_timeline(
    events: &[ReplayEvent],
    frame_count: u64,
    project_media_id: &MediaObjectId,
) -> Result<()> {
    let mut previous_frame = None;
    let mut inserted = true;
    for event in events
        .iter()
        .filter(|event| is_editable_event(event, project_media_id))
    {
        let frame = event.frame();
        if frame >= frame_count || previous_frame.is_some_and(|previous| previous >= frame) {
            bail!("FDS drive events must be non-terminal and occupy distinct frame boundaries");
        }
        match event {
            ReplayEvent::FdsDiskSide { .. }
            | ReplayEvent::Media {
                event: MediaEvent::SetWriteProtected { .. },
                ..
            } if !inserted => bail!("FDS drive event requires inserted project media"),
            ReplayEvent::Media {
                event: MediaEvent::Eject { .. },
                ..
            } => {
                if !inserted {
                    bail!("FDS drive is already ejected");
                }
                inserted = false;
            }
            ReplayEvent::Media {
                event: MediaEvent::Insert { .. },
                ..
            } => {
                if inserted {
                    bail!("FDS project media is already inserted");
                }
                inserted = true;
            }
            _ => {}
        }
        previous_frame = Some(frame);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct FdsDrawContext<'a> {
    pub(super) project_sha256: TasDigest,
    pub(super) branch_id: &'a str,
    pub(super) frame_count: u64,
}

pub(super) fn draw_form(
    ui: &mut egui::Ui,
    context: FdsDrawContext<'_>,
    can_add: bool,
    state: &mut TasEventEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    let last_frame = context.frame_count.saturating_sub(1);
    egui::Grid::new("tas_fds_event_editor").show(ui, |ui| {
        ui.label("Frame boundary");
        ui.add(egui::DragValue::new(&mut state.draft.frame).range(0..=last_frame));
        ui.end_row();
        ui.label("FDS event");
        egui::ComboBox::from_id_salt("tas_fds_event_kind")
            .selected_text(match state.draft.kind {
                FdsDraftKind::SelectSide => "Select side",
                FdsDraftKind::SetWriteProtected => "Set write protection",
                FdsDraftKind::Eject => "Eject",
                FdsDraftKind::Insert => "Insert project disk",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.draft.kind,
                    FdsDraftKind::SelectSide,
                    "Select side",
                );
                ui.selectable_value(
                    &mut state.draft.kind,
                    FdsDraftKind::SetWriteProtected,
                    "Set write protection",
                );
                ui.selectable_value(&mut state.draft.kind, FdsDraftKind::Eject, "Eject");
                ui.selectable_value(
                    &mut state.draft.kind,
                    FdsDraftKind::Insert,
                    "Insert project disk",
                );
            });
        ui.end_row();
        if matches!(
            state.draft.kind,
            FdsDraftKind::SelectSide | FdsDraftKind::Insert
        ) {
            ui.label("Disk side (raw u8)");
            ui.add(egui::DragValue::new(&mut state.draft.side).range(0..=u8::MAX));
            ui.end_row();
        }
        if matches!(
            state.draft.kind,
            FdsDraftKind::SetWriteProtected | FdsDraftKind::Insert
        ) {
            ui.label("Write protected");
            ui.checkbox(&mut state.draft.write_protected, "Enabled");
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        if let (Some(canonical_index), Some(expected_event)) = (
            state.draft.canonical_index,
            state.draft.expected_event.clone(),
        ) {
            if ui.button("Update FDS event").clicked() {
                let event_mutation = if let Some(mutation) = state.draft.mutation() {
                    TasEventMutation::UpdateMedia {
                        branch_id: context.branch_id.to_owned(),
                        canonical_index,
                        expected_event,
                        frame: state.draft.frame,
                        mutation,
                    }
                } else {
                    TasEventMutation::Update {
                        branch_id: context.branch_id.to_owned(),
                        canonical_index,
                        expected_event,
                        frame: state.draft.frame,
                        side: state.draft.side,
                    }
                };
                actions.push(TasEditorAction::Event(TasEventAction::new(
                    context.project_sha256,
                    event_mutation,
                )));
            }
            if ui.button("Cancel edit").clicked() {
                state.draft = FdsDraft::new(state.draft.frame.min(last_frame));
            }
        } else if ui
            .add_enabled(can_add, egui::Button::new("Add FDS event"))
            .clicked()
        {
            let event_mutation = state.draft.mutation().map_or_else(
                || TasEventMutation::Add {
                    branch_id: context.branch_id.to_owned(),
                    frame: state.draft.frame,
                    side: state.draft.side,
                },
                |mutation| TasEventMutation::AddMedia {
                    branch_id: context.branch_id.to_owned(),
                    frame: state.draft.frame,
                    mutation,
                },
            );
            actions.push(TasEditorAction::Event(TasEventAction::new(
                context.project_sha256,
                event_mutation,
            )));
        }
    });
}
