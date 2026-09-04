use std::sync::mpsc::{self, Receiver as StdReceiver, TryRecvError};
use std::thread;

use crate::emu_thread::{EmuResponse, TcpLinkMode};
use crate::link::transport::TcpLinkTransport;
use crate::link::{
    LinkConnectionState, LinkEndpointId, LinkSession, LinkSystemType, RemoteLink,
    remote_link_system_for_active_system,
};

use super::EmuLoop;

pub(super) struct PendingTcpLink {
    label: String,
    endpoint: LinkEndpointId,
    system: LinkSystemType,
    receiver: StdReceiver<Result<TcpLinkTransport, String>>,
}

impl EmuLoop {
    pub(super) fn start_tcp_link(&mut self, mode: TcpLinkMode) {
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

    pub(super) fn poll_tcp_link_connection(&mut self) {
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

    pub(super) fn clear_disconnected_tcp_link(&mut self) {
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

    pub(super) fn disconnect_tcp_link(&mut self) {
        self.pending_tcp_link = None;
        if let Some(mut link) = self.tcp_link.take() {
            link.disconnect();
        }
        self.backend.set_link_peer_present(false);
    }
}
