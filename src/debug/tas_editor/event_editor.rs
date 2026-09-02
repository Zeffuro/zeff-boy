use anyhow::{Result, bail};
use zeff_emu_common::media::MediaObjectId;
use zeff_emu_common::replay::ReplayEvent;

use super::{TasEditorAction, TasEditorWindowState};
use crate::tas_project::{TasDigest, TasEditorSession};

mod fds;
#[cfg(test)]
pub(super) use fds::is_editable_event;
use fds::{FdsDraft, FdsDrawContext};
pub(super) use fds::{FdsMediaMutation, can_author_fds_events};

const EVENT_ROW_HEIGHT: f32 = 22.0;
const EVENT_LIST_HEIGHT: f32 = 176.0;
const FDS_BIOS_FIRMWARE_ID: &str = "nintendo.fds.bios";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TasEventMutation {
    Add {
        branch_id: String,
        frame: u64,
        side: u8,
    },
    Update {
        branch_id: String,
        canonical_index: usize,
        expected_event: ReplayEvent,
        frame: u64,
        side: u8,
    },
    AddMedia {
        branch_id: String,
        frame: u64,
        mutation: FdsMediaMutation,
    },
    UpdateMedia {
        branch_id: String,
        canonical_index: usize,
        expected_event: ReplayEvent,
        frame: u64,
        mutation: FdsMediaMutation,
    },
    Remove {
        branch_id: String,
        canonical_index: usize,
        expected_event: ReplayEvent,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasEventAction {
    expected_project_sha256: TasDigest,
    mutation: TasEventMutation,
}

impl TasEventAction {
    pub(super) fn new(expected_project_sha256: TasDigest, mutation: TasEventMutation) -> Self {
        Self {
            expected_project_sha256,
            mutation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EditorContext {
    project_sha256: TasDigest,
    branch_id: String,
}

pub(super) struct TasEventEditorState {
    context: Option<EditorContext>,
    draft: FdsDraft,
}

impl TasEventEditorState {
    pub(super) fn new() -> Self {
        Self {
            context: None,
            draft: FdsDraft::new(0),
        }
    }

    pub(super) fn clear(&mut self) {
        self.context = None;
        self.draft = FdsDraft::new(0);
    }

    fn sync_context(&mut self, session: &TasEditorSession) {
        let context = EditorContext {
            project_sha256: session.project_content_sha256(),
            branch_id: session.selected_branch_id().to_owned(),
        };
        if self.context.as_ref() == Some(&context) {
            return;
        }
        self.draft = FdsDraft::new(session.cursor());
        self.context = Some(context);
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_event_action(&mut self, action: TasEventAction) -> Result<String> {
        let (label, outcome) = {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            if session.project_content_sha256() != action.expected_project_sha256 {
                bail!("TAS project changed after this event edit was drafted; retry it");
            }

            let selected_branch_id = session.selected_branch_id().to_owned();
            let frame_count = session.selected_branch().frame_count();
            let project_media_id = fds::project_media_id(session.project().identity());
            let mut events = session.selected_branch().events().to_vec();
            let label = match action.mutation {
                TasEventMutation::Add {
                    branch_id,
                    frame,
                    side,
                } => {
                    ensure_selected_branch(&selected_branch_id, &branch_id)?;
                    if !can_author_fds_events(session.project().identity()) {
                        bail!(
                            "adding FDS disk-side events requires a NES project with declared {FDS_BIOS_FIRMWARE_ID} firmware"
                        );
                    }
                    events.push(ReplayEvent::FdsDiskSide { frame, side });
                    "Added FDS disk-side event"
                }
                TasEventMutation::Update {
                    branch_id,
                    canonical_index,
                    expected_event,
                    frame,
                    side,
                } => {
                    ensure_selected_branch(&selected_branch_id, &branch_id)?;
                    let event = exact_fds_event_mut(
                        &mut events,
                        canonical_index,
                        &expected_event,
                        &project_media_id,
                    )?;
                    *event = ReplayEvent::FdsDiskSide { frame, side };
                    "Updated FDS disk-side event"
                }
                TasEventMutation::AddMedia {
                    branch_id,
                    frame,
                    mutation,
                } => {
                    ensure_selected_branch(&selected_branch_id, &branch_id)?;
                    if !can_author_fds_events(session.project().identity()) {
                        bail!(
                            "adding FDS drive events requires a NES project with declared {FDS_BIOS_FIRMWARE_ID} firmware"
                        );
                    }
                    events.push(fds::media_event(frame, mutation, project_media_id.clone()));
                    "Added FDS drive event"
                }
                TasEventMutation::UpdateMedia {
                    branch_id,
                    canonical_index,
                    expected_event,
                    frame,
                    mutation,
                } => {
                    ensure_selected_branch(&selected_branch_id, &branch_id)?;
                    let event = exact_fds_event_mut(
                        &mut events,
                        canonical_index,
                        &expected_event,
                        &project_media_id,
                    )?;
                    *event = fds::media_event(frame, mutation, project_media_id.clone());
                    "Updated FDS drive event"
                }
                TasEventMutation::Remove {
                    branch_id,
                    canonical_index,
                    expected_event,
                } => {
                    ensure_selected_branch(&selected_branch_id, &branch_id)?;
                    exact_fds_event_mut(
                        &mut events,
                        canonical_index,
                        &expected_event,
                        &project_media_id,
                    )?;
                    events.remove(canonical_index);
                    "Removed FDS event"
                }
            };
            events.sort_by(ReplayEvent::canonical_cmp);
            fds::validate_timeline(&events, frame_count, &project_media_id)?;

            let branch_id = selected_branch_id;
            let outcome = session
                .edit_transaction(move |edit| edit.replace_branch_events(&branch_id, events))?;
            (label, outcome)
        };

        if !outcome.changed {
            return Ok(format!("No event change ({label})"));
        }

        self.execution_preview.clear();
        self.event_editor.clear();
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "{label}; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok(label.to_owned())
    }
}

fn ensure_selected_branch(selected: &str, action_branch: &str) -> Result<()> {
    if selected != action_branch {
        bail!("selected TAS branch changed after this event edit was drafted; retry it");
    }
    Ok(())
}

fn exact_fds_event_mut<'a>(
    events: &'a mut [ReplayEvent],
    canonical_index: usize,
    expected_event: &ReplayEvent,
    project_media_id: &MediaObjectId,
) -> Result<&'a mut ReplayEvent> {
    let Some(event) = events.get_mut(canonical_index) else {
        bail!("the selected TAS event no longer exists; retry the edit");
    };
    if event != expected_event {
        bail!("the selected TAS event changed; retry the edit");
    }
    if !fds::is_editable_event(event, project_media_id) {
        bail!("only exact FDS drive events are editable in this panel");
    }
    Ok(event)
}

pub(super) fn draw_event_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasEventEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    state.sync_context(session);
    let project_sha256 = session.project_content_sha256();
    let branch_id = session.selected_branch_id();
    let frame_count = session.selected_branch().frame_count();
    let events = session.selected_branch().events();
    let project_media_id = fds::project_media_id(session.project().identity());
    let can_add = can_author_fds_events(session.project().identity()) && frame_count != 0;

    ui.collapsing(format!("Replay events ({})", events.len()), |ui| {
        ui.small(
            "Exact FDS drive events are editable. Other media and link events remain read-only.",
        );
        if can_add || state.draft.canonical_index.is_some() {
            fds::draw_form(
                ui,
                FdsDrawContext {
                    project_sha256,
                    branch_id,
                    frame_count,
                },
                can_add,
                state,
                actions,
            );
        } else {
            ui.small(format!(
                "Adding requires exact NES identity with {FDS_BIOS_FIRMWARE_ID} firmware."
            ));
        }

        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("tas_event_list")
            .max_height(EVENT_LIST_HEIGHT)
            .show_rows(ui, EVENT_ROW_HEIGHT, events.len(), |ui, rows| {
                for index in rows {
                    let event = &events[index];
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:>8}", event.frame()));
                        ui.label(event_summary(event));
                        if fds::is_editable_event(event, &project_media_id) {
                            if ui.small_button("Edit").clicked() {
                                selected = Some((index, event.clone()));
                            }
                            if ui.small_button("Remove").clicked() {
                                actions.push(TasEditorAction::Event(TasEventAction::new(
                                    project_sha256,
                                    TasEventMutation::Remove {
                                        branch_id: branch_id.to_owned(),
                                        canonical_index: index,
                                        expected_event: event.clone(),
                                    },
                                )));
                            }
                        } else {
                            ui.small("read-only");
                        }
                    });
                }
            });
        if events.is_empty() {
            ui.small("No recorded frame-boundary or synchronized events on this branch.");
        }
        if let Some((index, event)) = selected {
            state.draft =
                FdsDraft::from_event(index, event).expect("only supported FDS rows offer editing");
        }
    });
}

fn event_summary(event: &ReplayEvent) -> String {
    match event {
        ReplayEvent::FdsDiskSide { side, .. } => format!("FDS disk side {side}"),
        ReplayEvent::Media {
            sequence, event, ..
        } => {
            format!("Media event #{sequence}: {event:?}")
        }
        ReplayEvent::GameBoyLink { tick, .. } => format!("Game Boy link event at tick {tick}"),
        ReplayEvent::GameBoyLinkState { .. } => "Game Boy link state".to_owned(),
        ReplayEvent::GameBoyLinkStateAtTick { tick, .. } => {
            format!("Game Boy link state at tick {tick}")
        }
        ReplayEvent::WonderSwanLink { session_cycle, .. } => {
            format!("WonderSwan link event at session cycle {session_cycle}")
        }
    }
}
