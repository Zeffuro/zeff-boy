use anyhow::Result;
use zeff_netplay::wire::PacketCodec;
use zeff_netplay_connect::browser::BrowserPeer;

pub(super) enum Peer {
    Browser(BrowserPeer),
    #[cfg(test)]
    Paired(super::paired::Endpoint),
}

fn js_error(error: wasm_bindgen::JsValue) -> anyhow::Error {
    use wasm_bindgen::JsCast;
    anyhow::anyhow!(
        error
            .as_string()
            .or_else(|| error
                .dyn_ref::<js_sys::Error>()
                .and_then(|error| error.message().as_string()))
            .unwrap_or_else(|| "Peer connection failed".into())
    )
}

impl Peer {
    pub(super) fn send_control(&self, packet: &[u8]) -> Result<()> {
        match self {
            Self::Browser(peer) => peer.send_control(packet).map_err(js_error),
            #[cfg(test)]
            Self::Paired(peer) => peer.send(0, packet).map(|_| ()),
        }
    }
    pub(super) fn send_input(&self, packet: &[u8]) -> Result<bool> {
        match self {
            Self::Browser(peer) => peer.send_input(packet).map_err(js_error),
            #[cfg(test)]
            Self::Paired(peer) => peer.send(1, packet),
        }
    }
    pub(super) fn try_receive(&self) -> Result<Option<(u8, Vec<u8>)>> {
        match self {
            Self::Browser(peer) => peer.try_receive().map_err(js_error),
            #[cfg(test)]
            Self::Paired(peer) => peer.receive(),
        }
    }
    pub(super) fn close(&self) {
        match self {
            Self::Browser(peer) => peer.close(),
            #[cfg(test)]
            Self::Paired(peer) => peer.close(),
        }
    }
    pub(super) fn finish(self, codec: PacketCodec, frame: u64) {
        match self {
            Self::Browser(peer) => super::closing::start(peer, codec, frame),
            #[cfg(test)]
            Self::Paired(peer) => {
                let mut codec = codec;
                if let Ok(packet) = codec.encode(&zeff_netplay::wire::Message::Close { frame }) {
                    let _ = peer.send(0, &packet);
                }
                peer.close();
            }
        }
    }
}
