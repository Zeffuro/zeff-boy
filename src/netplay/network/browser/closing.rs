use std::cell::Cell;
use std::time::Duration;

use zeff_netplay::wire::{Message, PacketCodec};
use zeff_netplay_connect::browser::BrowserPeer;

use crate::platform::Instant;

thread_local! {
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
}

struct Slot;

pub(super) fn active() -> usize {
    ACTIVE.with(Cell::get)
}

impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(active.get() - 1));
    }
}

pub(super) fn start(peer: BrowserPeer, mut codec: PacketCodec, frame: u64) {
    let slot = ACTIVE.with(|active| {
        (active.get() < 4).then(|| {
            active.set(active.get() + 1);
            Slot
        })
    });
    let Some(slot) = slot else {
        peer.close();
        return;
    };
    let Ok(packet) = codec.encode(&Message::Close { frame }) else {
        peer.close();
        return;
    };
    if peer.send_control(&packet).is_err() {
        peer.close();
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let _slot = slot;
        let start = Instant::now();
        let mut decoding = true;
        while start.elapsed() < Duration::from_millis(250) {
            match peer.try_receive() {
                Ok(Some((0, bytes))) if decoding => match codec.decode(&bytes) {
                    Ok(Message::Close { .. }) => break,
                    _ => decoding = false,
                },
                Ok(Some(_)) => {}
                Ok(None) => {
                    if wait().await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        peer.close();
    });
}

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function waitNetplayClose() { return new Promise(resolve => setTimeout(resolve, 5)); }"
)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = waitNetplayClose)]
    fn wait_close() -> js_sys::Promise;
}

async fn wait() -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    wasm_bindgen_futures::JsFuture::from(wait_close()).await
}
