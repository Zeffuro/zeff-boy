use super::{BrowserLobby, BrowserPeer, async_util::Delay};
use futures_util::future::{Either, select};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;
use zeff_netplay_protocol::{ClientMessage, SessionIdentity, SessionMode, VERSION};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn browser_ice_config_uses_shared_bounds_before_creating_a_peer() {
    use zeff_netplay_protocol::{IceServer, MAX_ICE_CREDENTIAL_BYTES, MAX_ICE_URLS};
    let mut server = IceServer {
        urls: vec!["stun:example.com:3478".into(); MAX_ICE_URLS],
        username: None,
        credential: Some("p".repeat(MAX_ICE_CREDENTIAL_BYTES)),
    };
    let json = |server: &IceServer| serde_json::to_string(&[server]).unwrap();
    let peer = BrowserPeer::new(&json(&server), false).unwrap();
    peer.close();
    server.urls.push("stun:other.example".into());
    assert!(BrowserPeer::new(&json(&server), false).is_err());
    server.urls.pop();
    server.credential.as_mut().unwrap().push('p');
    assert!(BrowserPeer::new(&json(&server), false).is_err());
    server.credential = None;
    for url in ["turn:example.com", "stun:", "stun:host\n"] {
        server.urls = vec![url.into()];
        assert!(BrowserPeer::new(&json(&server), false).is_err());
    }
}

fn auth(room: Option<String>) -> ClientMessage {
    let identity = SessionIdentity {
        core: "nes".into(),
        content_hash: "a".repeat(64),
        compatibility_hash: "b".repeat(64),
        mode: SessionMode::SharedConsole,
    };
    match room {
        Some(room) => ClientMessage::Join {
            version: VERSION,
            access_token: String::new(),
            room,
            identity,
        },
        None => ClientMessage::Create {
            version: VERSION,
            access_token: String::new(),
            identity,
        },
    }
}

fn lobby() -> BrowserLobby {
    BrowserLobby::new(
        option_env!("ZEFF_BROWSER_TEST_LOBBY_URL").unwrap_or("ws://127.0.0.1:47180/v1/ws"),
        &auth(None),
    )
    .unwrap()
}

async fn pair() -> (BrowserPeer, BrowserPeer) {
    let mut host = lobby();
    let cancellation = host.cancellation();
    host.open().await.unwrap();
    let url = option_env!("ZEFF_BROWSER_TEST_LOBBY_URL").unwrap_or("ws://127.0.0.1:47180/v1/ws");
    let mut guest = BrowserLobby::new(url, &auth(Some(host.room().unwrap().into()))).unwrap();
    guest.open().await.unwrap();
    let (host, guest) = futures_util::future::join(host.establish(), guest.establish()).await;
    let pair = (host.unwrap(), guest.unwrap());
    cancellation.cancel();
    assert!(pair.0.ready() && pair.1.ready());
    pair
}

async fn until(mut predicate: impl FnMut() -> bool) {
    let start = super::async_util::now().unwrap();
    while !predicate() {
        assert!(super::async_util::now().unwrap() - start < 5_000.0);
        Delay::new().unwrap().await.unwrap();
    }
}

#[wasm_bindgen_test(async)]
async fn browser_lobby_delivers_both_channels_and_terminal_errors() {
    let (host, guest) = pair().await;
    assert!(host.try_receive().unwrap().is_none());
    host.send_control(&[1, 2, 3]).unwrap();
    assert!(host.send_input(&[4, 5]).unwrap());
    guest.send_control(&[6, 7, 8]).unwrap();
    assert!(guest.send_input(&[9, 10]).unwrap());
    for (peer, expected) in [
        (&guest, vec![(0, vec![1, 2, 3]), (1, vec![4, 5])]),
        (&host, vec![(0, vec![6, 7, 8]), (1, vec![9, 10])]),
    ] {
        let mut received = Vec::new();
        until(|| {
            while let Some(packet) = peer.try_receive().unwrap() {
                received.push(packet);
            }
            received.len() == 2
        })
        .await;
        received.sort_by_key(|packet| packet.0);
        assert_eq!(received, expected);
    }
    assert!(host.send_input(&vec![0; 1025]).is_err());
    assert!(host.send_control(&[]).is_err());
    host.close();
    assert!(host.send_input(&[1]).is_err());
    assert!(host.send_control(&[1]).is_err());
    assert!(host.try_receive().is_err());
    until(|| guest.failure().is_some()).await;
    assert!(guest.send_input(&[1]).is_err());
}

#[wasm_bindgen_test(async)]
async fn browser_lobby_receive_queue_and_send_buffer_are_bounded() {
    let (host, guest) = pair().await;
    let packet = [42; 1024];
    let mut dropped = false;
    for _ in 0..128 {
        if !host.send_input(&packet).unwrap() {
            dropped = true;
            break;
        }
    }
    assert!(dropped);
    assert!(host.failure().is_none());
    assert!(host.ready());
    until(|| {
        host.inner.channels.borrow()[1]
            .as_ref()
            .unwrap()
            .buffered_amount()
            == 0
    })
    .await;
    while guest.try_receive().unwrap().is_some() {}
    for _ in 0..65 {
        host.send_control(&[42]).unwrap();
    }
    until(|| guest.failure().is_some()).await;
    assert_eq!(guest.failure().as_deref(), Some("receive limit exceeded"));
    assert!(guest.inner.packets.borrow().is_empty());
    assert!(guest.inner.pc.ondatachannel().is_none());
    assert!(
        guest
            .inner
            .channels
            .borrow()
            .iter()
            .flatten()
            .all(|channel| {
                channel.onmessage().is_none()
                    && channel.onclose().is_none()
                    && channel.onerror().is_none()
            })
    );
}

#[wasm_bindgen_test(async)]
async fn browser_lobby_cancel_waiting_peer_closes_synchronously() {
    let mut lobby = lobby();
    lobby.open().await.unwrap();
    let cancellation = lobby.cancellation();
    let connection = lobby.establish();
    let cancel = async {
        Delay::new().unwrap().await.unwrap();
        cancellation.cancel();
    };
    futures_util::pin_mut!(connection, cancel);
    let start = super::async_util::now().unwrap();
    let result = match select(connection, cancel).await {
        Either::Left((result, _)) => result,
        Either::Right(((), connection)) => connection.await,
    };
    assert!(result.is_err());
    assert!(super::async_util::now().unwrap() - start < 500.0);
}

#[wasm_bindgen_test(async)]
async fn browser_peer_cancel_pending_operation_is_terminal() {
    let peer = BrowserPeer::new("[]", false).unwrap();
    peer.create_channels().unwrap();
    let pending = js_sys::Promise::new(&mut |_, _| {});
    let operation = peer.promise(pending);
    let close = async {
        Delay::new().unwrap().await.unwrap();
        peer.close();
        assert!(peer.inner.pc.ondatachannel().is_none());
    };
    let (result, ()) = futures_util::future::join(operation, close).await;
    assert!(result.is_err());
    assert!(peer.try_receive().is_err());
    assert!(peer.offer().await.is_err());
    assert!(peer.answer().await.is_err());
}

#[wasm_bindgen_test]
fn browser_lobby_rejects_unsafe_addresses_without_echoing_secrets() {
    for url in [
        "ws://example.com/v1/ws",
        "wss://user:private-key@example.com/v1/ws",
        "wss://example.com/v1/ws?key=private-key",
        "wss://example.com/v1/ws#private-key",
    ] {
        let value: JsValue = BrowserLobby::new(url, &auth(None)).err().unwrap();
        let message = js_sys::Error::from(value).message().as_string().unwrap();
        assert!(!message.contains("private-key"));
    }
    assert!(BrowserPeer::new(r#"[{"urls":["turn:example.com:3478"]}]"#, false).is_err());
}
