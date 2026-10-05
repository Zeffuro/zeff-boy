use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{net::TcpStream, task::JoinHandle, time::timeout};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use zeff_netplay_lobby::{Lobby, config::Config};
use zeff_netplay_protocol::*;

type Socket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;
const TOKEN: &str = "proof-access-token-with-32-characters";
struct Server {
    url: String,
    lobby: Lobby,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(config: Config) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/v1/ws", listener.local_addr().unwrap());
        let lobby = Lobby::new(config);
        let router = lobby.router();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, lobby, task }
    }
    async fn socket(&self) -> Socket {
        tokio_tungstenite::connect_async(&self.url).await.unwrap().0
    }
}
fn identity() -> SessionIdentity {
    SessionIdentity {
        core: "sega8".into(),
        content_hash: "0".repeat(64),
        compatibility_hash: "1".repeat(64),
        mode: SessionMode::SharedConsole,
    }
}
fn create() -> ClientMessage {
    ClientMessage::Create {
        version: VERSION,
        access_token: TOKEN.into(),
        identity: identity(),
    }
}
fn join(room: &str) -> ClientMessage {
    ClientMessage::Join {
        version: VERSION,
        access_token: TOKEN.into(),
        room: room.into(),
        identity: identity(),
    }
}
async fn send(s: &mut Socket, m: ClientMessage) {
    s.send(Message::Text(serde_json::to_string(&m).unwrap().into()))
        .await
        .unwrap();
}
async fn recv(s: &mut Socket) -> ServerMessage {
    let frame = timeout(Duration::from_secs(3), s.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}
fn welcome(m: ServerMessage, role: Role) -> String {
    let ServerMessage::Welcome {
        version,
        room,
        role: r,
        ice_servers,
        relay_allowed,
    } = m
    else {
        panic!("expected welcome");
    };
    assert_eq!(version, VERSION);
    assert_eq!(r, role);
    assert!(valid_hex(&room, 24));
    assert!(!relay_allowed);
    assert!(
        ice_servers
            .iter()
            .flat_map(|s| &s.urls)
            .all(|u| u.starts_with("stun:"))
    );
    room
}

#[tokio::test]
async fn negotiation_is_finite_generic_and_releases_room_before_gameplay() {
    let server = Server::start(Config::local(TOKEN).unwrap()).await;
    let mut host = server.socket().await;
    send(&mut host, create()).await;
    let room = welcome(recv(&mut host).await, Role::Host);
    let mut guest = server.socket().await;
    send(&mut guest, join(&room)).await;
    welcome(recv(&mut guest).await, Role::Guest);
    assert_eq!(recv(&mut host).await, ServerMessage::PeerJoined);
    let offer = Signal::Offer {
        sdp: "v=0\r\nsynthetic-offer".into(),
    };
    send(
        &mut host,
        ClientMessage::Signal {
            signal: offer.clone(),
        },
    )
    .await;
    assert_eq!(
        recv(&mut guest).await,
        ServerMessage::Signal { signal: offer }
    );
    let answer = Signal::Answer {
        sdp: "v=0\r\nsynthetic-answer".into(),
    };
    send(
        &mut guest,
        ClientMessage::Signal {
            signal: answer.clone(),
        },
    )
    .await;
    assert_eq!(
        recv(&mut host).await,
        ServerMessage::Signal { signal: answer }
    );
    send(&mut host, ClientMessage::Finish {}).await;
    send(&mut guest, ClientMessage::Finish {}).await;
    assert_eq!(recv(&mut host).await, ServerMessage::Complete);
    assert_eq!(recv(&mut guest).await, ServerMessage::Complete);
    assert_eq!(server.lobby.room_count(), 0);
    let mut stranger = server.socket().await;
    send(&mut stranger, join(&room)).await;
    assert_eq!(
        recv(&mut stranger).await,
        ServerMessage::Error {
            code: ErrorCode::Unavailable
        }
    );
}

#[tokio::test]
async fn authentication_version_compatibility_and_capacity_are_enforced() {
    let mut config = Config::local(TOKEN).unwrap();
    config.max_rooms = 1;
    let server = Server::start(config).await;
    for (token, version, code) in [
        ("", VERSION, ErrorCode::Unauthorized),
        ("wrong", VERSION, ErrorCode::Unauthorized),
        (TOKEN, VERSION + 1, ErrorCode::Version),
    ] {
        let mut s = server.socket().await;
        send(
            &mut s,
            ClientMessage::Create {
                version,
                access_token: token.into(),
                identity: identity(),
            },
        )
        .await;
        assert_eq!(recv(&mut s).await, ServerMessage::Error { code });
    }
    let mut host = server.socket().await;
    send(&mut host, create()).await;
    let room = welcome(recv(&mut host).await, Role::Host);
    let mut extra = server.socket().await;
    send(&mut extra, create()).await;
    assert_eq!(
        recv(&mut extra).await,
        ServerMessage::Error {
            code: ErrorCode::Full
        }
    );
    let mut bad = server.socket().await;
    let mut mismatched = identity();
    mismatched.core = "nes".into();
    send(
        &mut bad,
        ClientMessage::Join {
            version: VERSION,
            access_token: TOKEN.into(),
            room: room.clone(),
            identity: mismatched,
        },
    )
    .await;
    assert_eq!(
        recv(&mut bad).await,
        ServerMessage::Error {
            code: ErrorCode::Incompatible
        }
    );
    let mut guest = server.socket().await;
    send(&mut guest, join(&room)).await;
    welcome(recv(&mut guest).await, Role::Guest);
    assert_eq!(recv(&mut host).await, ServerMessage::PeerJoined);
    let mut third = server.socket().await;
    send(&mut third, join(&room)).await;
    assert_eq!(
        recv(&mut third).await,
        ServerMessage::Error {
            code: ErrorCode::Full
        }
    );
    host.close(None).await.unwrap();
    assert_eq!(recv(&mut guest).await, ServerMessage::PeerLeft);
    assert_eq!(server.lobby.room_count(), 0);
}

#[tokio::test]
async fn public_room_capacity_and_disconnect_cleanup_are_bounded() {
    let mut config = Config::public();
    config.max_rooms = 4;
    let server = Server::start(config).await;
    let public_create = || ClientMessage::Create {
        version: VERSION,
        access_token: String::new(),
        identity: identity(),
    };
    let mut hosts = Vec::new();
    let mut codes = Vec::new();
    for _ in 0..4 {
        let mut host = server.socket().await;
        send(&mut host, public_create()).await;
        codes.push(welcome(recv(&mut host).await, Role::Host));
        hosts.push(host);
    }
    assert_eq!(server.lobby.room_count(), 4);
    let mut overflow = server.socket().await;
    send(&mut overflow, public_create()).await;
    assert_eq!(
        recv(&mut overflow).await,
        ServerMessage::Error {
            code: ErrorCode::Full
        }
    );
    let mut guest = server.socket().await;
    send(
        &mut guest,
        ClientMessage::Join {
            version: VERSION,
            access_token: String::new(),
            identity: identity(),
            room: codes[0].clone(),
        },
    )
    .await;
    welcome(recv(&mut guest).await, Role::Guest);
    assert_eq!(recv(&mut hosts[0]).await, ServerMessage::PeerJoined);
    hosts[0].close(None).await.unwrap();
    assert_eq!(recv(&mut guest).await, ServerMessage::PeerLeft);
    assert_eq!(server.lobby.room_count(), 3);
    let mut replacement = server.socket().await;
    send(&mut replacement, public_create()).await;
    welcome(recv(&mut replacement).await, Role::Host);
    assert_eq!(server.lobby.room_count(), 4);
}

#[tokio::test]
async fn public_admission_rate_is_capped_before_room_creation() {
    let mut config = Config::public();
    config.max_connections = 256;
    let server = Server::start(config).await;
    for _ in 0..120 {
        server.socket().await.close(None).await.unwrap();
    }
    let error = tokio_tungstenite::connect_async(&server.url)
        .await
        .unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected HTTP admission rejection");
    };
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(server.lobby.room_count(), 0);
}

#[tokio::test]
async fn no_binary_or_arbitrary_gameplay_forwarding() {
    for config in [Config::local(TOKEN).unwrap(), Config::public()] {
        let server = Server::start(config).await;
        for frame in [
            Message::Binary(vec![0; 32].into()),
            Message::Text(r#"{"type":"input","payload":"gameplay"}"#.into()),
        ] {
            let mut socket = server.socket().await;
            socket.send(frame).await.unwrap();
            assert_eq!(
                recv(&mut socket).await,
                ServerMessage::Error {
                    code: ErrorCode::Invalid
                }
            );
        }
        let mut host = server.socket().await;
        send(&mut host, create()).await;
        welcome(recv(&mut host).await, Role::Host);
        send(&mut host, ClientMessage::Finish {}).await;
        assert_eq!(
            recv(&mut host).await,
            ServerMessage::Error {
                code: ErrorCode::Unavailable
            }
        );
    }
}

#[tokio::test]
async fn origins_slots_and_room_deadline_are_bounded() {
    let mut config = Config::public();
    config.max_connections = 1;
    config.room_ttl = Duration::from_millis(50);
    config.origins = vec!["https://example.org".into()];
    let server = Server::start(config).await;
    let mut request = server.url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
    let mut host = server.socket().await;
    assert!(tokio_tungstenite::connect_async(&server.url).await.is_err());
    send(&mut host, create()).await;
    welcome(recv(&mut host).await, Role::Host);
    assert_eq!(
        recv(&mut host).await,
        ServerMessage::Error {
            code: ErrorCode::Expired
        }
    );
    assert_eq!(server.lobby.room_count(), 0);
}

#[tokio::test]
async fn oversized_frame_is_closed_before_parsing() {
    let server = Server::start(Config::local(TOKEN).unwrap()).await;
    let mut socket = server.socket().await;
    socket
        .send(Message::Text("x".repeat(MAX_MESSAGE_BYTES + 1).into()))
        .await
        .unwrap();
    let result = timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap();
    assert!(!matches!(result, Some(Ok(Message::Text(_)))));
    assert_eq!(server.lobby.room_count(), 0);
}

#[tokio::test]
async fn heartbeat_preserves_membership_and_cannot_carry_gameplay() {
    let server = Server::start(Config::public()).await;
    let mut host = server.socket().await;
    send(&mut host, create()).await;
    welcome(recv(&mut host).await, Role::Host);
    host.send(Message::Ping(b"heartbeat".to_vec().into()))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), host.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(b"heartbeat".to_vec().into())
    );
    assert_eq!(server.lobby.room_count(), 1);
    for _ in 0..22 {
        let _ = host.send(Message::Pong(vec![].into())).await;
    }
    assert_eq!(
        recv(&mut host).await,
        ServerMessage::Error {
            code: ErrorCode::RateLimited
        }
    );
}

#[tokio::test]
async fn incomplete_http_expires_but_authenticated_websocket_survives() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use zeff_netplay_lobby::listener::{BoundedListener, ConnectionGate};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let lobby = Lobby::new(Config::local(TOKEN).unwrap());
    let bounded = BoundedListener::new(listener, 1, Duration::from_secs(30));
    let router = lobby
        .router()
        .into_make_service_with_connect_info::<ConnectionGate>();
    let task = tokio::spawn(async move {
        axum::serve(bounded, router).await.unwrap();
    });
    let _server = Server {
        url: format!("ws://{address}/v1/ws"),
        lobby,
        task,
    };
    let mut incomplete = TcpStream::connect(address).await.unwrap();
    incomplete
        .write_all(b"GET /health HTTP/1.1\r\n")
        .await
        .unwrap();
    let mut pending = TcpStream::connect(address).await.unwrap();
    pending
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buffer = vec![0; 1024];
    assert!(
        timeout(Duration::from_millis(50), pending.read(&mut buffer))
            .await
            .is_err()
    );
    let received = timeout(Duration::from_secs(6), pending.read(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buffer[..received]).contains("200 OK"));
    drop(pending);
    drop(incomplete);
    let mut host = _server.socket().await;
    send(&mut host, create()).await;
    welcome(recv(&mut host).await, Role::Host);
    tokio::time::sleep(Duration::from_millis(5100)).await;
    host.send(Message::Ping(vec![1].into())).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), host.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(vec![1].into())
    );
    assert_eq!(_server.lobby.room_count(), 1);
}
