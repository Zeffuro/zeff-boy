use crossbeam_channel::{Receiver, Sender};
use std::sync::mpsc::{self, Receiver as StdReceiver, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;
use crate::link::transport::TcpLinkTransport;
use crate::link::{
    LinkConnectionState, LinkEndpointId, LinkSession, LinkSystemType, RemoteLink,
    remote_link_system_for_active_system,
};

use super::{
    AudioRecordingCapture, DEFAULT_REWIND_SECONDS, EmuCommand, EmuResponse, EmuThread, FrameResult,
    REWIND_SNAPSHOTS_PER_SECOND, ReplayStartState, SharedFramebuffer, TcpLinkMode,
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
    audio_recording_capture: AudioRecordingCapture,
    pending_audio_discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    last_cheats: Vec<CheatPatch>,
    tcp_link: Option<RemoteLink<TcpLinkTransport>>,
    pending_tcp_link: Option<PendingTcpLink>,
    game_boy_replay_link: Option<crate::link::gb::GameBoyReplayLink>,
    wonder_swan_replay_link: Option<crate::link::ws_replay::WonderSwanReplayLink>,
    rewind_buffer: zeff_emu_common::rewind::RewindBuffer,
    rewind_seconds: usize,
}

impl EmuLoop {
    pub(super) fn new(
        backend: EmuBackend,
        cmd_rx: Receiver<EmuCommand>,
        frame_tx: Sender<FrameResult>,
        drain_rx: Receiver<FrameResult>,
        resp_tx: Sender<EmuResponse>,
        shared_framebuffer: SharedFramebuffer,
    ) -> Self {
        Self {
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            shared_framebuffer,
            uncapped_mode: false,
            audio_recording_capture: AudioRecordingCapture::default(),
            pending_audio_discontinuities: Vec::new(),
            last_cheats: Vec::new(),
            tcp_link: None,
            pending_tcp_link: None,
            game_boy_replay_link: None,
            wonder_swan_replay_link: None,
            rewind_buffer: zeff_emu_common::rewind::RewindBuffer::new(
                DEFAULT_REWIND_SECONDS,
                REWIND_SNAPSHOTS_PER_SECOND,
            ),
            rewind_seconds: DEFAULT_REWIND_SECONDS,
        }
    }

    pub(super) fn run(&mut self) {
        loop {
            self.poll_tcp_link_connection();
            self.clear_disconnected_tcp_link();

            let command = if self.uncapped_mode {
                match self.cmd_rx.try_recv() {
                    Ok(cmd) => Some(cmd),
                    Err(crossbeam_channel::TryRecvError::Empty) => None,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            } else if self.pending_tcp_link.is_some() {
                match self.cmd_rx.recv_timeout(PENDING_LINK_POLL_INTERVAL) {
                    Ok(cmd) => Some(cmd),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match self.cmd_rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => break,
                }
            };

            if let Some(command) = command {
                if !self.handle_command(command) {
                    break;
                }
            } else if self.uncapped_mode {
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
                );
            }
        }
    }

    fn handle_command(&mut self, command: EmuCommand) -> bool {
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
                self.uncapped_mode = on;
                self.backend.set_apu_sample_generation_enabled(!on);
            }

            EmuCommand::UpdateCheats(cheats) => {
                self.last_cheats = cheats;
                self.backend.install_rom_patches(&self.last_cheats);
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
                let result = if let Some(replay_link) = self.game_boy_replay_link.as_mut() {
                    EmuThread::handle_step_frames_with_game_boy_replay_link(
                        &mut self.backend,
                        input,
                        &self.last_cheats,
                        replay_link,
                        self.uncapped_mode,
                        &mut self.rewind_buffer,
                        &mut self.rewind_seconds,
                        &self.shared_framebuffer,
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
                        &self.shared_framebuffer,
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
                        &self.shared_framebuffer,
                    );
                    self.clear_disconnected_tcp_link();
                    result
                };
                if debugger_mutation {
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    );
                }
                let mut result = result;
                self.attach_audio_discontinuities(&mut result);
                if !EmuThread::send_frame(
                    &self.frame_tx,
                    &self.drain_rx,
                    result,
                    self.audio_recording_capture.active,
                ) {
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

            EmuCommand::SetFdsDiskSide(side) => {
                let resp = match self.backend.set_fds_disk_side(side) {
                    Ok(()) => {
                        let selected = self.backend.fds_disk_side().unwrap_or(side);
                        EmuResponse::FdsDiskSideChanged(selected)
                    }
                    Err(err) => EmuResponse::FdsDiskSideChangeFailed(err.to_string()),
                };
                if !self.send_resp(resp) {
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
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::UndoGuestCall(state) => {
                let resp = match self.backend.load_state_from_bytes(state) {
                    Ok(()) => EmuResponse::GuestCallUndone,
                    Err(error) => EmuResponse::GuestCallUndoFailed(error.to_string()),
                };
                super::types::publish_framebuffer(
                    &self.shared_framebuffer,
                    self.backend.framebuffer(),
                );
                if matches!(&resp, EmuResponse::GuestCallUndone) {
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::GuestCallUndo,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::CaptureReplayStart { capture_id } => {
                let resp = if let Some(blocker) = self.replay_start_capture_blocker() {
                    EmuResponse::ReplayStartCaptureFailed {
                        capture_id,
                        error: format!("replay start rejected: {blocker}"),
                    }
                } else {
                    let metadata = self.capture_replay_metadata();
                    match EmuThread::encode_current_state(&self.backend) {
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
                let resp = match EmuThread::encode_current_state(&self.backend) {
                    Ok(state_bytes) => EmuResponse::ReplayCheckpointCaptured { frame, state_bytes },
                    Err(err) => EmuResponse::ReplayCheckpointCaptureFailed {
                        frame,
                        error: err.to_string(),
                    },
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
                game_boy_link_start_tick,
                wonder_swan_link_start_tick,
            } => {
                let mut result = self.backend.load_state_from_bytes(state_bytes);
                let state_loaded = result.is_ok();
                if result.is_ok()
                    && let Some(expected_tick) = game_boy_link_start_tick
                {
                    result = match self.backend.game_boy_cpu_cycles() {
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
                    result = match self.backend.wonder_swan_cpu_cycles() {
                        Some(actual_tick) if actual_tick == expected_tick => Ok(()),
                        Some(actual_tick) => Err(anyhow::anyhow!(
                            "replay WonderSwan start tick mismatch: metadata={expected_tick}, state={actual_tick}"
                        )),
                        None => Err(anyhow::anyhow!(
                            "replay declares a WonderSwan start tick but current backend is not WonderSwan"
                        )),
                    };
                }
                if result.is_ok() {
                    self.game_boy_replay_link = None;
                    self.wonder_swan_replay_link = None;
                    let has_game_boy_link_events = replay_events.as_ref().is_some_and(|events| {
                        events.iter().any(|event| {
                            matches!(
                                event,
                                zeff_emu_common::replay::ReplayEvent::GameBoyLink { .. }
                                    | zeff_emu_common::replay::ReplayEvent::GameBoyLinkState { .. }
                                    | zeff_emu_common::replay::ReplayEvent::GameBoyLinkStateAtTick { .. }
                            )
                        })
                    });
                    if has_game_boy_link_events && let Some(state) = game_boy_link_start_state {
                        self.backend.restore_game_boy_link_replay_state(state);
                    }
                    match replay_events
                        .as_ref()
                        .map(|events| {
                            crate::link::gb::GameBoyReplayLink::try_new(
                                events.clone(),
                                self.backend.frame_count(),
                                game_boy_link_start_tick,
                                self.backend.game_boy_cpu_cycles().unwrap_or(0),
                            )
                        })
                        .transpose()
                    {
                        Ok(Some(link)) if !link.is_empty() => {
                            self.game_boy_replay_link = Some(link);
                            self.disconnect_tcp_link();
                        }
                        Ok(_) => self.game_boy_replay_link = None,
                        Err(err) => result = Err(err),
                    }
                    if result.is_ok() {
                        match replay_events
                            .as_ref()
                            .map(|events| {
                                crate::link::ws_replay::WonderSwanReplayLink::try_new(
                                    events.clone(),
                                    self.backend.frame_count(),
                                    wonder_swan_link_start_tick,
                                    self.backend.wonder_swan_cpu_cycles().unwrap_or(0),
                                )
                            })
                            .transpose()
                        {
                            Ok(Some(link)) if !link.is_empty() => {
                                self.wonder_swan_replay_link = Some(link);
                                self.disconnect_tcp_link();
                            }
                            Ok(_) => self.wonder_swan_replay_link = None,
                            Err(err) => result = Err(err),
                        }
                    }
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

            EmuCommand::AutoSaveState => {
                if let Some(path) = self.backend.auto_save_path() {
                    if !EmuThread::save_state_async(
                        &self.backend,
                        path,
                        &self.resp_tx,
                        &self.send_resp_fn(),
                    ) {
                        return false;
                    }
                } else if !self.send_resp(EmuResponse::SaveStateFailed(
                    "Auto-save not supported for this system".to_string(),
                )) {
                    return false;
                }
            }

            EmuCommand::AutoLoadState {
                buttons_pressed,
                dpad_pressed,
            } => {
                return self.handle_auto_load(buttons_pressed, dpad_pressed);
            }

            EmuCommand::Rewind => {
                let resp = EmuThread::handle_rewind(
                    &mut self.backend,
                    &mut self.rewind_buffer,
                    &self.shared_framebuffer,
                );
                if matches!(&resp, EmuResponse::RewindOk) {
                    self.backend.install_rom_patches(&self.last_cheats);
                    self.mark_audio_discontinuity(
                        crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
                    );
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::Shutdown => {
                self.disconnect_tcp_link();
                self.handle_shutdown();
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

    fn replay_start_capture_blocker(&self) -> Option<String> {
        let Some(RemoteLink::GameBoy(link)) = self.tcp_link.as_ref() else {
            return None;
        };
        game_boy_replay_start_capture_blocker(
            link.pending_master_transfer_id(),
            self.backend.game_boy_link_replay_state(),
        )
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
            self.rewind_buffer.clear();
            self.backend.install_rom_patches(&self.last_cheats);
            self.mark_audio_discontinuity(
                crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad,
            );
        }
        self.send_resp(resp)
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

    fn handle_auto_load(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        if let Some(path) = self.backend.auto_save_path() {
            if path.exists() {
                let label = path.display().to_string();
                let result = self.backend.load_state_from_path(&path);
                let state_loaded = result.is_ok();
                self.finalize_load_state(result, label, buttons_pressed, dpad_pressed, state_loaded)
            } else {
                self.send_resp(EmuResponse::LoadStateFailed("no auto-save".to_string()))
            }
        } else {
            self.send_resp(EmuResponse::LoadStateFailed("no auto-save".to_string()))
        }
    }

    fn handle_shutdown(&mut self) {
        let sram_path = self.backend.flush_battery_sram().unwrap_or_else(|err| {
            log::error!("Failed to flush SRAM on shutdown: {}", err);
            None
        });
        if self
            .resp_tx
            .send(EmuResponse::SramFlushed(sram_path))
            .is_err()
        {
            log::debug!("shutdown: SRAM flush response dropped (receiver closed)");
        }
        if self.resp_tx.send(EmuResponse::ShutdownComplete).is_err() {
            log::debug!("shutdown: completion response dropped (receiver closed)");
        }
    }
}

fn game_boy_replay_start_capture_blocker(
    pending_live_transfer_id: Option<u64>,
    state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
) -> Option<String> {
    if let Some(transfer_id) = pending_live_transfer_id {
        return Some(format!(
            "GB live link transfer {transfer_id} is waiting for a peer reply"
        ));
    }

    let state = state?;
    if state.pending_master_byte.is_some()
        && state.pending_master_response.is_none()
        && state.queued_master_action.is_none()
    {
        return Some(
            "GB live link local master transfer has already left the core but has no recorded reply"
                .to_string(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{EmuLoop, game_boy_replay_start_capture_blocker};
    use crate::audio_tooling::{
        AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState,
        NTSC_60_TEMPO_US_PER_BEAT,
    };
    use crate::emu_backend::EmuBackend;
    use crate::emu_thread::{AudioRecordingCapture, EmuCommand, FrameResult};
    use std::path::PathBuf;

    fn test_loop() -> (
        EmuLoop,
        crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
    ) {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x00],
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        let backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
        let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
        let drain_rx = frame_rx.clone();
        let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
        let shared = crate::emu_thread::types::new_shared_framebuffer();
        (
            EmuLoop::new(backend, cmd_rx, frame_tx, drain_rx, resp_tx, shared),
            resp_rx,
        )
    }

    fn semantic_result() -> FrameResult {
        FrameResult {
            advanced_frames: 1,
            replay_events: Vec::new(),
            replay_error: None,
            rumble: false,
            audio_samples: Vec::new(),
            ui_data: crate::ui::UiFrameData::default(),
            is_mbc7: false,
            is_pocket_camera: false,
            rewind_fill: 0.0,
            audio_semantic_frames: vec![AudioSemanticFrame {
                frame: 1,
                tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
                voices: vec![AudioVoiceState {
                    channel: AudioChannelId(0),
                    name: "Test",
                    class: AudioVoiceClass::Tone,
                    active: false,
                    pitch_hz: Some(440.0),
                    level: Some(0.0),
                }],
            }],
            audio_timeline_discontinuities: Vec::new(),
        }
    }

    fn replay_link_state(
        pending_master_byte: Option<u8>,
        pending_master_response: Option<u8>,
        queued_master_action: Option<zeff_emu_common::replay::ReplayGameBoyLinkAction>,
    ) -> zeff_emu_common::replay::ReplayGameBoyLinkState {
        zeff_emu_common::replay::ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte,
            pending_master_response,
            pending_master_completion_ready: false,
            queued_master_action,
            serial_generation: 7,
        }
    }

    #[test]
    fn replay_start_capture_rejects_pending_live_master_transfer() {
        assert!(
            game_boy_replay_start_capture_blocker(Some(17), None)
                .unwrap()
                .contains("17")
        );
    }

    #[test]
    fn replay_start_capture_rejects_consumed_core_master_without_reply() {
        let state = replay_link_state(Some(0x12), None, None);

        assert!(game_boy_replay_start_capture_blocker(None, Some(state)).is_some());
    }

    #[test]
    fn replay_start_capture_allows_queued_or_replied_core_master() {
        let queued = replay_link_state(
            Some(0x12),
            None,
            Some(zeff_emu_common::replay::ReplayGameBoyLinkAction {
                out_byte: 0x12,
                clock_period_t_cycles: 4096,
                serial_generation: 7,
            }),
        );
        let replied = replay_link_state(Some(0x12), Some(0x34), None);

        assert_eq!(
            game_boy_replay_start_capture_blocker(None, Some(queued)),
            None
        );
        assert_eq!(
            game_boy_replay_start_capture_blocker(None, Some(replied)),
            None
        );
    }

    #[test]
    fn state_load_discontinuity_survives_post_load_validation_failure() {
        let (mut emu_loop, _responses) = test_loop();
        emu_loop.audio_recording_capture = AudioRecordingCapture {
            active: true,
            semantic: true,
        };
        let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();

        assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
            state_bytes,
            buttons_pressed: 0,
            dpad_pressed: 0,
            replay_events: None,
            game_boy_link_start_state: None,
            game_boy_link_start_tick: Some(1),
            wonder_swan_link_start_tick: None,
        }));
        assert_eq!(
            emu_loop.pending_audio_discontinuities,
            vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
        );

        let mut pre_mutation_result = semantic_result();
        pre_mutation_result.audio_semantic_frames.clear();
        emu_loop.attach_audio_discontinuities(&mut pre_mutation_result);
        assert!(
            pre_mutation_result
                .audio_timeline_discontinuities
                .is_empty()
        );
        assert_eq!(emu_loop.pending_audio_discontinuities.len(), 1);

        let mut post_mutation_result = semantic_result();
        emu_loop.attach_audio_discontinuities(&mut post_mutation_result);
        assert_eq!(
            post_mutation_result.audio_timeline_discontinuities,
            vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
        );
        assert!(emu_loop.pending_audio_discontinuities.is_empty());
    }

    #[test]
    fn failed_state_decode_does_not_start_a_semantic_epoch() {
        let (mut emu_loop, _responses) = test_loop();
        emu_loop.audio_recording_capture = AudioRecordingCapture {
            active: true,
            semantic: true,
        };

        assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
            state_bytes: vec![0xFF],
            buttons_pressed: 0,
            dpad_pressed: 0,
            replay_events: None,
            game_boy_link_start_state: None,
            game_boy_link_start_tick: None,
            wonder_swan_link_start_tick: None,
        }));
        assert!(emu_loop.pending_audio_discontinuities.is_empty());
    }
}
