use super::*;
use crate::app::{App, DebugRequests};

impl App {
    pub(in crate::app) fn refresh_tas_control_readiness(&mut self) {
        if self.pending_tas_repair_activation.is_some()
            || !self.tas_control.gameplay_commands_allowed()
        {
            return;
        }
        let Some(session) = self.debug_windows.tas_editor.active_session() else {
            self.tas_control.clear_readiness();
            return;
        };
        let Ok(profile) =
            crate::emu_backend::loader::classify_direct_tas_execution_profile(session.project())
        else {
            self.tas_control.clear_readiness();
            return;
        };
        let key = TasReadinessKey {
            worker_generation: self.emu_worker_generation,
            profile,
            project_content_sha256: session.project_content_sha256(),
            configured_sample_rate: self.settings.audio.output_sample_rate,
        };
        let command = match self.tas_control.begin_readiness_observation(key) {
            Ok(Some(command)) => command,
            Ok(None) => return,
            Err(error) => {
                log::error!("Could not inspect TAS readiness: {error:#}");
                self.tas_control.clear_readiness();
                return;
            }
        };
        if self.send_emu_command_checked(command).is_err() {
            self.tas_control.clear_readiness();
        }
    }

    pub(in crate::app) fn tas_control_readiness_report(
        &self,
    ) -> Option<&readiness::TasReadinessReport> {
        self.tas_control.readiness_report()
    }

    pub(in crate::app) fn tas_control_live_status(&self) -> crate::debug::TasEditorLiveStatus {
        self.tas_control.live_status()
    }

    pub(in crate::app) fn worker_gameplay_commands_allowed(&self) -> bool {
        self.pending_tas_repair_activation.is_none()
            && !self.tas_repair.has_active_transaction()
            && self.tas_control.gameplay_commands_allowed()
    }

    pub(in crate::app) fn fence_tas_control_gameplay(&mut self) {
        if self.worker_gameplay_commands_allowed() {
            return;
        }
        self.recompute_pause();
        self.speed.fast_forward_held = false;
        self.speed.turbo_held = false;
        self.rewind.held = false;
        self.debug_requests = DebugRequests::default();
        self.pending_debug_actions = crate::debug::DebugUiActions::none();
    }

    #[allow(dead_code)]
    pub(in crate::app) fn begin_tas_control_acquire(&mut self) -> Result<()> {
        self.begin_tas_control_acquire_with_mode(TasControlStartMode::Preview)
    }

    pub(in crate::app) fn begin_tas_control_recording_acquire(&mut self) -> Result<()> {
        self.begin_tas_control_acquire_with_mode(TasControlStartMode::Record)
    }

    pub(in crate::app) fn seek_linked_tas_to_editor_cursor(&mut self) -> Result<()> {
        let (profile, sync_identity_sha256) = self
            .tas_control
            .linked_identity()
            .ok_or_else(|| anyhow::anyhow!("the loaded game is not linked to the TAS editor"))?;
        let session = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?;
        let binding = TasEditorControlSnapshot::prepare_linked_seek(
            session,
            profile,
            sync_identity_sha256,
            self.tas_control.linked_cache_candidate_cursors(),
        )?;
        let command = self.tas_control.begin_linked_seek(binding)?;
        self.send_tas_control_command(command);
        Ok(())
    }

    pub(in crate::app) fn reconstruct_linked_tas_after_edit(
        &mut self,
        edited_start: u64,
        edited_end: u64,
    ) -> Result<()> {
        let (profile, sync_identity_sha256) = self
            .tas_control
            .linked_identity()
            .ok_or_else(|| anyhow::anyhow!("the loaded game is not linked to the TAS editor"))?;
        let session = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?;
        let binding = TasEditorControlSnapshot::prepare_linked_seek_at(
            session,
            edited_end,
            profile,
            sync_identity_sha256,
            self.tas_control.linked_cache_candidate_cursors(),
        )?;
        let command =
            self.tas_control
                .begin_linked_edit_follow(binding, edited_start, edited_end)?;
        self.send_tas_control_command(command);
        Ok(())
    }

    fn begin_tas_control_acquire_with_mode(&mut self, mode: TasControlStartMode) -> Result<()> {
        if self.recording.replay_timeline_active()
            || self.rewind.pending
            || self.rewind.backstep_pending
            || self.recording.audio_recorder.is_some()
            || self.timing.uncapped_speed
        {
            bail!("current emulator activity is incompatible with TAS control acquisition");
        }
        if self.emu_thread.is_none() {
            bail!("no emulator is running");
        }
        match self.detached_tas_editor_live_status() {
            crate::debug::TasEditorLiveStatus::Ready { .. } => {}
            crate::debug::TasEditorLiveStatus::ReloadRequired(reason)
            | crate::debug::TasEditorLiveStatus::Unavailable(reason) => bail!(reason),
            _ => bail!("finish the current TAS action before connecting again"),
        }
        let project = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?;
        let project = TasEditorControlSnapshot::capture(project)?;
        self.tas_control
            .queue_acquire(self.emu_worker_generation, project, mode)?;
        self.recompute_pause();
        Ok(())
    }

    pub(in crate::app) fn begin_queued_tas_control_acquire(&mut self) {
        if !acquisition_delivery_quiesced(self.frames_in_flight) {
            return;
        }
        let command = match self.tas_control.begin_queued_acquire() {
            Ok(Some(command)) => command,
            Ok(None) => return,
            Err(error) => {
                self.tas_control.terminalize_worker(
                    self.emu_worker_generation,
                    TasControlTerminalReason::CommandChannelClosed,
                );
                self.toast_manager
                    .error(format!("Could not acquire TAS control: {error:#}"));
                self.recompute_pause();
                return;
            }
        };
        let sent = self
            .emu_thread
            .as_ref()
            .is_some_and(|thread| thread.send_checked(command));
        if !sent {
            self.tas_control.terminalize_worker(
                self.emu_worker_generation,
                TasControlTerminalReason::CommandChannelClosed,
            );
            self.recompute_pause();
            self.toast_manager
                .error("The emulator command channel is closed");
        }
    }

    pub(in crate::app) fn cancel_tas_control(&mut self) {
        self.pending_tas_repair_activation = None;
        self.request_tas_repair_resolution(repair::TasRepairResolution::Restore);
        self.stop_realtime_tas_recording();
        if let Some(command) = self.tas_control.cancel() {
            self.send_tas_control_command(command);
        }
        self.finish_realtime_tas_history_group_if_idle();
        self.pump_tas_repair_resolution();
    }

    #[allow(dead_code)]
    pub(in crate::app) fn commit_tas_control(&mut self) -> Result<()> {
        self.stop_realtime_tas_recording();
        let current = self
            .debug_windows
            .tas_editor
            .active_session()
            .and_then(|session| TasEditorControlSnapshot::capture(session).ok());
        let command = self
            .tas_control
            .commit(current.as_ref())
            .ok_or_else(|| anyhow::anyhow!("no completed TAS execution is awaiting a decision"))?;
        self.request_tas_repair_resolution(repair::TasRepairResolution::Keep);
        self.send_tas_control_command(command);
        Ok(())
    }

    pub(in crate::app) fn record_live_tas_frame(
        &mut self,
        input: crate::tas_project::TasInputFrame,
    ) -> Result<()> {
        if !self.tas_control.can_record_live_input() {
            bail!("no completed TAS execution is awaiting a live input frame");
        }
        let recording_mode = self.debug_windows.tas_editor.live_recording_mode();
        let prepared = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?
            .prepare_live_frame_with_mode(input, recording_mode)?;
        let command = self.tas_control.begin_live_frame_advance(prepared)?;
        self.send_tas_control_command(command);
        Ok(())
    }

    pub(in crate::app) fn reconcile_tas_control_project_binding(&mut self) {
        if !self.tas_control.requires_project_reconciliation() {
            return;
        }
        let Some(session) = self.debug_windows.tas_editor.active_session() else {
            if let Some(command) = self.tas_control.reconcile_project(None) {
                self.send_tas_control_command(command);
            }
            return;
        };
        if self.tas_control.matches_project_revision(session) {
            return;
        }
        let current = self
            .tas_control
            .project_binding_cursor()
            .and_then(|cursor| TasEditorControlSnapshot::capture_at(session, cursor).ok());
        if let Some(command) = self.tas_control.reconcile_project(current.as_ref()) {
            self.send_tas_control_command(command);
        }
    }

    pub(in crate::app) fn consume_tas_control_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        let response = match response {
            EmuResponse::TasReadinessObserved {
                request_id,
                observation,
            } => {
                let accepted = if let Some(session) = self.debug_windows.tas_editor.active_session()
                {
                    self.tas_control.accept_readiness_observation(
                        self.emu_worker_generation,
                        request_id,
                        session.project().identity(),
                        &observation,
                    )
                } else {
                    false
                };
                if accepted {
                    self.pump_tas_repair_connect_after_readiness();
                }
                return None;
            }
            response => response,
        };
        let replay_response = self.tas_control.execution_replay_pending()
            && matches!(
                &response,
                EmuResponse::TasFrameAdvanced { .. } | EmuResponse::TasFrameAdvanceRejected { .. }
            );
        let repair_failure = matches!(
            &response,
            EmuResponse::TasControlAcquireRejected { .. }
                | EmuResponse::TasExecutionRejected { .. }
                | EmuResponse::TasFrameAdvanceRejected { .. }
                | EmuResponse::TasControlRollbackRejected { .. }
                | EmuResponse::TasControlCommitRejected { .. }
        );
        let replay_session = replay_response
            .then(|| self.debug_windows.tas_editor.active_session())
            .flatten();
        let current_project = (!replay_response)
            .then(|| {
                self.debug_windows
                    .tas_editor
                    .active_session()
                    .and_then(|session| {
                        let cursor = self
                            .tas_control
                            .project_binding_cursor()
                            .unwrap_or(session.cursor());
                        TasEditorControlSnapshot::capture_at(session, cursor).ok()
                    })
            })
            .flatten();
        let acquired_project = match &response {
            EmuResponse::TasControlAcquired { witness, .. } => self
                .debug_windows
                .tas_editor
                .active_session()
                .and_then(|session| {
                    TasEditorControlSnapshot::validate_acquired(session, witness).ok()
                }),
            _ => None,
        };
        let disposition = self.tas_control.consume_response_with_session(
            self.emu_worker_generation,
            response,
            acquired_project,
            current_project.as_ref(),
            replay_session,
        );
        let mut live_audio = None;
        let mut live_rumble = None;
        let mut live_ui_data = None;
        let disposition = match disposition {
            ResponseDisposition::CommitLiveFrame {
                prepared,
                rumble,
                audio_samples,
                ui_data,
            } => {
                let committed = self
                    .debug_windows
                    .tas_editor
                    .commit_prepared_live_frame(*prepared)
                    .and_then(|()| {
                        self.debug_windows
                            .tas_editor
                            .active_session()
                            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))
                            .and_then(TasEditorControlSnapshot::capture)
                    });
                if committed.is_ok() {
                    live_audio = Some(audio_samples);
                    live_rumble = Some(rumble);
                    live_ui_data = ui_data;
                }
                self.tas_control.finish_live_frame_commit(committed)
            }
            ResponseDisposition::PresentPlaybackFrame {
                rumble,
                audio_samples,
                ui_data,
            } => {
                live_audio = Some(audio_samples);
                live_rumble = Some(rumble);
                live_ui_data = ui_data;
                ResponseDisposition::Consumed { follow_up: None }
            }
            ResponseDisposition::ContinueExecutionReplay => self
                .tas_control
                .continue_execution_replay(self.debug_windows.tas_editor.active_session()),
            disposition => disposition,
        };
        if self.tas_control.take_realtime_recording_start_request()
            && let Err(error) = self.start_realtime_tas_recording()
        {
            self.toast_manager
                .error(format!("Could not start TAS recording: {error:#}"));
            if let Some(command) = self.tas_control.cancel() {
                self.send_tas_control_command(command);
            }
        }
        self.finish_realtime_tas_history_group_if_idle();
        if let Some(audio_samples) = live_audio {
            self.queue_emulator_audio(&audio_samples, 1);
        }
        if let (Some(rumble), Some(gamepad)) = (live_rumble, &mut self.gamepad) {
            gamepad.set_rumble(rumble);
        }
        if let Some(ui_data) = live_ui_data {
            self.process_ui_frame_data(*ui_data);
        }
        let refresh_framebuffer = self.tas_control.take_framebuffer_refresh();
        if let Some(error) = self.tas_control.take_error() {
            self.toast_manager.error(error);
        }
        let response = match disposition {
            ResponseDisposition::Unrelated(response) => Some(response),
            ResponseDisposition::Consumed { follow_up } => {
                if refresh_framebuffer && let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                    if let (Some(cursor), Some(frame)) =
                        (self.tas_control.linked_cursor(), self.latest_frame.as_ref())
                    {
                        let (width, height) = self.active_system.screen_size();
                        if let Err(error) = self.debug_windows.tas_editor.install_linked_frame(
                            cursor,
                            width,
                            height,
                            frame.as_ref().clone(),
                        ) {
                            self.toast_manager
                                .error(format!("Could not show the linked TAS frame: {error:#}"));
                        }
                    }
                }
                if let Some(command) = follow_up {
                    self.send_tas_control_command(command);
                }
                self.recompute_pause();
                None
            }
            ResponseDisposition::CommitLiveFrame { .. } => {
                unreachable!("live frame commit must complete before response disposition")
            }
            ResponseDisposition::PresentPlaybackFrame { .. } => {
                unreachable!("playback frame must be presented before response disposition")
            }
            ResponseDisposition::ContinueExecutionReplay => {
                unreachable!("staged execution replay must enqueue its next frame before dispatch")
            }
        };
        if repair_failure
            && self.request_tas_repair_resolution(repair::TasRepairResolution::Restore)
            && self.tas_control.terminal()
        {
            self.tas_control.retire_worker(self.emu_worker_generation);
        }
        self.pump_tas_repair_resolution();
        response
    }

    pub(in crate::app) fn retire_tas_control_worker(&mut self) {
        self.tas_control.retire_worker(self.emu_worker_generation);
        self.tas_realtime_recorder.reset();
        self.tas_playback_scheduler.reset();
        self.finish_realtime_tas_history_group_if_idle();
        self.recompute_pause();
        self.pump_tas_repair_resolution();
    }

    pub(in crate::app) fn terminalize_tas_control_runtime_fault(&mut self) {
        self.terminalize_tas_control_worker(TasControlTerminalReason::RuntimeFault);
    }

    pub(in crate::app) fn terminalize_tas_control_response_loss(&mut self) {
        self.terminalize_tas_control_worker(TasControlTerminalReason::ResponseChannelClosed);
    }

    pub(in crate::app) fn terminalize_tas_control_command_loss(&mut self) {
        self.terminalize_tas_control_worker(TasControlTerminalReason::CommandChannelClosed);
    }

    fn terminalize_tas_control_worker(&mut self, reason: TasControlTerminalReason) {
        let repairing = self.request_tas_repair_resolution(repair::TasRepairResolution::Restore);
        self.tas_control
            .terminalize_worker(self.emu_worker_generation, reason);
        if repairing {
            self.tas_control.retire_worker(self.emu_worker_generation);
        }
        self.tas_realtime_recorder.reset();
        self.tas_playback_scheduler.reset();
        self.finish_realtime_tas_history_group_if_idle();
        self.recompute_pause();
        self.pump_tas_repair_resolution();
    }

    pub(in crate::app) fn send_tas_control_command(&mut self, command: WorkerBoundCommand) {
        let Some((_, mut command)) = command.into_parts_for_worker(self.emu_worker_generation)
        else {
            return;
        };
        if self.tas_control.captures_frame_snapshot()
            && let crate::emu_thread::EmuCommand::AdvanceTasControl(request) = &mut command
        {
            let requirements = if self.debug_workspace_visible() {
                crate::debug::compute_tab_requirements(&self.debug_dock)
            } else {
                crate::debug::dock::TabDataRequirements::default()
            };
            request.snapshot = Some(crate::emu_thread::TasFrameAdvanceSnapshot {
                request: self.build_snapshot_request(&requirements, true),
                buffers: self.take_reusable_buffers(),
            });
        }
        let sent = self
            .emu_thread
            .as_ref()
            .is_some_and(|thread| thread.send_checked(command));
        if !sent {
            self.terminalize_tas_control_command_loss();
        }
    }
}

pub(super) fn acquisition_delivery_quiesced(frames_in_flight: usize) -> bool {
    frames_in_flight == 0
}
