use super::auth;
use crate::browser::{
    BrowserLobby, BrowserPeer,
    async_util::{Delay, now},
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

#[path = "../../../examples/browser-test-lobby/packets.rs"]
mod packets;

#[wasm_bindgen(
    inline_js = "export async function mixedFixtureRequest(url, body) { const response = await fetch(url, body == null ? {} : {method: 'POST', body}); if (!response.ok) throw new Error('mixed fixture HTTP ' + response.status); return await response.text(); }"
)]
extern "C" {
    #[wasm_bindgen(js_name = mixedFixtureRequest)]
    fn request(url: &str, body: Option<&str>) -> js_sys::Promise;
}

fn url() -> &'static str {
    option_env!("ZEFF_BROWSER_TEST_LOBBY_URL").unwrap_or("ws://127.0.0.1:47180/v1/ws")
}

async fn fixture(path: &str, body: Option<&str>) -> String {
    let base = url()
        .strip_suffix("/v1/ws")
        .unwrap()
        .replacen("ws", "http", 1);
    JsFuture::from(request(&format!("{base}/test/mixed/{path}"), body))
        .await
        .unwrap()
        .as_string()
        .unwrap()
}

fn send(peer: &BrowserPeer, round: u8) {
    let packet = packets::payload(round);
    if packets::channel(round) == 0 {
        peer.send_control(&packet).unwrap();
    } else {
        peer.send_input(&packet).unwrap();
    }
}

async fn exchange(peer: &BrowserPeer) {
    let mut omitted = false;
    let mut retries = 0;
    for round in 0..packets::ROUNDS {
        let start = now().unwrap();
        let mut next_send = start + 100.0;
        if !packets::native_sender(round) {
            send(peer, round);
        }
        loop {
            assert!(
                now().unwrap() - start < 3000.0,
                "mixed round {round} stalled"
            );
            if let Some((kind, packet)) = peer.try_receive().unwrap() {
                let current = packets::validate(kind, &packet, round, false).unwrap();
                if current && round == 1 && !omitted {
                    omitted = true;
                    continue;
                }
                if packet[0] == 0xd1 {
                    peer.send_control(&packets::ack(packet[1])).unwrap();
                }
                if current {
                    break;
                }
            } else {
                if !packets::native_sender(round)
                    && packets::channel(round) == 1
                    && now().unwrap() >= next_send
                {
                    send(peer, round);
                    retries += 1;
                    next_send = now().unwrap() + 100.0;
                }
                Delay::new().unwrap().await.unwrap();
            }
        }
    }
    assert!(
        omitted && retries > 0,
        "mixed loss recovery was not exercised"
    );
    peer.send_control(b"mixed-finish").unwrap();
    let start = now().unwrap();
    loop {
        assert!(now().unwrap() - start < 3000.0, "mixed finish stalled");
        if let Some((kind, packet)) = peer.try_receive().unwrap() {
            if kind == 0 && packet == b"mixed-finished" {
                break;
            }
            packets::validate(kind, &packet, packets::ROUNDS - 1, false).unwrap();
        } else {
            Delay::new().unwrap().await.unwrap();
        }
    }
    peer.send_control(b"mixed-close").unwrap();
}

fn close(peer: &BrowserPeer) {
    peer.close();
    assert!(peer.failure().is_some());
    assert!(peer.send_input(&[1]).is_err());
    assert!(peer.send_control(&[1]).is_err());
    assert!(peer.try_receive().is_err());
}

async fn run(native_host: bool) {
    let mut lobby;
    let room;
    if native_host {
        room = fixture("start", Some("")).await;
        lobby = BrowserLobby::new(url(), &auth(Some(room.clone()))).unwrap();
        lobby.open().await.unwrap();
    } else {
        lobby = BrowserLobby::new(url(), &auth(None)).unwrap();
        lobby.open().await.unwrap();
        room = lobby.room().unwrap().to_owned();
        assert_eq!(fixture("start", Some(&room)).await, room);
    }
    let cancellation = lobby.cancellation();
    let peer = lobby.establish().await.unwrap();
    cancellation.cancel();
    assert!(peer.ready());
    exchange(&peer).await;
    let start = now().unwrap();
    loop {
        let status = fixture(&format!("status/{room}"), None).await;
        if status != "running" {
            assert_eq!(status, "passed:64:direct:signaling-closed:loss-recovery");
            break;
        }
        assert!(
            now().unwrap() - start < 3000.0,
            "native close receipt stalled"
        );
        Delay::new().unwrap().await.unwrap();
    }
    close(&peer);
}

#[wasm_bindgen_test(async)]
async fn browser_mixed_native_host_transfers_both_channels_after_signaling() {
    run(true).await;
}

#[wasm_bindgen_test(async)]
async fn browser_mixed_browser_host_transfers_both_channels_after_signaling() {
    run(false).await;
}
