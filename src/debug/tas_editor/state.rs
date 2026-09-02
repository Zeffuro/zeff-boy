use anyhow::{Result, bail};

use super::coleco_input::{ColecoControl, control_pressed, is_coleco_project, set_control};
use super::input_columns::{DigitalField, applicable_player_count, digital_columns};
use super::*;

impl TasEditorWindowState {
    pub(super) fn reset_active_branch_name(&mut self) {
        self.active_branch_name = self.session.as_ref().map_or_else(String::new, |session| {
            session.selected_branch().name().to_owned()
        });
    }

    pub(super) fn allows_linked_boundary_branch(&self, action: &TasEditorAction) -> bool {
        let TasEditorAction::ForkBranch { .. } = action else {
            return false;
        };
        let TasEditorLiveStatus::Linked { cursor, .. } = &self.live_status else {
            return false;
        };
        self.session
            .as_ref()
            .is_some_and(|session| session.cursor() == *cursor)
    }

    pub(super) fn queue_linked_edit_reconstruction(&mut self, start: u64, end: u64) {
        if end > start && self.live_status.is_linked() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::ReconstructAfterEdit { start, end },
            ));
        }
    }

    pub(super) fn edit_digital_input(
        &mut self,
        cursor: u64,
        player: usize,
        field: DigitalField,
        mask: u8,
        pressed: Option<bool>,
    ) -> Result<Option<String>> {
        if player >= 5 || mask.count_ones() != 1 {
            bail!("invalid TAS digital input column");
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if player >= applicable_player_count(session) {
            bail!("the TAS project does not declare that player");
        }
        if !digital_columns(&session.project().identity().system)
            .iter()
            .any(|column| column.field == field && column.mask == mask)
        {
            bail!("the TAS project does not declare that digital control");
        }
        let session = self.session_mut()?;
        let branch_id = session.selected_branch_id().to_owned();
        if cursor >= session.selected_branch().frame_count() {
            bail!("cannot edit input at the end cursor");
        }
        let mut input = session.selected_branch().input_at(cursor);
        let controller = &mut input.players[player];
        let target = match field {
            DigitalField::Buttons => &mut controller.buttons,
            DigitalField::Dpad => &mut controller.dpad,
        };
        let current = *target & mask != 0;
        let pressed = pressed.unwrap_or(!current);
        if current == pressed {
            return Ok(None);
        }
        if pressed {
            *target |= mask;
        } else {
            *target &= !mask;
        }
        let outcome = session
            .edit_transaction(move |edit| edit.set_input_range(&branch_id, cursor, 1, input))?;
        self.execution_preview.clear();
        if outcome.changed {
            self.queue_linked_edit_reconstruction(cursor, cursor + 1);
        }
        Ok(None)
    }

    pub(super) fn toggle_coleco_control(
        &mut self,
        cursor: u64,
        player: usize,
        control: ColecoControl,
    ) -> Result<Option<String>> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if !is_coleco_project(session) || player >= 2 {
            bail!("the TAS project does not declare that ColecoVision control");
        }
        if cursor >= session.selected_branch().frame_count() {
            bail!("cannot edit input at the end cursor");
        }
        let session = self.session_mut()?;
        let branch_id = session.selected_branch_id().to_owned();
        let mut input = session.selected_branch().input_at(cursor);
        let controller = &mut input.coleco[player];
        set_control(controller, control, !control_pressed(*controller, control));
        let outcome = session
            .edit_transaction(move |edit| edit.set_input_range(&branch_id, cursor, 1, input))?;
        self.execution_preview.clear();
        if outcome.changed {
            self.queue_linked_edit_reconstruction(cursor, cursor + 1);
        }
        Ok(None)
    }

    pub(super) fn set_coleco_keypad(
        &mut self,
        cursor: u64,
        player: usize,
        key: crate::tas_project::TasColecoKeypadKey,
    ) -> Result<Option<String>> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if !is_coleco_project(session) || player >= 2 {
            bail!("the TAS project does not declare that ColecoVision keypad");
        }
        if cursor >= session.selected_branch().frame_count() {
            bail!("cannot edit input at the end cursor");
        }
        let session = self.session_mut()?;
        let branch_id = session.selected_branch_id().to_owned();
        let mut input = session.selected_branch().input_at(cursor);
        if input.coleco[player].keypad == key {
            return Ok(None);
        }
        input.coleco[player].keypad = key;
        let outcome = session
            .edit_transaction(move |edit| edit.set_input_range(&branch_id, cursor, 1, input))?;
        self.execution_preview.clear();
        if outcome.changed {
            self.queue_linked_edit_reconstruction(cursor, cursor + 1);
        }
        Ok(None)
    }

    pub(super) fn apply_timeline_selection_change(
        &mut self,
        change: TasTimelineSelectionChange,
    ) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;

        if let TasTimelineSelectionChange::Boundary(cursor) = change {
            let target = cursor.min(session.selected_branch().frame_count());
            if self
                .recording
                .as_ref()
                .is_some_and(|recording| recording.cursor != target)
            {
                self.discard_recording_draft()?;
            }
            let frame_count = self
                .session
                .as_ref()
                .expect("open session was checked before selecting a TAS boundary")
                .selected_branch()
                .frame_count();
            self.session_mut()?.set_cursor(cursor.min(frame_count))?;
            let session = self
                .session
                .as_ref()
                .expect("open session was checked before selecting a TAS boundary");
            self.timeline_selection.collapse_to_cursor(session);
            self.timeline_follow_selection = true;
            self.execution_preview.clear();
            return Ok(());
        }

        let mut selection = self.timeline_selection.clone();
        let target = match change {
            TasTimelineSelectionChange::Frame {
                frame,
                extend_selection,
            } => {
                if frame >= session.selected_branch().frame_count() {
                    bail!("cannot select the end cursor as an input frame");
                }
                selection.select_frame(session, frame, extend_selection);
                frame
            }
            TasTimelineSelectionChange::Range { anchor, active } => selection
                .select_frame_range(session, anchor, active)
                .ok_or_else(|| anyhow::anyhow!("cannot select input rows in an empty movie"))?,
            TasTimelineSelectionChange::Navigate {
                navigation,
                extend_selection,
            } => selection.navigate(session, navigation, extend_selection),
            TasTimelineSelectionChange::Boundary(_) => unreachable!(),
        };
        if self
            .recording
            .as_ref()
            .is_some_and(|recording| recording.cursor != target)
        {
            self.discard_recording_draft()?;
        }
        let session = self
            .session
            .as_ref()
            .expect("open session was checked before selecting timeline input");
        let target = selection.active_cursor(session);
        self.session_mut()?.set_cursor(target)?;
        self.timeline_selection = selection;
        self.timeline_follow_selection = true;
        self.execution_preview.clear();
        Ok(())
    }
}
