#[cfg(test)]
use super::super::audio_timing;
use super::super::{MAX_UNCAPPED_BATCH_SIZE, commands};
use super::EmuLoop;
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};

impl EmuLoop {
    pub(super) fn handle_command(&mut self, command: EmuCommand) -> bool {
        #[cfg(test)]
        audio_timing::worker_dequeue(&command);
        let command = match self.dispatch_netplay(command) {
            std::ops::ControlFlow::Continue(command) => command,
            std::ops::ControlFlow::Break(alive) => return alive,
        };
        let command = match self.dispatch_tas_authority(command) {
            std::ops::ControlFlow::Continue(command) => command,
            std::ops::ControlFlow::Break(keep_running) => return keep_running,
        };
        self.speculation.invalidate();
        let command = match (commands::CommonCommandContext {
            backend: &mut self.backend,
            shared_framebuffer: &self.shared_framebuffer,
            uncapped_mode: &mut self.uncapped_mode,
            rewind_buffer: &mut self.rewind_buffer,
            last_cheats: &mut self.last_cheats,
            audio_recording_capture: &mut self.audio_recording_capture,
            pending_audio_discontinuities: &mut self.pending_audio_discontinuities,
            runtime_fault: &mut self.runtime_fault,
        })
        .dispatch(command)
        {
            commands::CommonCommandDispatch::Handled(effects) => {
                if effects.potentially_dirty {
                    self.battery_flush.mark_potentially_dirty();
                }
                return effects
                    .response
                    .is_none_or(|response| self.send_resp(response));
            }
            commands::CommonCommandDispatch::PlatformSpecific(command) => command,
        };
        match command {
            EmuCommand::SetUncappedBatchSize(frames) => {
                self.uncapped_batch_size = frames.clamp(1, MAX_UNCAPPED_BATCH_SIZE);
            }

            EmuCommand::StartTcpLink(mode) => {
                self.game_boy_replay_link = None;
                self.wonder_swan_replay_link = None;
                self.start_tcp_link(mode);
            }

            EmuCommand::DisconnectLink => {
                self.disconnect_tcp_link();
                self.game_boy_replay_link = None;
                self.wonder_swan_replay_link = None;
                if !self.send_resp(EmuResponse::LinkDisconnected {
                    frame_count: self.backend.frame_count(),
                    game_boy_cpu_cycles: self.backend.game_boy_cpu_cycles(),
                    game_boy_link_state: self.backend.game_boy_link_replay_state(),
                }) {
                    return false;
                }
            }

            EmuCommand::StepFrames(input) => {
                if input.debug_actions.md_timed_ports.is_some() {
                    return self.deliver_md_timed_ports(*input);
                }
                #[cfg(test)]
                let trace_id = input.audio_timing_request;
                #[cfg(test)]
                audio_timing::record(trace_id, "worker_start", 0, input.frames as u64, 0, 0);
                let input = *input;
                self.audio_recording_capture = input.audio.recording_capture;
                let debugger_mutation = match self
                    .backend
                    .debugger_memory_mutation(&input.debug_actions, self.runtime_fault.can_step())
                {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        if !self.send_resp(EmuResponse::MemoryWriteRejected(error)) {
                            return false;
                        }
                        false
                    }
                };
                let instruction_step_requested = input.debug_step && !input.debug_continue;
                let md_time_before = self.backend.md_execution_time();
                let local_context = self.game_boy_replay_link.is_none()
                    && self.wonder_swan_replay_link.is_none()
                    && self.tcp_link.is_none();
                let detached_request = self.speculation.request_detached_frame(
                    &self.backend,
                    &input,
                    &self.last_cheats,
                    self.uncapped_mode,
                    local_context,
                );
                let result = if let Some(replay_link) = self.game_boy_replay_link.as_mut() {
                    EmuThread::handle_step_frames_with_game_boy_replay_link(
                        &mut self.backend,
                        input,
                        &self.last_cheats,
                        replay_link,
                        self.uncapped_mode,
                        &mut self.rewind_buffer,
                        &mut self.rewind_seconds,
                        &mut self.runtime_fault,
                    )
                } else if let Some(replay_link) = self.wonder_swan_replay_link.as_mut() {
                    EmuThread::handle_step_frames_with_wonder_swan_replay_link(
                        &mut self.backend,
                        input,
                        &self.last_cheats,
                        replay_link,
                        self.uncapped_mode,
                        &mut self.rewind_buffer,
                        &mut self.rewind_seconds,
                        &mut self.runtime_fault,
                    )
                } else {
                    let result = EmuThread::handle_step_frames_with_tcp_link(
                        &mut self.backend,
                        input,
                        &self.last_cheats,
                        self.tcp_link.as_mut(),
                        self.uncapped_mode,
                        &mut self.rewind_buffer,
                        &mut self.rewind_seconds,
                        &mut self.runtime_fault,
                    );
                    self.clear_disconnected_tcp_link();
                    result
                };
                #[cfg(test)]
                audio_timing::result(trace_id, "result_built", &result);
                let detached_frame = self.speculation.run_detached_frame(
                    &self.backend,
                    detached_request,
                    &result,
                    self.runtime_fault.can_step(),
                );
                self.speculation.commit_primary_frame(
                    &self.shared_framebuffer,
                    &self.backend,
                    detached_frame,
                );
                let mut result = result;
                if commands::finalize_step_result(
                    &mut result,
                    debugger_mutation,
                    instruction_step_requested
                        || md_time_before != self.backend.md_execution_time(),
                    self.audio_recording_capture,
                    &mut self.pending_audio_discontinuities,
                    &self.runtime_fault,
                    &mut self.uncapped_mode,
                ) {
                    self.battery_flush.mark_potentially_dirty();
                }
                #[cfg(test)]
                audio_timing::ready(trace_id, &mut result);
                let preserve_delivery = EmuThread::frame_requires_preserved_delivery(
                    &result,
                    self.audio_recording_capture.active,
                );
                if !EmuThread::send_frame(&self.frame_tx, &self.drain_rx, result, preserve_delivery)
                {
                    return false;
                }
            }

            EmuCommand::SaveStateSlot(slot) => match self.backend.slot_path(slot) {
                Ok(path) => {
                    if !EmuThread::save_state_async(
                        &self.backend,
                        path,
                        &self.resp_tx,
                        &self.send_resp_fn(),
                    ) {
                        return false;
                    }
                }
                Err(e) => {
                    if !self.send_resp(EmuResponse::SaveStateFailed(e.to_string())) {
                        return false;
                    }
                }
            },

            EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = self.backend.load_state(slot);
                let label = result
                    .as_ref()
                    .ok()
                    .map(|(path, _)| path.clone())
                    .unwrap_or_default();
                if !self.finalize_load_state(
                    result.map(|(_, outcome)| outcome),
                    label,
                    Some((buttons_pressed, dpad_pressed)),
                ) {
                    return false;
                }
            }

            EmuCommand::SaveStateToPath(path) => {
                if !EmuThread::save_state_async(
                    &self.backend,
                    path,
                    &self.resp_tx,
                    &self.send_resp_fn(),
                ) {
                    return false;
                }
            }

            EmuCommand::LoadStateFromPath {
                path,
                buttons_pressed,
                dpad_pressed,
            } => {
                let label = path.display().to_string();
                let result = self.backend.load_state_from_path(&path);
                if !self.finalize_load_state(result, label, Some((buttons_pressed, dpad_pressed))) {
                    return false;
                }
            }

            EmuCommand::CaptureReplayStart { capture_id } => {
                let blocker = self
                    .backend
                    .supports_replay()
                    .then(|| self.replay_start_capture_blocker())
                    .flatten();
                let metadata = if self.backend.supports_replay() && blocker.is_none() {
                    Some(self.capture_replay_metadata())
                } else {
                    None
                };
                let resp = commands::capture_replay_start_response(
                    &self.backend,
                    capture_id,
                    blocker,
                    || metadata.expect("supported, unblocked replay capture has metadata"),
                );
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
                replay_events,
                game_boy_link_start_state,
                game_boy_link_coordinator_start_state,
                game_boy_link_start_tick,
                wonder_swan_link_start_tick,
            } => {
                let has_game_boy_link = game_boy_link_start_state.is_some()
                    || game_boy_link_coordinator_start_state.is_some()
                    || replay_events.as_ref().is_some_and(|events| {
                        events.iter().any(|event| {
                            matches!(
                                event,
                                zeff_emu_common::replay::ReplayEvent::GameBoyLink { .. }
                                    | zeff_emu_common::replay::ReplayEvent::GameBoyLinkState { .. }
                                    | zeff_emu_common::replay::ReplayEvent::GameBoyLinkStateAtTick { .. }
                            )
                        })
                    });
                let has_wonder_swan_link = replay_events.as_ref().is_some_and(|events| {
                    events.iter().any(|event| {
                        matches!(
                            event,
                            zeff_emu_common::replay::ReplayEvent::WonderSwanLink { .. }
                        )
                    })
                });
                let input = if matches!(&self.backend, EmuBackend::Md(_)) && replay_events.is_some()
                {
                    None
                } else {
                    Some((buttons_pressed, dpad_pressed))
                };
                let replay_load = replay_events.is_some()
                    || has_game_boy_link
                    || has_wonder_swan_link
                    || game_boy_link_start_tick.is_some()
                    || wonder_swan_link_start_tick.is_some();
                let probe = if replay_load {
                    self.backend
                        .probe_replay_state_load(
                            &state_bytes,
                            game_boy_link_start_state,
                            has_game_boy_link,
                            has_wonder_swan_link,
                        )
                        .map(Some)
                } else {
                    Ok(None)
                };
                let mut result = probe
                    .as_ref()
                    .map(|_| zeff_emu_common::StateRestoreOutcome::Exact)
                    .map_err(|err| anyhow::anyhow!("{err:#}"));
                if result.is_ok()
                    && let Some(expected_tick) = game_boy_link_start_tick
                {
                    result = match probe
                        .as_ref()
                        .ok()
                        .and_then(|probe| probe.as_ref())
                        .and_then(|(_, tick, _)| *tick)
                    {
                        Some(actual_tick) if actual_tick == expected_tick => {
                            Ok(zeff_emu_common::StateRestoreOutcome::Exact)
                        }
                        Some(actual_tick) => Err(anyhow::anyhow!(
                            "replay GB start tick mismatch: metadata={expected_tick}, state={actual_tick}"
                        )),
                        None => Err(anyhow::anyhow!(
                            "replay declares a GB start tick but current backend is not Game Boy"
                        )),
                    };
                }
                if result.is_ok()
                    && let Some(expected_tick) = wonder_swan_link_start_tick
                {
                    result = match probe
                        .as_ref()
                        .ok()
                        .and_then(|probe| probe.as_ref())
                        .and_then(|(_, _, tick)| *tick)
                    {
                        Some(actual_tick) if actual_tick == expected_tick => {
                            Ok(zeff_emu_common::StateRestoreOutcome::Exact)
                        }
                        Some(actual_tick) => Err(anyhow::anyhow!(
                            "replay WonderSwan start tick mismatch: metadata={expected_tick}, state={actual_tick}"
                        )),
                        None => Err(anyhow::anyhow!(
                            "replay declares a WonderSwan start tick but current backend is not WonderSwan"
                        )),
                    };
                }
                let target_frame = probe
                    .as_ref()
                    .ok()
                    .and_then(|probe| probe.as_ref())
                    .map_or_else(|| self.backend.frame_count(), |(frame, _, _)| *frame);
                let target_gb_tick = probe
                    .as_ref()
                    .ok()
                    .and_then(|probe| probe.as_ref())
                    .and_then(|(_, tick, _)| *tick)
                    .unwrap_or(0);
                let target_ws_tick = probe
                    .as_ref()
                    .ok()
                    .and_then(|probe| probe.as_ref())
                    .and_then(|(_, _, tick)| *tick)
                    .unwrap_or(0);
                let mut staged_game_boy_replay_link = None;
                let mut staged_wonder_swan_replay_link = None;
                if result.is_ok() {
                    match replay_events
                        .as_ref()
                        .map(|events| {
                            crate::link::gb::GameBoyReplayLink::try_new_with_start(
                                events.clone(),
                                target_frame,
                                game_boy_link_start_tick,
                                target_gb_tick,
                                game_boy_link_coordinator_start_state,
                            )
                        })
                        .transpose()
                    {
                        Ok(Some(link)) if !link.is_empty() => {
                            staged_game_boy_replay_link = Some(link);
                        }
                        Ok(_) => {}
                        Err(err) => result = Err(err),
                    }
                }
                if result.is_ok() {
                    match replay_events
                        .as_ref()
                        .map(|events| {
                            crate::link::ws_replay::WonderSwanReplayLink::try_new(
                                events.clone(),
                                target_frame,
                                wonder_swan_link_start_tick,
                                target_ws_tick,
                            )
                        })
                        .transpose()
                    {
                        Ok(Some(link)) if !link.is_empty() => {
                            staged_wonder_swan_replay_link = Some(link);
                        }
                        Ok(_) => {}
                        Err(err) => result = Err(err),
                    }
                }
                if result.is_ok() {
                    result = self
                        .backend
                        .load_state_from_bytes(state_bytes)
                        .and_then(|outcome| {
                            anyhow::ensure!(
                                outcome == zeff_emu_common::StateRestoreOutcome::Exact,
                                "replay restore requires an exact native save state"
                            );
                            Ok(outcome)
                        });
                }
                let state_loaded = result.is_ok();
                if state_loaded {
                    if has_game_boy_link || has_wonder_swan_link {
                        self.disconnect_tcp_link();
                    }
                    if has_game_boy_link && let Some(state) = game_boy_link_start_state {
                        let restored = self.backend.restore_game_boy_link_replay_state(state);
                        debug_assert!(restored, "preflighted Game Boy link state must commit");
                    }
                    self.game_boy_replay_link = staged_game_boy_replay_link;
                    self.wonder_swan_replay_link = staged_wonder_swan_replay_link;
                }
                if !self.finalize_load_state(result, "(replay)".to_string(), input) {
                    return false;
                }
            }

            EmuCommand::InspectRecovery {
                resume,
                buttons_pressed,
                dpad_pressed,
            } => {
                return self.handle_recovery_inspection(resume, buttons_pressed, dpad_pressed);
            }

            EmuCommand::Shutdown => {
                self.finish_shutdown();
                return false;
            }
            EmuCommand::StartNetplay(_)
            | EmuCommand::StepNetplay(_)
            | EmuCommand::SendNetplayChat(_)
            | EmuCommand::SetNetplayPaused(_)
            | EmuCommand::StopNetplay => {
                unreachable!("netplay dispatch precedes gameplay dispatch")
            }

            EmuCommand::SuspendTasRepair { .. }
            | EmuCommand::ResumeTasRepair { .. }
            | EmuCommand::DiscardTasRepair { .. }
            | EmuCommand::CommitRepairedTasWorker { .. }
            | EmuCommand::DiscardRepairedTasWorker { .. }
            | EmuCommand::InspectTasReadiness { .. }
            | EmuCommand::AcquireTasControl { .. }
            | EmuCommand::ExecuteTasControl(_)
            | EmuCommand::AdvanceTasControl(_)
            | EmuCommand::RollbackTasControl { .. }
            | EmuCommand::CommitTasControl { .. } => {
                unreachable!("TAS control command escaped authority dispatch")
            }

            EmuCommand::SetAudioRecordingCapture { .. }
            | EmuCommand::SetSampleRate(_)
            | EmuCommand::SetUncapped(_)
            | EmuCommand::ApplyMediaEvent(_)
            | EmuCommand::CaptureStateBytes
            | EmuCommand::ExecuteGuestCall(_)
            | EmuCommand::UndoGuestCall(_)
            | EmuCommand::CaptureReplayCheckpoint { .. }
            | EmuCommand::SetGameBoySerialDevice(_)
            | EmuCommand::QueueBardigunBarcodeScan(_)
            | EmuCommand::TriggerBarcodeBoyScan(_)
            | EmuCommand::RestoreGameBoyLinkState(_)
            | EmuCommand::UpdateCheats(_)
            | EmuCommand::Rewind(_)
            | EmuCommand::Reset => unreachable!("shared command escaped common dispatch"),
        }
        true
    }
}
