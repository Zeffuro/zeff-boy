use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver as StdReceiver, TryRecvError};
use std::thread;
use std::time::Duration;
use zeff_emu_common::time::Reset as _;

use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;
use crate::link::transport::TcpLinkTransport;
use crate::link::{
    LinkConnectionState, LinkEndpointId, LinkSession, LinkSystemType, RemoteLink,
    remote_link_system_for_active_system,
};

use super::persistence::BatteryFlushSchedule;
use super::recovery::{
    RecoveryCandidate, RecoveryCoordinator, TerminalRecoveryBarrier, should_load_recovery,
};
use super::speculation::{SpeculationBoundary, TerminalPersistenceReady};
use super::{
    AudioRecordingCapture, DEFAULT_REWIND_SECONDS, EmuCommand, EmuResponse, EmuThread, FrameResult,
    REWIND_CAPTURE_INTERVAL_FRAMES, ReplayStartState, SharedFramebuffer, TcpLinkMode,
    WorkerRuntimeFault,
};

const PENDING_LINK_POLL_INTERVAL: Duration = Duration::from_millis(10);

struct PendingTcpLink {
    label: String,
    endpoint: LinkEndpointId,
    system: LinkSystemType,
    receiver: StdReceiver<Result<TcpLinkTransport, String>>,
}

pub(super) struct EmuLoop {
    pub(super) backend: EmuBackend,
    pub(super) cmd_rx: Receiver<EmuCommand>,
    pub(super) frame_tx: Sender<FrameResult>,
    pub(super) drain_rx: Receiver<FrameResult>,
    pub(super) resp_tx: Sender<EmuResponse>,
    pub(super) shared_framebuffer: SharedFramebuffer,
    uncapped_mode: bool,
    uncapped_batch_size: usize,
    audio_recording_capture: AudioRecordingCapture,
    pending_audio_discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    last_cheats: Vec<CheatPatch>,
    tcp_link: Option<RemoteLink<TcpLinkTransport>>,
    pending_tcp_link: Option<PendingTcpLink>,
    game_boy_replay_link: Option<crate::link::gb::GameBoyReplayLink>,
    wonder_swan_replay_link: Option<crate::link::ws_replay::WonderSwanReplayLink>,
    rewind_buffer: zeff_emu_common::rewind::RewindBuffer,
    rewind_seconds: usize,
    frame_duration_ns: Arc<AtomicU64>,
    runtime_fault: WorkerRuntimeFault,
    battery_flush: BatteryFlushSchedule,
    recovery: RecoveryCoordinator,
    speculation: SpeculationBoundary,
    save_recovery_on_shutdown: bool,
}

pub(super) struct EmuLoopConfig {
    pub(super) shared_framebuffer: SharedFramebuffer,
    pub(super) save_recovery_on_shutdown: bool,
    #[cfg(test)]
    pub(super) recovery: Option<super::recovery::RecoveryTestConfig>,
}

impl EmuLoop {
    pub(super) fn new(
        backend: EmuBackend,
        cmd_rx: Receiver<EmuCommand>,
        frame_tx: Sender<FrameResult>,
        drain_rx: Receiver<FrameResult>,
        resp_tx: Sender<EmuResponse>,
        config: EmuLoopConfig,
    ) -> Self {
        let frame_duration_ns = backend.nominal_frame_duration_ns();
        let runtime_frame_duration_ns = Arc::new(AtomicU64::new(frame_duration_ns));
        runtime_frame_duration_ns.store(frame_duration_ns, Ordering::Release);
        #[cfg(test)]
        let recovery = match config.recovery {
            Some(config) => RecoveryCoordinator::new_for_test(&backend, config),
            None => RecoveryCoordinator::new(&backend),
        };
        #[cfg(not(test))]
        let recovery = RecoveryCoordinator::new(&backend);
        Self {
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            shared_framebuffer: config.shared_framebuffer,
            uncapped_mode: false,
            uncapped_batch_size: super::DEFAULT_UNCAPPED_BATCH_SIZE,
            audio_recording_capture: AudioRecordingCapture::default(),
            pending_audio_discontinuities: Vec::new(),
            last_cheats: Vec::new(),
            tcp_link: None,
            pending_tcp_link: None,
            game_boy_replay_link: None,
            wonder_swan_replay_link: None,
            rewind_buffer: zeff_emu_common::rewind::RewindBuffer::new_with_frame_duration(
                DEFAULT_REWIND_SECONDS,
                REWIND_CAPTURE_INTERVAL_FRAMES,
                frame_duration_ns,
            ),
            rewind_seconds: DEFAULT_REWIND_SECONDS,
            frame_duration_ns: runtime_frame_duration_ns,
            runtime_fault: WorkerRuntimeFault::default(),
            battery_flush: BatteryFlushSchedule::new(std::time::Instant::now()),
            recovery,
            speculation: SpeculationBoundary::default(),
            save_recovery_on_shutdown: config.save_recovery_on_shutdown,
        }
    }

    pub(super) fn set_frame_duration_handle(&mut self, frame_duration_ns: Arc<AtomicU64>) {
        frame_duration_ns.store(self.backend.nominal_frame_duration_ns(), Ordering::Release);
        self.frame_duration_ns = frame_duration_ns;
    }

    pub(super) fn run(&mut self) {
        loop {
            self.poll_tcp_link_connection();
            self.clear_disconnected_tcp_link();
            self.flush_battery_sram_if_due(std::time::Instant::now());

            let command = if self.uncapped_mode {
                match self.cmd_rx.try_recv() {
                    Ok(cmd) => Some(cmd),
                    Err(crossbeam_channel::TryRecvError::Empty) => None,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        self.finish_shutdown();
                        break;
                    }
                }
            } else if let Some(timeout) = self.command_wait_timeout(std::time::Instant::now()) {
                match self.cmd_rx.recv_timeout(timeout) {
                    Ok(cmd) => Some(cmd),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        self.finish_shutdown();
                        break;
                    }
                }
            } else {
                match self.cmd_rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => {
                        self.finish_shutdown();
                        break;
                    }
                }
            };

            if let Some(command) = command {
                let is_shutdown = matches!(&command, EmuCommand::Shutdown);
                if !self.handle_command(command) {
                    if !is_shutdown {
                        self.finish_shutdown();
                    }
                    break;
                }
            } else if self.uncapped_mode {
                self.speculation.invalidate();
                let frame_count = self.backend.frame_count();
                EmuThread::run_uncapped_batch(
                    &mut self.backend,
                    &self.last_cheats,
                    self.tcp_link.as_mut(),
                    &self.shared_framebuffer,
                    &self.rewind_buffer,
                    &self.frame_tx,
                    &self.drain_rx,
                    self.audio_recording_capture,
                    &mut self.pending_audio_discontinuities,
                    &mut self.runtime_fault,
                    self.uncapped_batch_size,
                );
                if self.backend.frame_count() != frame_count {
                    self.battery_flush.mark_potentially_dirty();
                }
                if !self.runtime_fault.can_step() {
                    self.uncapped_mode = false;
                }
            }
        }
    }

    fn handle_command(&mut self, command: EmuCommand) -> bool {
        self.speculation.invalidate();
        match command {
            EmuCommand::SetAudioRecordingCapture {
                capture,
                acknowledged,
            } => {
                if capture.semantic && !self.audio_recording_capture.semantic {
                    self.pending_audio_discontinuities.clear();
                }
                self.audio_recording_capture = capture;
                if let Some(acknowledged) = acknowledged {
                    let _ = acknowledged.send(());
                }
            }

            EmuCommand::SetUncapped(on) => {
                self.uncapped_mode = on && self.runtime_fault.can_step();
                self.backend
                    .set_apu_sample_generation_enabled(!self.uncapped_mode);
            }

            EmuCommand::SetUncappedBatchSize(frames) => {
                self.uncapped_batch_size = frames.clamp(1, super::MAX_UNCAPPED_BATCH_SIZE);
            }

            EmuCommand::UpdateCheats(cheats) => {
                if self.backend.supports_cheats() {
                    self.last_cheats = cheats;
                    self.backend.install_rom_patches(&self.last_cheats);
                } else {
                    self.last_cheats.clear();
                }
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
                let input = *input;
                self.audio_recording_capture = input.audio.recording_capture;
                let debugger_mutation = !input.debug_actions.memory_writes.is_empty();
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
                let detached_frame = self.speculation.run_detached_frame(
                    &self.backend,
                    detached_request,
                    &result,
                    self.runtime_fault.can_step(),
                );
                self.speculation.commit_primary_frame(
                    &self.shared_framebuffer,
                    self.backend.framebuffer(),
                    detached_frame,
                );
                if debugger_mutation && self.runtime_fault.can_step() {
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    );
                }
                let mut result = result;
                if result.advanced_frames != 0 || debugger_mutation {
                    self.battery_flush.mark_potentially_dirty();
                }
                if !self.runtime_fault.can_step() {
                    self.uncapped_mode = false;
                }
                self.attach_audio_discontinuities(&mut result);
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
                let state_loaded = result.is_ok();
                let label = result.as_ref().ok().cloned().unwrap_or_default();
                if !self.finalize_load_state(
                    result.map(|_| ()),
                    label,
                    buttons_pressed,
                    dpad_pressed,
                    state_loaded,
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
                let state_loaded = result.is_ok();
                if !self.finalize_load_state(
                    result,
                    label,
                    buttons_pressed,
                    dpad_pressed,
                    state_loaded,
                ) {
                    return false;
                }
            }

            EmuCommand::SetSampleRate(rate) => {
                self.backend.set_sample_rate(rate);
            }

            EmuCommand::ApplyMediaEvent(event) => {
                let resp = match self.backend.apply_media_event(&event) {
                    Ok(()) => match self.backend.media_slot_snapshot() {
                        Some(snapshot) => EmuResponse::MediaEventApplied {
                            event,
                            snapshot,
                            frame_count: self.backend.frame_count(),
                        },
                        None => EmuResponse::MediaEventFailed {
                            event,
                            error: "media slot disappeared after applying event".to_string(),
                        },
                    },
                    Err(err) => EmuResponse::MediaEventFailed {
                        event,
                        error: err.to_string(),
                    },
                };
                if matches!(&resp, EmuResponse::MediaEventApplied { .. }) {
                    self.battery_flush.mark_potentially_dirty();
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::SetGameBoySerialDevice(device) => {
                self.backend.set_game_boy_serial_device(device);
            }

            EmuCommand::QueueBardigunBarcodeScan(bytes) => {
                let byte_count = bytes.len();
                let response = match self.backend.queue_bardigun_barcode_scan(bytes) {
                    Ok(()) => EmuResponse::BardigunBarcodeScanStarted(byte_count),
                    Err(err) => EmuResponse::BardigunBarcodeScanFailed(err.to_string()),
                };
                if !self.send_resp(response) {
                    return false;
                }
            }

            EmuCommand::TriggerBarcodeBoyScan(digits) => {
                let response = match self.backend.trigger_barcode_boy_scan(&digits) {
                    Ok(()) => EmuResponse::BarcodeBoyScanStarted,
                    Err(err) => EmuResponse::BarcodeBoyScanFailed(err.to_string()),
                };
                if !self.send_resp(response) {
                    return false;
                }
            }

            EmuCommand::RestoreGameBoyLinkState(state) => {
                self.backend.restore_game_boy_link_replay_state(state);
            }

            EmuCommand::CaptureStateBytes => {
                let resp = match EmuThread::encode_current_state(&self.backend) {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                };
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::ExecuteGuestCall(request) => {
                let name = request.name.clone();
                let resp = match self.backend.execute_guest_call(&request) {
                    Ok((instructions, undo_state)) => EmuResponse::GuestCallCompleted {
                        name,
                        instructions,
                        undo_state,
                    },
                    Err(error) => EmuResponse::GuestCallFailed {
                        name,
                        error: error.to_string(),
                    },
                };
                super::types::publish_framebuffer(
                    &self.shared_framebuffer,
                    self.backend.framebuffer(),
                );
                if matches!(&resp, EmuResponse::GuestCallCompleted { .. }) {
                    self.battery_flush.mark_potentially_dirty();
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::UndoGuestCall(state) => {
                let resp = if self.backend.supports_guest_calls() {
                    match self.backend.load_state_from_bytes(state) {
                        Ok(()) => EmuResponse::GuestCallUndone,
                        Err(error) => EmuResponse::GuestCallUndoFailed(error.to_string()),
                    }
                } else {
                    EmuResponse::GuestCallUndoFailed(
                        "guest calls are not supported by this core".to_string(),
                    )
                };
                super::types::publish_framebuffer(
                    &self.shared_framebuffer,
                    self.backend.framebuffer(),
                );
                if matches!(&resp, EmuResponse::GuestCallUndone) {
                    self.battery_flush.mark_potentially_dirty();
                    self.backend.discard_game_boy_printer_jobs();
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::GuestCallUndo,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::CaptureReplayStart { capture_id } => {
                let resp = if !self.backend.supports_replay() {
                    EmuResponse::ReplayStartCaptureFailed {
                        capture_id,
                        error: "replay capture is not supported by this core".to_string(),
                    }
                } else if let Some(blocker) = self.replay_start_capture_blocker() {
                    EmuResponse::ReplayStartCaptureFailed {
                        capture_id,
                        error: format!("replay start rejected: {blocker}"),
                    }
                } else {
                    let metadata = self.capture_replay_metadata();
                    match self.backend.encode_replay_start_state_bytes() {
                        Ok(bytes) => EmuResponse::ReplayStartCaptured {
                            capture_id,
                            start: Box::new(ReplayStartState {
                                state_bytes: bytes,
                                frame_count: self.backend.frame_count(),
                                game_boy_cpu_cycles: self.backend.game_boy_cpu_cycles(),
                                wonder_swan_cpu_cycles: self.backend.wonder_swan_cpu_cycles(),
                                metadata,
                            }),
                        },
                        Err(err) => EmuResponse::ReplayStartCaptureFailed {
                            capture_id,
                            error: err.to_string(),
                        },
                    }
                };
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::CaptureReplayCheckpoint { frame } => {
                let resp = if self.backend.supports_replay() {
                    match EmuThread::encode_current_state(&self.backend) {
                        Ok(state_bytes) => {
                            EmuResponse::ReplayCheckpointCaptured { frame, state_bytes }
                        }
                        Err(err) => EmuResponse::ReplayCheckpointCaptureFailed {
                            frame,
                            error: err.to_string(),
                        },
                    }
                } else {
                    EmuResponse::ReplayCheckpointCaptureFailed {
                        frame,
                        error: "replay capture is not supported by this core".to_string(),
                    }
                };
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
                    .map(|_| ())
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
                        Some(actual_tick) if actual_tick == expected_tick => Ok(()),
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
                        Some(actual_tick) if actual_tick == expected_tick => Ok(()),
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
                    result = self.backend.load_state_from_bytes(state_bytes);
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
                if !self.finalize_load_state(
                    result,
                    "(replay)".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    state_loaded,
                ) {
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

            EmuCommand::Rewind(steps) => {
                let resp = EmuThread::handle_rewind(
                    &mut self.backend,
                    &mut self.rewind_buffer,
                    &self.shared_framebuffer,
                    steps,
                );
                if matches!(&resp, EmuResponse::RewindOk { .. }) {
                    self.battery_flush.mark_potentially_dirty();
                    self.backend.discard_game_boy_printer_jobs();
                    self.backend.install_rom_patches(&self.last_cheats);
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::Reset => {
                self.backend.reset();
                self.battery_flush.mark_potentially_dirty();
                self.backend.install_rom_patches(&self.last_cheats);
                self.rewind_buffer.clear();
                self.runtime_fault = WorkerRuntimeFault::default();
                self.mark_audio_discontinuity(
                    crate::audio_recorder::AudioTimelineDiscontinuity::Reset,
                );
            }

            EmuCommand::Shutdown => {
                self.finish_shutdown();
                return false;
            }
        }
        true
    }

    fn send_resp(&self, resp: EmuResponse) -> bool {
        self.resp_tx.send(resp).is_ok()
    }

    fn send_resp_fn(&self) -> impl Fn(EmuResponse) -> bool + '_ {
        |resp| self.resp_tx.send(resp).is_ok()
    }

    fn replay_start_capture_blocker(&mut self) -> Option<String> {
        let state = self.backend.game_boy_link_replay_state();
        let Some(RemoteLink::GameBoy(link)) = self.tcp_link.as_mut() else {
            return game_boy_replay_start_capture_blocker(None, state);
        };
        let state = state?;
        link.replay_coordinator_state(state).err()
    }

    fn start_tcp_link(&mut self, mode: TcpLinkMode) {
        let Some(system) = remote_link_system_for_active_system(self.backend.system()) else {
            let _ = self.send_resp(EmuResponse::LinkFailed(
                "TCP link currently supports GB/GBC and WonderSwan/WSC only".to_string(),
            ));
            return;
        };

        self.disconnect_tcp_link();
        self.pending_tcp_link = None;

        let (label, endpoint, receiver) = match mode {
            TcpLinkMode::Host { bind_addr } => {
                let label = format!("hosting on {bind_addr}");
                let (sender, receiver) = mpsc::channel();
                thread::spawn(move || {
                    let result = TcpLinkTransport::host_once(bind_addr.as_str())
                        .map_err(|err| format!("Host failed: {err}"));
                    let _ = sender.send(result);
                });
                (label, LinkEndpointId(1), receiver)
            }
            TcpLinkMode::Join { connect_addr } => {
                let label = format!("joining {connect_addr}");
                let (sender, receiver) = mpsc::channel();
                thread::spawn(move || {
                    let result = TcpLinkTransport::connect(connect_addr.as_str())
                        .map_err(|err| format!("Join failed: {err}"));
                    let _ = sender.send(result);
                });
                (label, LinkEndpointId(2), receiver)
            }
        };

        self.pending_tcp_link = Some(PendingTcpLink {
            label: label.clone(),
            endpoint,
            system,
            receiver,
        });
        let _ = self.send_resp(EmuResponse::LinkPending(label));
    }

    fn capture_replay_metadata(&mut self) -> zeff_emu_common::replay::ReplayMetadata {
        let mut metadata = self.backend.replay_metadata();
        let has_connected_game_boy_link = matches!(
            self.tcp_link.as_ref(),
            Some(RemoteLink::GameBoy(link)) if link.state() != LinkConnectionState::Disconnected
        );
        if has_connected_game_boy_link {
            self.backend.set_link_peer_present(true);
        }
        metadata.game_boy_link_start_state = self
            .backend
            .game_boy_link_replay_state()
            .filter(|state| !state.is_idle());
        if let (Some(RemoteLink::GameBoy(link)), Some(state)) =
            (self.tcp_link.as_mut(), metadata.game_boy_link_start_state)
        {
            metadata.game_boy_link_coordinator_start_state = link
                .replay_coordinator_state(state)
                .expect("replay start blocker validated GB coordinator state");
            link.discard_replay_events_before_capture();
        }
        metadata
    }

    fn poll_tcp_link_connection(&mut self) {
        let Some(pending) = self.pending_tcp_link.as_ref() else {
            return;
        };

        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("link connection worker stopped".to_string()),
        };

        let pending = self
            .pending_tcp_link
            .take()
            .expect("pending link should exist after polling it");
        match result {
            Ok(transport) => {
                self.backend.set_link_peer_present(true);
                let session = LinkSession::new(transport, pending.system, pending.endpoint);
                self.tcp_link = Some(match pending.system {
                    LinkSystemType::GameBoy => {
                        RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(session))
                    }
                    LinkSystemType::WonderSwan => {
                        RemoteLink::WonderSwan(crate::link::ws::WonderSwanRemoteLink::new(session))
                    }
                    LinkSystemType::GameGear => {
                        self.backend.set_link_peer_present(false);
                        let _ = self.send_resp(EmuResponse::LinkFailed(
                            "TCP link does not support Game Gear yet".to_string(),
                        ));
                        return;
                    }
                });
                let _ = self.send_resp(EmuResponse::LinkConnected {
                    label: pending.label,
                    frame_count: self.backend.frame_count(),
                    game_boy_cpu_cycles: self.backend.game_boy_cpu_cycles(),
                    game_boy_link_state: self.backend.game_boy_link_replay_state(),
                });
            }
            Err(err) => {
                self.backend.set_link_peer_present(false);
                let _ = self.send_resp(EmuResponse::LinkFailed(err));
            }
        }
    }

    fn clear_disconnected_tcp_link(&mut self) {
        let disconnected = self
            .tcp_link
            .as_ref()
            .is_some_and(|link| link.state() == LinkConnectionState::Disconnected);
        if disconnected {
            self.tcp_link = None;
            self.backend.set_link_peer_present(false);
            let _ = self.send_resp(EmuResponse::LinkDisconnected {
                frame_count: self.backend.frame_count(),
                game_boy_cpu_cycles: self.backend.game_boy_cpu_cycles(),
                game_boy_link_state: self.backend.game_boy_link_replay_state(),
            });
        }
    }

    fn disconnect_tcp_link(&mut self) {
        self.pending_tcp_link = None;
        if let Some(mut link) = self.tcp_link.take() {
            link.disconnect();
        }
        self.backend.set_link_peer_present(false);
    }

    fn finalize_load_state(
        &mut self,
        result: anyhow::Result<()>,
        label: String,
        buttons_pressed: u8,
        dpad_pressed: u8,
        state_loaded: bool,
    ) -> bool {
        let resp = EmuThread::respond_load_state(
            &mut self.backend,
            result,
            label,
            buttons_pressed,
            dpad_pressed,
            &self.shared_framebuffer,
        );
        if state_loaded {
            self.battery_flush.mark_potentially_dirty();
            self.backend.discard_game_boy_printer_jobs();
            self.reset_rewind_buffer();
            self.backend.install_rom_patches(&self.last_cheats);
            self.mark_audio_discontinuity(
                crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad,
            );
        }
        self.send_resp(resp)
    }

    fn reset_rewind_buffer(&mut self) {
        let frame_duration_ns = EmuThread::reset_rewind_for_loaded_state(
            &mut self.rewind_buffer,
            self.rewind_seconds,
            &self.backend,
        );
        self.frame_duration_ns
            .store(frame_duration_ns, Ordering::Release);
    }

    fn mark_audio_discontinuity(
        &mut self,
        reason: crate::audio_recorder::AudioTimelineDiscontinuity,
    ) {
        if self.audio_recording_capture.semantic {
            self.pending_audio_discontinuities.push(reason);
        }
    }

    fn attach_audio_discontinuities(&mut self, result: &mut FrameResult) {
        if !result.audio_semantic_frames.is_empty() {
            result
                .audio_timeline_discontinuities
                .append(&mut self.pending_audio_discontinuities);
        }
    }

    fn handle_recovery_inspection(
        &mut self,
        resume: bool,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> bool {
        match self.recovery.inspect(&self.backend) {
            RecoveryCandidate::Missing => self.send_resp(EmuResponse::RecoveryMissing),
            RecoveryCandidate::Rejected(error) => {
                self.send_resp(EmuResponse::RecoveryRejected(error))
            }
            RecoveryCandidate::Available {
                freshness,
                native_payload,
                path,
            } if should_load_recovery(freshness, resume) => {
                let result = self.backend.load_state_from_bytes(native_payload);
                let loaded = result.is_ok();
                self.finalize_load_state(
                    result,
                    path.display().to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    loaded,
                )
            }
            RecoveryCandidate::Available { freshness, .. } => {
                self.send_resp(EmuResponse::RecoveryAvailable(freshness))
            }
        }
    }

    fn handle_shutdown(&mut self) {
        let ready = self.speculation.prepare_terminal_persistence();
        let barrier = self.commit_terminal_battery_generation(ready);
        let battery_response = terminal_battery_response(&barrier);
        let _ = self.resp_tx.send(battery_response);
        if self.save_recovery_on_shutdown && self.backend.supports_save_states() {
            let recovery_result = barrier
                .and_then(|(_, record)| self.recovery.write_recovery_state(&self.backend, record));
            let response = match recovery_result {
                Ok(path) => EmuResponse::RecoverySaved(path),
                Err(error) => EmuResponse::RecoverySaveFailed(error.to_string()),
            };
            let _ = self.resp_tx.send(response);
        }
        if self.resp_tx.send(EmuResponse::ShutdownComplete).is_err() {
            log::debug!("shutdown: completion response dropped (receiver closed)");
        }
    }

    fn finish_shutdown(&mut self) {
        self.disconnect_tcp_link();
        self.handle_shutdown();
    }

    fn command_wait_timeout(&self, now: std::time::Instant) -> Option<Duration> {
        let link_timeout = self
            .pending_tcp_link
            .is_some()
            .then_some(PENDING_LINK_POLL_INTERVAL);
        let save_timeout = if self.periodic_battery_flush_blocked() {
            None
        } else {
            self.battery_flush.wait_timeout(now)
        };
        match (link_timeout, save_timeout) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(timeout), None) | (None, Some(timeout)) => Some(timeout),
            (None, None) => None,
        }
    }

    fn periodic_battery_flush_blocked(&self) -> bool {
        self.pending_tcp_link.is_some() || self.tcp_link.is_some()
    }

    fn flush_battery_sram_if_due(&mut self, now: std::time::Instant) {
        if self.periodic_battery_flush_blocked() || !self.battery_flush.is_due(now) {
            return;
        }
        let result = self.commit_battery_generation();
        self.battery_flush.finish_attempt(now, result.is_ok());
        if let Err(error) = result {
            log::warn!("Periodic battery RAM flush failed; retrying in 30 seconds: {error:#}");
        }
    }

    fn commit_battery_generation(
        &mut self,
    ) -> anyhow::Result<(
        Option<String>,
        crate::save_paths::recovery_state::BatteryGenerationRecord,
    )> {
        let mut barrier = TerminalRecoveryBarrier::default();
        let path = self.backend.flush_battery_sram()?;
        barrier.acknowledge_battery_commit();
        let record = self.recovery.write_generation(&self.backend)?;
        barrier.acknowledge_generation_commit(record)?;
        Ok((path, barrier.envelope_witness()?))
    }

    fn commit_terminal_battery_generation(
        &mut self,
        _ready: TerminalPersistenceReady,
    ) -> anyhow::Result<(
        Option<String>,
        crate::save_paths::recovery_state::BatteryGenerationRecord,
    )> {
        self.commit_battery_generation()
    }
}

fn terminal_battery_response(
    result: &anyhow::Result<(
        Option<String>,
        crate::save_paths::recovery_state::BatteryGenerationRecord,
    )>,
) -> EmuResponse {
    match result {
        Ok((path, _)) => EmuResponse::SramFlushed(path.clone()),
        Err(error) => EmuResponse::SramFlushFailed(error.to_string()),
    }
}

fn game_boy_replay_start_capture_blocker(
    coordinator_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState>,
    state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
) -> Option<String> {
    if let Some(coordinator) = coordinator_state {
        let Some(state) = state else {
            return Some(format!(
                "GB live link transfer {} has no core link state",
                coordinator.transfer_id
            ));
        };
        return coordinator.validate_against(state).err().map(|error| {
            format!(
                "GB live link transfer {} cannot be captured: {error}",
                coordinator.transfer_id
            )
        });
    }

    let state = state?;
    if state.pending_master_byte.is_some() && state.queued_master_action.is_none() {
        return Some(
            "GB live link local master transfer has no retained coordinator ownership".to_string(),
        );
    }

    None
}

#[cfg(test)]
mod tests;
