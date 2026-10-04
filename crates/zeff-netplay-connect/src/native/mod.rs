use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use webrtc::data_channel::{DataChannel, RTCDataChannelInit};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, RTCConfigurationBuilder, RTCIceCandidateInit,
    RTCIceCandidateType, RTCIceServer, RTCSessionDescription,
};
use zeff_netplay_protocol::{
    ClientMessage, MAX_CANDIDATES, MAX_MESSAGE_BYTES, MAX_PEER_PACKET_BYTES, Role, ServerMessage,
    Signal, VERSION, valid_hex,
};

use crate::{CHANNEL_PROTOCOL, CONTROL_CHANNEL_ID, CONTROL_LABEL, INPUT_CHANNEL_ID, INPUT_LABEL};

mod channels;
use channels::{Handler, PeerOwner};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const ESTABLISH_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    Control,
    Input,
}

#[derive(Debug)]
pub struct Packet {
    pub kind: PacketKind,
    pub bytes: Vec<u8>,
}

pub struct LobbyConnection {
    pub welcome: ServerMessage,
    socket: Socket,
}

impl LobbyConnection {
    pub async fn open(url: &str, auth: &ClientMessage) -> Result<Self> {
        ensure!(
            matches!(
                auth,
                ClientMessage::Create { .. } | ClientMessage::Join { .. }
            ),
            "first command must authenticate"
        );
        let request = url.into_client_request()?;
        ensure!(
            matches!(request.uri().scheme_str(), Some("ws" | "wss")),
            "lobby URL must use ws or wss"
        );
        if request.uri().scheme_str() == Some("ws") {
            let host = request.uri().host().context("missing lobby host")?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            ensure!(
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback()),
                "plaintext ws is permitted only on loopback; use wss"
            );
        }
        ensure!(
            request.uri().query().is_none() && !url.contains('@') && !url.contains('#'),
            "lobby URL must not contain credentials, query or fragment"
        );
        let config = WebSocketConfig::default()
            .read_buffer_size(4096)
            .write_buffer_size(4096)
            .max_write_buffer_size(2 * MAX_MESSAGE_BYTES)
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let (mut socket, _) = timeout(
            Duration::from_secs(10),
            connect_async_with_config(request, Some(config), false),
        )
        .await??;
        send(&mut socket, auth).await?;
        let welcome = timeout(Duration::from_secs(10), receive(&mut socket)).await??;
        let ServerMessage::Welcome {
            version,
            room,
            role,
            ..
        } = &welcome
        else {
            bail!("lobby rejected authentication");
        };
        ensure!(
            *version == VERSION && valid_hex(room, 24),
            "invalid lobby welcome"
        );
        ensure!(
            matches!(
                (auth, role),
                (ClientMessage::Create { .. }, Role::Host)
                    | (ClientMessage::Join { .. }, Role::Guest)
            ),
            "unexpected lobby role"
        );
        Ok(Self { welcome, socket })
    }

    pub fn room(&self) -> &str {
        match &self.welcome {
            ServerMessage::Welcome { room, .. } => room,
            _ => unreachable!("welcome is validated at construction"),
        }
    }

    pub async fn establish(mut self) -> Result<DataConnection> {
        let deadline = Instant::now() + ESTABLISH_TIMEOUT;
        let ServerMessage::Welcome {
            role,
            ice_servers,
            relay_allowed,
            ..
        } = &self.welcome
        else {
            bail!("invalid welcome");
        };
        let role = role.clone();
        let relay_allowed = *relay_allowed;
        let ice = ice_config(ice_servers, relay_allowed)?;
        if role == Role::Host {
            ensure!(
                timeout_at(deadline, receive(&mut self.socket)).await??
                    == ServerMessage::PeerJoined,
                "expected peer to join"
            );
        }
        let (handler, mut ready, incoming, mut status, mut gathering) = Handler::new();
        let rtc = Arc::new(
            timeout_at(
                deadline,
                PeerConnectionBuilder::new()
                    .with_configuration(
                        RTCConfigurationBuilder::new().with_ice_servers(ice).build(),
                    )
                    .with_handler(handler.clone())
                    .with_data_channel_send_buffer_limit(16 * 1024)
                    .with_setting_engine(channels::settings())
                    .with_udp_addrs(vec!["0.0.0.0:0".to_owned()])
                    .build(),
            )
            .await??,
        ) as Arc<dyn PeerConnection>;
        let owner = PeerOwner::new(rtc.clone(), handler.tasks.clone());
        handler.bind(&rtc);
        timeout_at(deadline, async {
            // Explicit IDs avoid rtc 0.21 losing reliability metadata when accepting DCEP.
            for (label, options) in [(CONTROL_LABEL, RTCDataChannelInit {
                    protocol: CHANNEL_PROTOCOL.into(), negotiated: Some(CONTROL_CHANNEL_ID), ..Default::default()
                }), (INPUT_LABEL, RTCDataChannelInit {
                    ordered: false, max_retransmits: Some(0), protocol: CHANNEL_PROTOCOL.into(),
                    negotiated: Some(INPUT_CHANNEL_ID),
                    ..Default::default()
                })] {
                handler.attach(rtc.create_data_channel(label, Some(options)).await?);
            }
            if role == Role::Host {
                let offer = rtc.create_offer(None).await?;
                rtc.set_local_description(offer).await?;
                let sdp = gathered(&rtc, &mut gathering, relay_allowed).await?;
                send(&mut self.socket, &ClientMessage::Signal { signal: Signal::Offer { sdp } }).await?;
            }
            let mut pending = Vec::new();
            let mut candidate_count = 0;
            loop {
                match receive(&mut self.socket).await? {
                    ServerMessage::Signal { signal } => {
                        validate_signal(&signal, relay_allowed)?;
                        match signal {
                            Signal::Offer { sdp } if role == Role::Guest => {
                                rtc.set_remote_description(RTCSessionDescription::offer(sdp)?).await?;
                                for candidate in pending.drain(..) { rtc.add_ice_candidate(candidate).await?; }
                                let answer = rtc.create_answer(None).await?;
                                rtc.set_local_description(answer).await?;
                                let sdp = gathered(&rtc, &mut gathering, relay_allowed).await?;
                                send(&mut self.socket, &ClientMessage::Signal { signal: Signal::Answer { sdp } }).await?;
                                break;
                            }
                            Signal::Answer { sdp } if role == Role::Host => {
                                rtc.set_remote_description(RTCSessionDescription::answer(sdp)?).await?;
                                for candidate in pending.drain(..) { rtc.add_ice_candidate(candidate).await?; }
                                break;
                            }
                            candidate @ Signal::Candidate { .. } => {
                                candidate_count += 1;
                                ensure!(candidate_count <= MAX_CANDIDATES, "too many remote candidates");
                                pending.push(candidate_init(candidate));
                            }
                            _ => bail!("unexpected session description"),
                        }
                    }
                    ServerMessage::PeerJoined if role == Role::Guest => {}
                    _ => bail!("negotiation ended before session description"),
                }
            }
            let mut control = None;
            let mut input = None;
            while control.is_none() || input.is_none() {
                if let Some(reason) = *status.borrow() {
                    bail!("peer failed before channel readiness: {reason}");
                }
                tokio::select! {
                    message = receive(&mut self.socket) => {
                        apply_candidate(&rtc, message?, relay_allowed, &mut candidate_count).await?;
                    }
                    channel = ready.recv() => {
                        let (kind, channel) = channel.context("channel readiness ended")?;
                        match kind { PacketKind::Control => control = Some(channel), PacketKind::Input => input = Some(channel) }
                    }
                    changed = status.changed() => {
                        changed?;
                        bail!("peer failed: {}", status.borrow().unwrap_or("closed"));
                    }
                }
            }
            send(&mut self.socket, &ClientMessage::Finish {}).await?;
            loop {
                match receive(&mut self.socket).await? {
                    ServerMessage::Complete => break,
                    message => apply_candidate(&rtc, message, relay_allowed, &mut candidate_count).await?,
                }
            }
            let _ = timeout(SEND_TIMEOUT, self.socket.close(None)).await;
            Ok::<_, anyhow::Error>((control.unwrap(), input.unwrap()))
        }).await?
        .map(|(control, input)| DataConnection { owner, control, input, incoming, status })
    }
}

pub struct DataConnection {
    owner: PeerOwner,
    control: Arc<dyn DataChannel>,
    input: Arc<dyn DataChannel>,
    incoming: mpsc::Receiver<Packet>,
    status: tokio::sync::watch::Receiver<Option<&'static str>>,
}

#[derive(Debug)]
pub struct ConnectionPath {
    pub local_type: String,
    pub remote_type: String,
    pub protocol: String,
    pub relay: bool,
}

impl DataConnection {
    pub async fn connection_path(&self) -> Result<ConnectionPath> {
        let pair = timeout(SEND_TIMEOUT, async {
            self.owner
                .rtc
                .sctp()
                .await
                .context("SCTP transport unavailable")?
                .transport()
                .ice_transport()
                .get_selected_candidate_pair()
                .await?
                .context("selected candidate pair unavailable")
        })
        .await??;
        Ok(ConnectionPath {
            local_type: pair.local().typ.to_string(),
            remote_type: pair.remote().typ.to_string(),
            protocol: pair.local().protocol.to_string(),
            relay: pair.local().typ == RTCIceCandidateType::Relay
                || pair.remote().typ == RTCIceCandidateType::Relay,
        })
    }

    pub async fn send_control(&self, bytes: &[u8]) -> Result<()> {
        self.check_packet(bytes)?;
        timeout(
            SEND_TIMEOUT,
            self.control.send(bytes::BytesMut::from(bytes)),
        )
        .await??;
        Ok(())
    }

    pub async fn try_send_input(&self, bytes: &[u8]) -> Result<()> {
        self.check_packet(bytes)?;
        self.input.try_send(bytes::BytesMut::from(bytes)).await?;
        Ok(())
    }

    pub async fn receive(&mut self, wait: Duration) -> Result<Packet> {
        if let Some(reason) = *self.status.borrow() {
            bail!("peer failed: {reason}");
        }
        tokio::select! {
            packet = timeout(wait, self.incoming.recv()) => packet?.context("peer channel closed"),
            changed = self.status.changed() => {
                changed?;
                bail!("peer failed: {}", self.status.borrow().unwrap_or("closed"));
            }
        }
    }

    pub async fn close(self) -> Result<()> {
        timeout(SEND_TIMEOUT, self.owner.rtc.close()).await??;
        Ok(())
    }

    fn check_packet(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_PEER_PACKET_BYTES,
            "invalid peer packet size"
        );
        if let Some(reason) = *self.status.borrow() {
            bail!("peer failed: {reason}");
        }
        Ok(())
    }
}

async fn send(socket: &mut Socket, message: &ClientMessage) -> Result<()> {
    let text = serde_json::to_string(message)?;
    ensure!(
        text.len() <= MAX_MESSAGE_BYTES,
        "signaling message exceeds limit"
    );
    timeout(SEND_TIMEOUT, socket.send(Message::Text(text.into()))).await??;
    Ok(())
}

async fn receive(socket: &mut Socket) -> Result<ServerMessage> {
    loop {
        match socket.next().await.context("lobby connection closed")?? {
            Message::Text(text) => {
                ensure!(
                    text.len() <= MAX_MESSAGE_BYTES,
                    "signaling message exceeds limit"
                );
                let event = serde_json::from_str(&text)?;
                match event {
                    ServerMessage::Error { code } => bail!("lobby error: {code:?}"),
                    ServerMessage::PeerLeft => bail!("peer left lobby before signaling completed"),
                    _ => return Ok(event),
                }
            }
            Message::Ping(_) | Message::Pong(_) => {
                timeout(SEND_TIMEOUT, socket.flush()).await??;
            }
            _ => bail!("unexpected lobby frame"),
        }
    }
}

fn ice_config(
    servers: &[zeff_netplay_protocol::IceServer],
    relay_allowed: bool,
) -> Result<Vec<RTCIceServer>> {
    ensure!(servers.len() <= 8, "too many ICE servers");
    servers
        .iter()
        .map(|server| {
            ensure!(
                !server.urls.is_empty() && server.urls.len() <= 8,
                "invalid ICE URLs"
            );
            for url in &server.urls {
                ensure!(url.len() <= 256, "ICE URL too long");
                let scheme = url.split_once(':').map(|v| v.0).unwrap_or_default();
                ensure!(
                    matches!(scheme, "stun" | "stuns" | "turn" | "turns"),
                    "unsupported ICE URL"
                );
                ensure!(
                    relay_allowed || matches!(scheme, "stun" | "stuns"),
                    "TURN forbidden by lobby policy"
                );
            }
            ensure!(
                server.username.as_ref().is_none_or(|v| v.len() <= 1024)
                    && server.credential.as_ref().is_none_or(|v| v.len() <= 1024),
                "ICE credentials exceed limit"
            );
            Ok(RTCIceServer {
                urls: server.urls.clone(),
                username: server.username.clone().unwrap_or_default(),
                credential: server.credential.clone().unwrap_or_default(),
            })
        })
        .collect()
}

async fn gathered(
    rtc: &Arc<dyn PeerConnection>,
    gathering: &mut tokio::sync::watch::Receiver<bool>,
    relay_allowed: bool,
) -> Result<String> {
    timeout(Duration::from_secs(15), async {
        while !*gathering.borrow_and_update() {
            gathering.changed().await?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let sdp = rtc
        .local_description()
        .await
        .context("missing local description")?
        .sdp;
    validate_sdp(&sdp, relay_allowed)?;
    Ok(sdp)
}

fn validate_signal(signal: &Signal, relay_allowed: bool) -> Result<()> {
    ensure!(signal.valid(), "invalid signaling payload");
    match signal {
        Signal::Offer { sdp } | Signal::Answer { sdp } => validate_sdp(sdp, relay_allowed),
        Signal::Candidate { candidate, .. } => validate_candidate(candidate, relay_allowed),
    }
}

fn validate_sdp(sdp: &str, relay_allowed: bool) -> Result<()> {
    ensure!(
        Signal::Offer { sdp: sdp.into() }.valid(),
        "invalid SDP size"
    );
    let mut candidates = 0;
    let mut media = 0;
    for line in sdp.lines() {
        if let Some(candidate) = line.strip_prefix("a=candidate:") {
            candidates += 1;
            ensure!(candidates <= MAX_CANDIDATES, "too many SDP candidates");
            validate_candidate(candidate, relay_allowed)?;
        }
        if line.starts_with("m=") {
            media += 1;
            ensure!(
                line.starts_with("m=application "),
                "only data channels are supported"
            );
        }
    }
    ensure!(media == 1, "expected one data-channel media section");
    Ok(())
}

fn validate_candidate(candidate: &str, relay_allowed: bool) -> Result<()> {
    if candidate.is_empty() {
        return Ok(());
    }
    let mut fields = candidate.split_ascii_whitespace();
    ensure!(fields.nth(6) == Some("typ"), "invalid ICE candidate");
    let kind = fields.next().context("missing candidate type")?;
    ensure!(
        matches!(kind, "host" | "srflx" | "prflx" | "relay"),
        "unsupported candidate type"
    );
    ensure!(
        relay_allowed || kind != "relay",
        "relay candidate forbidden by lobby policy"
    );
    Ok(())
}

fn candidate_init(signal: Signal) -> RTCIceCandidateInit {
    let Signal::Candidate {
        candidate,
        sdp_mid,
        sdp_m_line_index,
    } = signal
    else {
        unreachable!()
    };
    RTCIceCandidateInit {
        candidate,
        sdp_mid,
        sdp_mline_index: sdp_m_line_index,
        ..Default::default()
    }
}

async fn apply_candidate(
    rtc: &Arc<dyn PeerConnection>,
    message: ServerMessage,
    relay_allowed: bool,
    count: &mut usize,
) -> Result<()> {
    let ServerMessage::Signal {
        signal: signal @ Signal::Candidate { .. },
    } = message
    else {
        bail!("unexpected signaling event");
    };
    validate_signal(&signal, relay_allowed)?;
    *count += 1;
    ensure!(*count <= MAX_CANDIDATES, "too many remote candidates");
    rtc.add_ice_candidate(candidate_init(signal)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_policy_checks_embedded_and_trickled_candidates() {
        let host = "candidate:1 1 udp 123 127.0.0.1 1234 typ host";
        let relay = "candidate:1 1 udp 123 192.0.2.1 1234 typ relay raddr 127.0.0.1 rport 1234";
        assert!(validate_candidate(host, false).is_ok());
        assert!(validate_candidate(relay, false).is_err());
        assert!(validate_candidate(relay, true).is_ok());
        let sdp =
            format!("v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na={relay}\r\n");
        assert!(validate_sdp(&sdp, false).is_err());
        assert!(validate_sdp(&sdp, true).is_ok());
        let turn = zeff_netplay_protocol::IceServer {
            urls: vec!["turn:example.com:3478".into()],
            username: Some("user".into()),
            credential: Some("secret".into()),
        };
        assert!(ice_config(std::slice::from_ref(&turn), false).is_err());
        assert!(ice_config(&[turn], true).is_ok());
    }
}
