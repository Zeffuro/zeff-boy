use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use rtc::peer_connection::configuration::setting_engine::SctpMaxMessageSize;
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;
use tokio::time::timeout;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState,
    SettingEngine, SettingEngineBuilder,
};
use zeff_netplay_protocol::MAX_PEER_PACKET_BYTES;

use super::{Packet, PacketKind};
use crate::{CHANNEL_PROTOCOL, CONTROL_LABEL, INPUT_LABEL};

type Ready = (PacketKind, Arc<dyn DataChannel>);
type Tasks = Arc<Mutex<Vec<AbortHandle>>>;
type HandlerSetup = (
    Arc<Handler>,
    mpsc::Receiver<Ready>,
    mpsc::Receiver<Packet>,
    watch::Receiver<Option<&'static str>>,
    watch::Receiver<bool>,
);

#[derive(Clone)]
pub(super) struct Handler {
    ready: mpsc::Sender<Ready>,
    incoming: mpsc::Sender<Packet>,
    status: watch::Sender<Option<&'static str>>,
    gathering: watch::Sender<bool>,
    claims: Arc<AtomicU8>,
    arrivals: Arc<AtomicUsize>,
    peer: Arc<OnceLock<Weak<dyn PeerConnection>>>,
    pub tasks: Tasks,
}

impl Handler {
    pub fn new() -> HandlerSetup {
        let (ready, ready_rx) = mpsc::channel(2);
        let (incoming, incoming_rx) = mpsc::channel(64);
        let (status, status_rx) = watch::channel(None);
        let (gathering, gathering_rx) = watch::channel(false);
        (
            Arc::new(Self {
                ready,
                incoming,
                status,
                gathering,
                claims: Arc::default(),
                arrivals: Arc::default(),
                peer: Arc::default(),
                tasks: Arc::default(),
            }),
            ready_rx,
            incoming_rx,
            status_rx,
            gathering_rx,
        )
    }

    pub fn bind(&self, peer: &Arc<dyn PeerConnection>) {
        let _ = self.peer.set(Arc::downgrade(peer));
    }

    fn fail(&self, reason: &'static str) {
        self.status.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(reason);
            true
        });
    }

    async fn close_peer(&self) {
        if let Some(peer) = self.peer.get().and_then(Weak::upgrade) {
            let _ = timeout(Duration::from_secs(2), peer.close()).await;
        }
    }

    pub fn attach(&self, channel: Arc<dyn DataChannel>) {
        let arrival = self.arrivals.fetch_add(1, Ordering::Relaxed);
        if arrival >= 2 {
            self.fail("unexpected additional data channel");
            if arrival == 2 {
                let handler = self.clone();
                let task = tokio::spawn(async move { handler.close_peer().await });
                self.tasks.lock().unwrap().push(task.abort_handle());
            }
            return;
        }
        let handler = self.clone();
        let task = tokio::spawn(async move {
            if let Err(reason) = handler.poll(channel.clone()).await {
                handler.fail(reason);
                let _ = timeout(Duration::from_secs(2), channel.close()).await;
            }
            handler.close_peer().await;
        });
        self.tasks.lock().unwrap().push(task.abort_handle());
    }

    async fn poll(&self, channel: Arc<dyn DataChannel>) -> Result<(), &'static str> {
        let kind = match channel
            .label()
            .await
            .map_err(|_| "channel label query failed")?
            .as_str()
        {
            CONTROL_LABEL => PacketKind::Control,
            INPUT_LABEL => PacketKind::Input,
            _ => return Err("unknown data channel label"),
        };
        if channel
            .protocol()
            .await
            .map_err(|_| "channel protocol query failed")?
            != CHANNEL_PROTOCOL
        {
            return Err("wrong data channel protocol");
        }
        if !channel
            .negotiated()
            .await
            .map_err(|_| "channel negotiation query failed")?
        {
            return Err("data channel must use an explicit negotiated ID");
        }
        let ordered = channel
            .ordered()
            .await
            .map_err(|_| "channel ordering query failed")?;
        let retries = channel
            .max_retransmits()
            .await
            .map_err(|_| "channel reliability query failed")?;
        if channel
            .max_packet_life_time()
            .await
            .map_err(|_| "channel lifetime query failed")?
            .is_some()
        {
            return Err("unexpected channel lifetime");
        }
        match kind {
            PacketKind::Control if !ordered || retries.is_some() => {
                return Err("control channel must be reliable and ordered");
            }
            PacketKind::Input if ordered || retries != Some(0) => {
                return Err("input channel must be unordered without retransmissions");
            }
            _ => {}
        }
        let bit = match kind {
            PacketKind::Control => 1,
            PacketKind::Input => 2,
        };
        if self.claims.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
            return Err("duplicate data channel");
        }
        let mut opened = false;
        while let Some(event) = channel.poll().await {
            match event {
                DataChannelEvent::OnOpen => {
                    if opened {
                        return Err("duplicate channel open");
                    }
                    opened = true;
                    self.ready
                        .try_send((kind, channel.clone()))
                        .map_err(|_| "channel readiness queue unavailable")?;
                }
                DataChannelEvent::OnMessage(message) => {
                    if !opened {
                        return Err("peer packet arrived before channel open");
                    }
                    if message.is_string {
                        return Err("text peer packet is forbidden");
                    }
                    if message.data.is_empty() || message.data.len() > MAX_PEER_PACKET_BYTES {
                        return Err("peer packet size exceeded contract");
                    }
                    timeout(
                        Duration::from_millis(500),
                        self.incoming.send(Packet {
                            kind,
                            bytes: message.data.to_vec(),
                        }),
                    )
                    .await
                    .map_err(|_| "peer receive queue remained full")?
                    .map_err(|_| "peer packet consumer closed")?;
                }
                DataChannelEvent::OnClose => break,
                DataChannelEvent::OnError => return Err("data channel reported an error"),
                _ => {}
            }
        }
        self.fail("data channel closed");
        Ok(())
    }
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        self.attach(channel);
    }

    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.gathering.send_replace(true);
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(
            state,
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed
        ) {
            self.fail(if state == RTCPeerConnectionState::Failed {
                "peer connection failed"
            } else {
                "peer connection closed"
            });
        }
    }
}

pub(super) fn settings() -> SettingEngine {
    SettingEngineBuilder::new()
        .with_sctp_max_message_size(SctpMaxMessageSize::Bounded(MAX_PEER_PACKET_BYTES as u32))
        .with_sctp_max_receive_buffer_size(64 * 1024)
        .build()
}

pub(super) struct PeerOwner {
    pub rtc: Arc<dyn PeerConnection>,
    tasks: Tasks,
}

impl PeerOwner {
    pub fn new(rtc: Arc<dyn PeerConnection>, tasks: Tasks) -> Self {
        Self { rtc, tasks }
    }
}

impl Drop for PeerOwner {
    fn drop(&mut self) {
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let rtc = self.rtc.clone();
            runtime.spawn(async move {
                let _ = timeout(Duration::from_secs(2), rtc.close()).await;
            });
        }
    }
}
