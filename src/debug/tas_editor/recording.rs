use anyhow::{Result, bail};

use super::{
    TasEditorAction, TasEditorExecutionAvailability, TasEditorRecordingState, TasEditorWindowState,
};

impl TasEditorWindowState {
    pub(super) fn start_frame_recording(&mut self) -> Result<Option<String>> {
        if self.recording.is_some() {
            bail!("frame recording is already active");
        }
        let session = self.session_mut()?;
        let branch_id = session.selected_branch_id().to_owned();
        let cursor = session.selected_branch().frame_count();
        let checkpoint = session.begin_recording_draft(&branch_id, cursor)?;
        self.recording = Some(TasEditorRecordingState {
            branch_id,
            cursor,
            checkpoint,
        });
        self.execution_preview.clear();
        Ok(Some(format!("Editing new input frame {cursor}")))
    }

    pub(super) fn capture_recording_frame(&mut self) -> Result<Option<String>> {
        let recording = self
            .recording
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("frame recording is not active"))?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if session.selected_branch_id() != recording.branch_id
            || session.cursor() != recording.cursor
            || recording.cursor >= session.selected_branch().frame_count()
        {
            bail!("the recording row changed; finish manual row entry and start again");
        }
        let branch_id = recording.branch_id.clone();
        let accepted_cursor = recording.cursor;
        let next_cursor = accepted_cursor
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS recording cursor overflow"))?;
        if matches!(
            self.execution_availability,
            TasEditorExecutionAvailability::Ready
        ) {
            self.execute_private_seek(next_cursor)?;
        }
        let session = self.session_mut()?;
        let checkpoint = match session.begin_recording_draft(&branch_id, next_cursor) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                session.set_cursor(accepted_cursor)?;
                return Err(error);
            }
        };
        self.recording = Some(TasEditorRecordingState {
            branch_id,
            cursor: next_cursor,
            checkpoint,
        });
        Ok(Some(format!(
            "Kept input frame {}; ready for frame {next_cursor}",
            accepted_cursor
        )))
    }

    pub(super) fn stop_frame_recording(&mut self) -> Result<Option<String>> {
        if self.recording.is_none() {
            return Ok(None);
        }
        self.discard_recording_draft()?;
        Ok(Some("Finished manual row entry".to_owned()))
    }

    pub(super) fn discard_recording_draft(&mut self) -> Result<bool> {
        let Some(recording) = self.recording.take() else {
            return Ok(false);
        };
        match self
            .session_mut()
            .and_then(|session| session.discard_recording_draft(&recording.checkpoint))
        {
            Ok(()) => {
                self.execution_preview.clear();
                Ok(true)
            }
            Err(error) => {
                self.recording = Some(recording);
                Err(error)
            }
        }
    }
}

pub(super) fn draw_recording_strip(
    ui: &mut egui::Ui,
    recording: Option<&TasEditorRecordingState>,
    enabled: bool,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_wrapped(|ui| match recording {
            Some(recording) => {
                ui.strong(format!("Editing new row {}", recording.cursor));
                let advance_shortcut = ui
                    .input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter));
                if ui
                    .button("Keep row + add next  Ctrl+Enter")
                    .on_hover_text("Keep this row and create the next neutral input row")
                    .clicked()
                    || advance_shortcut
                {
                    actions.push(TasEditorAction::CaptureRecordingFrame);
                }
                if ui
                    .button("Finish")
                    .on_hover_text("Finish and discard the current unaccepted row")
                    .clicked()
                {
                    actions.push(TasEditorAction::StopRecording);
                }
                ui.small("Toggle input cells, then keep the row. Neutral rows need no clicks.");
            }
            None => {
                if ui.button("Append rows manually").clicked() {
                    actions.push(TasEditorAction::StartRecordingAtEnd);
                }
                ui.small(
                    "For cell-by-cell entry at the movie end. Live TAS recording uses Record.",
                );
            }
        });
    });
}
