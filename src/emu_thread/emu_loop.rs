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
    REWIND_SNAPSHOTS_PER_SECOND, SharedFramebuffer, TcpLinkMode,
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
            } else {
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
                self.start_tcp_link(mode);
            }

            EmuCommand::DisconnectLink => {
                self.disconnect_tcp_link();
            }

            EmuCommand::StepFrames(input) => {
                let input = *input;
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

            EmuCommand::CaptureStateBytes => {
                let resp = match EmuThread::encode_current_state(&self.backend) {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                };
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = self.backend.load_state_from_bytes(state_bytes);
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
                let _ = self.send_resp(EmuResponse::LinkConnected(pending.label));
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
            let _ = self.send_resp(EmuResponse::LinkDisconnected);
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
