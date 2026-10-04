use super::*;
use crate::netplay::network::tests::{event, identity, input};
use crate::netplay::test_lobby::TestLobby;
use crate::netplay::{DirectPeer, Transport};
use zeff_netplay_connect::{LobbyConnection, protocol::*};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn session_identity() -> SessionIdentity {
    SessionIdentity {
        core: "nes".into(),
        content_hash: "0".repeat(64),
        compatibility_hash: "1".repeat(64),
        mode: SessionMode::SharedConsole,
    }
}

fn direct_pair(server: &TestLobby) -> (DirectPeer, DirectPeer) {
    let host_url = server.url.clone();
    let guest_url = server.url.clone();
    let (ready, room) = std::sync::mpsc::channel();
    let host = thread::spawn(move || {
        let runtime = runtime();
        let connection = runtime.block_on(async {
            let request = ClientMessage::Create {
                version: VERSION,
                access_token: String::new(),
                identity: session_identity(),
            };
            let lobby = LobbyConnection::open(&host_url, &request).await.unwrap();
            let ServerMessage::Welcome { room, .. } = &lobby.welcome else {
                unreachable!()
            };
            ready.send(room.clone()).unwrap();
            tokio::time::timeout(Duration::from_secs(15), lobby.establish())
                .await
                .unwrap()
                .unwrap()
        });
        DirectPeer {
            connection,
            runtime,
        }
    });
    let guest = thread::spawn(move || {
        let runtime = runtime();
        let connection = runtime.block_on(async {
            let request = ClientMessage::Join {
                version: VERSION,
                access_token: String::new(),
                identity: session_identity(),
                room: room.recv_timeout(Duration::from_secs(3)).unwrap(),
            };
            let lobby = LobbyConnection::open(&guest_url, &request).await.unwrap();
            tokio::time::timeout(Duration::from_secs(15), lobby.establish())
                .await
                .unwrap()
                .unwrap()
        });
        DirectPeer {
            connection,
            runtime,
        }
    });
    (host.join().unwrap(), guest.join().unwrap())
}

#[test]
fn actual_rtc_runtime_handoff_authenticates_and_exchanges_both_channels() {
    let server = TestLobby::start();
    let (host, guest) = direct_pair(&server);
    assert_eq!(server.lobby.room_count(), 0);
    let mut one = Network::spawn_transport(
        Transport::Direct(Box::new(host)),
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
        InputDelay::default(),
    )
    .unwrap();
    let mut two = Network::spawn_transport(
        Transport::Direct(Box::new(guest)),
        Player::Two,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
        InputDelay::default(),
    )
    .unwrap();
    assert!(matches!(event(&one), Event::Ready));
    assert!(matches!(event(&two), Event::Ready));
    for frame in 2..10 {
        one.send(input(Player::One, frame)).unwrap();
        two.send(input(Player::Two, frame)).unwrap();
        assert!(
            matches!(event(&one), Event::Message(message) if message == input(Player::Two, frame))
        );
        assert!(
            matches!(event(&two), Event::Message(message) if message == input(Player::One, frame))
        );
    }
    one.send(Message::Chat {
        text: "direct".into(),
    })
    .unwrap();
    assert!(matches!(event(&two), Event::Message(Message::Chat { text }) if text == "direct"));
    let start = Instant::now();
    one.cancel();
    two.cancel();
    assert!(start.elapsed() < Duration::from_secs(1));
}
