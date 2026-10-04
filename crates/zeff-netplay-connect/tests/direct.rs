#![cfg(all(feature = "native", not(target_arch = "wasm32")))]

use std::time::Duration;
use tokio::time::timeout;
use zeff_netplay_connect::{LobbyConnection, PacketKind, protocol::*};
use zeff_netplay_lobby::{Lobby, config::Config};

#[tokio::test]
async fn both_channels_continue_after_lobby_server_stops() {
    timeout(Duration::from_secs(30), async {
        let token = "direct-proof-token-at-least-32-characters";
        let mut config = Config::local(token).unwrap();
        config.ice_servers.clear();
        let lobby = Lobby::new(config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/v1/ws", listener.local_addr().unwrap());
        let router = lobby.router();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let identity = SessionIdentity {
            core: "generic-test-core".into(),
            content_hash: "0".repeat(64),
            compatibility_hash: "1".repeat(64),
            mode: SessionMode::SharedConsole,
        };
        let host = LobbyConnection::open(
            &url,
            &ClientMessage::Create {
                version: VERSION,
                access_token: token.into(),
                identity: identity.clone(),
            },
        )
        .await
        .unwrap();
        let guest = LobbyConnection::open(
            &url,
            &ClientMessage::Join {
                version: VERSION,
                access_token: token.into(),
                identity,
                room: host.room().into(),
            },
        )
        .await
        .unwrap();
        let (host, guest) = tokio::join!(host.establish(), guest.establish());
        let (mut host, mut guest) = (host.unwrap(), guest.unwrap());
        assert_eq!(lobby.room_count(), 0);
        server.abort();
        let _ = server.await;
        for peer in [&host, &guest] {
            assert!(!peer.connection_path().await.unwrap().relay);
        }
        for sequence in 0..16u8 {
            let payload = vec![sequence; MAX_PEER_PACKET_BYTES];
            host.send_control(&payload).await.unwrap();
            host.try_send_input(&payload).await.unwrap();
            let mut kinds = Vec::new();
            for _ in 0..2 {
                let packet = guest.receive(Duration::from_secs(3)).await.unwrap();
                assert_eq!(packet.bytes, payload);
                kinds.push(packet.kind);
                match packet.kind {
                    PacketKind::Control => guest.send_control(&packet.bytes).await.unwrap(),
                    PacketKind::Input => guest.try_send_input(&packet.bytes).await.unwrap(),
                }
            }
            assert!(kinds.contains(&PacketKind::Control) && kinds.contains(&PacketKind::Input));
            for _ in 0..2 {
                assert_eq!(
                    host.receive(Duration::from_secs(3)).await.unwrap().bytes,
                    payload
                );
            }
        }
        assert!(host.try_send_input(&[]).await.is_err());
        assert!(
            host.send_control(&vec![0; MAX_PEER_PACKET_BYTES + 1])
                .await
                .is_err()
        );
        host.close().await.unwrap();
        guest.close().await.unwrap();
    })
    .await
    .unwrap();
}
