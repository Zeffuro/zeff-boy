use anyhow::{Result, bail};

use super::{
    TasEditorAction, TasEditorHostRequest, TasEditorLiveAction, TasEditorLiveStatus,
    TasEditorWindowState,
};
use crate::live_control::TasDigitalInput;
use crate::tas_project::{TasEditorSession, TasLiveRecordingMode, TasPreparedLiveFrame};

impl TasEditorWindowState {
    pub(crate) fn set_live_status(&mut self, status: TasEditorLiveStatus) {
        if matches!(&status, TasEditorLiveStatus::Acquiring) {
            self.execution_preview.clear();
            self.linked_session_active = false;
            self.close_keep_after_live_command = false;
        }
        if matches!(&status, TasEditorLiveStatus::Terminal(_)) || !status.holds_authority() {
            self.linked_session_active = false;
            self.close_keep_after_live_command = false;
        }
        if matches!(&status, TasEditorLiveStatus::Linked { .. }) {
            self.linked_session_active = true;
            if std::mem::take(&mut self.close_keep_after_live_command) {
                self.pending_host_request = Some(TasEditorHostRequest::Live(
                    TasEditorLiveAction::KeepResultAndReturnToGame,
                ));
            }
        }
        self.live_status = status;
    }

    pub(crate) fn take_pending_host_request(&mut self) -> Option<TasEditorHostRequest> {
        self.pending_host_request.take()
    }

    pub(crate) fn active_session(&self) -> Option<&TasEditorSession> {
        self.session.as_ref()
    }

    pub(crate) fn live_status(&self) -> &TasEditorLiveStatus {
        &self.live_status
    }

    pub(crate) fn live_recording_mode(&self) -> TasLiveRecordingMode {
        self.live_recording_mode
    }

    pub(crate) fn set_live_recording_mode(&mut self, mode: TasLiveRecordingMode) {
        self.live_recording_mode = mode;
    }

    pub(crate) fn select_cursor_for_live_control(&mut self, cursor: u64) -> Result<()> {
        self.reduce(TasEditorAction::SelectCursor(cursor))?;
        Ok(())
    }

    pub(crate) fn select_input_range_for_live_control(
        &mut self,
        start: u64,
        end: u64,
    ) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if start >= end || end > session.selected_branch().frame_count() {
            bail!("selected TAS input range is outside the active branch");
        }
        self.reduce(TasEditorAction::SelectTimelineRange {
            anchor: start,
            active: end - 1,
        })?;
        Ok(())
    }

    pub(crate) fn selected_input_range_for_live_control(&mut self) -> Result<Option<(u64, u64)>> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        Ok(self.timeline_selection.selected_range(session))
    }

    pub(crate) fn delete_selected_input_range_for_live_control(&mut self) -> Result<()> {
        let Some((start, end)) = self.selected_input_range_for_live_control()? else {
            bail!("select one or more TAS input frames first");
        };
        self.reduce(TasEditorAction::DeleteFrames {
            start,
            count: end - start,
        })?;
        Ok(())
    }

    pub(crate) fn insert_neutral_frames_for_live_control(
        &mut self,
        boundary: u64,
        count: u64,
    ) -> Result<()> {
        self.reduce(TasEditorAction::InsertNeutralFrames {
            cursor: boundary,
            count,
        })?;
        Ok(())
    }

    pub(crate) fn set_digital_input_for_live_control(
        &mut self,
        frame: u64,
        player: u8,
        input: TasDigitalInput,
        pressed: bool,
    ) -> Result<()> {
        if !(1..=5).contains(&player) {
            bail!("player must be 1 through 5");
        }
        let (field, mask) = match input {
            TasDigitalInput::Buttons(mask) => (super::DigitalField::Buttons, mask),
            TasDigitalInput::Dpad(mask) => (super::DigitalField::Dpad, mask),
        };
        self.reduce(TasEditorAction::SetDigital {
            cursor: frame,
            player: usize::from(player - 1),
            field,
            mask,
            pressed,
        })?;
        Ok(())
    }

    pub(crate) fn select_end_cursor_for_live_control(&mut self) -> Result<()> {
        let frame_count = self.session_mut()?.selected_branch().frame_count();
        self.select_cursor_for_live_control(frame_count)
    }

    pub(crate) fn fork_branch_at_linked_boundary_for_live_control(
        &mut self,
        boundary: u64,
        id: String,
        name: String,
    ) -> Result<()> {
        let selected_boundary = self.session_mut()?.cursor();
        if selected_boundary != boundary {
            bail!("move the linked game to the selected TAS boundary before creating a branch");
        }
        self.reduce(TasEditorAction::ForkBranch { id, name })?;
        Ok(())
    }

    pub(crate) fn commit_prepared_live_frame(
        &mut self,
        prepared: TasPreparedLiveFrame,
    ) -> Result<()> {
        self.session_mut()?.commit_prepared_live_frame(prepared)?;
        self.execution_preview.clear();
        Ok(())
    }

    pub(crate) fn begin_live_recording_history_group(&mut self) -> Result<()> {
        self.session_mut()?.begin_live_recording_history_group()
    }

    pub(crate) fn end_live_recording_history_group(&mut self) -> Result<bool> {
        let session = self.session_mut()?;
        if !session.live_recording_history_group_active() {
            return Ok(false);
        }
        session.end_live_recording_history_group()
    }

    pub(super) fn queue_return_to_game_unchanged(&mut self) {
        if self.live_status.is_linked() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::KeepResultAndReturnToGame,
            ));
        } else if self.linked_session_active && self.live_status.requires_return_on_close() {
            self.close_keep_after_live_command = true;
            match &self.live_status {
                TasEditorLiveStatus::Recording => {
                    self.pending_host_request = Some(TasEditorHostRequest::Live(
                        TasEditorLiveAction::StopRealtimeRecording,
                    ));
                }
                TasEditorLiveStatus::Playing { .. } => {
                    self.pending_host_request = Some(TasEditorHostRequest::Live(
                        TasEditorLiveAction::PausePlayback,
                    ));
                }
                _ => {}
            }
        } else if self.live_status.requires_return_on_close() {
            self.pending_host_request = Some(TasEditorHostRequest::Live(
                TasEditorLiveAction::ReturnToGameUnchanged,
            ));
        }
    }
}
