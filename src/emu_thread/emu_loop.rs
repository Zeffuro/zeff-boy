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
    DEFAULT_REWIND_SECONDS, EmuCommand, EmuResponse, EmuThread, FrameResult,
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
    last_cheats: Vec<CheatPatch>,
    tcp_link: Option<RemoteLink<TcpLinkTransport>>,
    pending_tcp_link: Option<PendingTcpLink>,
    game_boy_replay_link: Option<crate::link::gb::GameBoyReplayLink>,
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
            last_cheats: Vec::new(),
            tcp_link: None,
            pending_tcp_link: None,
            game_boy_replay_link: None,
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
                );
            }
        }
    }

    fn handle_command(&mut self, command: EmuCommand) -> bool {
        match command {
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
                self.start_tcp_link(mode);
            }

            EmuCommand::DisconnectLink => {
                self.disconnect_tcp_link();
                self.game_boy_replay_link = None;
                if !self.send_resp(EmuResponse::LinkDisconnected {
                    frame_count: self.backend.frame_count(),
                    game_boy_link_state: self.backend.game_boy_link_replay_state(),
                }) {
                    return false;
                }
            }

            EmuCommand::StepFrames(input) => {
                let input = *input;
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
                if !EmuThread::send_frame(&self.frame_tx, &self.drain_rx, result) {
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
                let label = result.as_ref().ok().cloned().unwrap_or_default();
                if !self.finalize_load_state(
                    result.map(|_| ()),
                    label,
                    buttons_pressed,
                    dpad_pressed,
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
                if !self.finalize_load_state(result, label, buttons_pressed, dpad_pressed) {
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

            EmuCommand::CaptureReplayStart => {
                let resp = if let Some(blocker) = self.replay_start_capture_blocker() {
                    EmuResponse::StateCaptureFailed(format!("replay start rejected: {blocker}"))
                } else {
                    let metadata = self.capture_replay_metadata();
                    match EmuThread::encode_current_state(&self.backend) {
                        Ok(bytes) => EmuResponse::ReplayStartCaptured(Box::new(ReplayStartState {
                            state_bytes: bytes,
                            frame_count: self.backend.frame_count(),
                            game_boy_cpu_cycles: self.backend.game_boy_cpu_cycles(),
                            metadata,
                        })),
                        Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
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
                game_boy_link_start_tick,
            } => {
                let mut result = self.backend.load_state_from_bytes(state_bytes);
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
                if result.is_ok() {
                    let has_game_boy_link_events = replay_events.as_ref().is_some_and(|events| {
                        events.iter().any(|event| {
                            matches!(
                                event,
                                zeff_emu_common::replay::ReplayEvent::GameBoyLink { .. }
                                    | zeff_emu_common::replay::ReplayEvent::GameBoyLinkState { .. }
                            )
                        })
                    });
                    if has_game_boy_link_events && let Some(state) = game_boy_link_start_state {
                        self.backend.restore_game_boy_link_replay_state(state);
                    }
                    self.game_boy_replay_link = replay_events.and_then(|events| {
                        let link = crate::link::gb::GameBoyReplayLink::new(
                            events,
                            self.backend.frame_count(),
                            game_boy_link_start_tick,
                            self.backend.game_boy_cpu_cycles().unwrap_or(0),
                        );
                        (!link.is_empty()).then_some(link)
                    });
                    if self.game_boy_replay_link.is_some() {
                        self.disconnect_tcp_link();
                    }
                }
                if !self.finalize_load_state(
                    result,
                    "(replay)".to_string(),
                    buttons_pressed,
                    dpad_pressed,
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
            metadata.game_boy_link_start_state = self
                .backend
                .game_boy_link_replay_state()
                .filter(|state| !state.is_idle());
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
    ) -> bool {
        let loaded = result.is_ok();
        let resp = EmuThread::respond_load_state(
            &mut self.backend,
            result,
            label,
            buttons_pressed,
            dpad_pressed,
            &self.shared_framebuffer,
        );
        if loaded {
            self.rewind_buffer.clear();
            self.backend.install_rom_patches(&self.last_cheats);
        }
        self.send_resp(resp)
    }

    fn handle_auto_load(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        if let Some(path) = self.backend.auto_save_path() {
            if path.exists() {
                let label = path.display().to_string();
                let result = self.backend.load_state_from_path(&path);
                self.finalize_load_state(result, label, buttons_pressed, dpad_pressed)
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
    use super::game_boy_replay_start_capture_blocker;

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
}
