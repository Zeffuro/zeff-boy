use crate::{CHANNEL_PROTOCOL, CONTROL_CHANNEL_ID, CONTROL_LABEL, INPUT_CHANNEL_ID, INPUT_LABEL};
use js_sys::{Array, ArrayBuffer, Reflect, Uint8Array};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcDataChannelType, RtcIceGatheringState, RtcPeerConnection, RtcSdpType,
    RtcSessionDescriptionInit,
};
use zeff_netplay_protocol::{
    IceServer, MAX_CANDIDATES, MAX_ICE_CONFIG_BYTES, MAX_PEER_PACKET_BYTES, Signal,
    validate_ice_servers,
};

mod async_util;
mod signaling;
pub use signaling::{BrowserLobby, LobbyCancellation};

#[cfg(all(test, feature = "browser-tests"))]
mod tests;

type Callback = Closure<dyn FnMut(JsValue)>;
struct Inner {
    pc: RtcPeerConnection,
    channels: RefCell<[Option<RtcDataChannel>; 2]>,
    packets: RefCell<VecDeque<(u8, Vec<u8>)>>,
    callbacks: RefCell<Vec<Callback>>,
    error: RefCell<Option<String>>,
    relay_allowed: bool,
    cancellation: async_util::Cancellation,
}

#[wasm_bindgen]
pub struct BrowserPeer {
    inner: Rc<Inner>,
}

#[wasm_bindgen]
impl BrowserPeer {
    #[wasm_bindgen(constructor)]
    pub fn new(ice_json: &str, relay_allowed: bool) -> Result<BrowserPeer, JsValue> {
        if ice_json.len() > MAX_ICE_CONFIG_BYTES {
            return Err(error("ICE configuration too large"));
        }
        let servers: Vec<IceServer> =
            serde_json::from_str(ice_json).map_err(|_| error("invalid ICE configuration"))?;
        validate_ice_servers(&servers, relay_allowed)
            .map_err(|reason| error(&reason.to_string()))?;
        let ice = Array::new();
        for server in servers {
            let object = js_sys::Object::new();
            let urls: Array = server.urls.iter().map(|u| JsValue::from_str(u)).collect();
            Reflect::set(&object, &"urls".into(), &urls)?;
            if let Some(username) = server.username {
                Reflect::set(&object, &"username".into(), &username.into())?;
            }
            if let Some(credential) = server.credential {
                Reflect::set(&object, &"credential".into(), &credential.into())?;
            }
            ice.push(&object);
        }
        let config = RtcConfiguration::new();
        config.set_ice_servers(&ice);
        let pc = RtcPeerConnection::new_with_configuration(&config)?;
        let inner = Rc::new(Inner {
            pc,
            channels: RefCell::new([None, None]),
            packets: RefCell::new(VecDeque::new()),
            callbacks: RefCell::new(Vec::new()),
            error: RefCell::new(None),
            relay_allowed,
            cancellation: Default::default(),
        });
        let weak = Rc::downgrade(&inner);
        let callback = Closure::wrap(Box::new(move |value: JsValue| {
            if let Some(inner) = weak.upgrade() {
                let event: RtcDataChannelEvent = value.unchecked_into();
                if attach(&inner, event.channel()).is_err() {
                    fail(&inner, "invalid data channel");
                }
            }
        }) as Box<dyn FnMut(JsValue)>);
        inner
            .pc
            .set_ondatachannel(Some(callback.as_ref().unchecked_ref()));
        inner.callbacks.borrow_mut().push(callback);
        let weak = Rc::downgrade(&inner);
        let state_changed = Closure::wrap(Box::new(move |_: JsValue| {
            if let Some(inner) = weak.upgrade()
                && matches!(
                    inner.pc.connection_state(),
                    web_sys::RtcPeerConnectionState::Failed
                        | web_sys::RtcPeerConnectionState::Closed
                )
            {
                fail(&inner, "peer connection failed");
            }
        }) as Box<dyn FnMut(JsValue)>);
        inner
            .pc
            .set_onconnectionstatechange(Some(state_changed.as_ref().unchecked_ref()));
        inner.callbacks.borrow_mut().push(state_changed);
        Ok(Self { inner })
    }

    pub fn create_channels(&self) -> Result<(), JsValue> {
        self.ensure_alive()?;
        if self.inner.channels.borrow().iter().any(Option::is_some) {
            return Err(error("channels already created"));
        }
        for (id, label) in [
            (CONTROL_CHANNEL_ID, CONTROL_LABEL),
            (INPUT_CHANNEL_ID, INPUT_LABEL),
        ] {
            let options = RtcDataChannelInit::new();
            options.set_protocol(CHANNEL_PROTOCOL);
            options.set_ordered(id == CONTROL_CHANNEL_ID);
            options.set_negotiated(true);
            options.set_id(id);
            if id == INPUT_CHANNEL_ID {
                options.set_max_retransmits(0);
            }
            let channel = self
                .inner
                .pc
                .create_data_channel_with_data_channel_dict(label, &options);
            attach(&self.inner, channel)?;
        }
        Ok(())
    }

    pub async fn offer(&self) -> Result<String, JsValue> {
        self.ensure_alive()?;
        let offer = self.promise(self.inner.pc.create_offer()).await?;
        let desc: RtcSessionDescriptionInit = offer.unchecked_into();
        self.promise(self.inner.pc.set_local_description(&desc))
            .await?;
        self.gathered().await
    }

    pub async fn answer(&self) -> Result<String, JsValue> {
        self.ensure_alive()?;
        let answer = self.promise(self.inner.pc.create_answer()).await?;
        let desc: RtcSessionDescriptionInit = answer.unchecked_into();
        self.promise(self.inner.pc.set_local_description(&desc))
            .await?;
        self.gathered().await
    }

    pub async fn apply_remote(&self, sdp: &str, offer: bool) -> Result<(), JsValue> {
        self.ensure_alive()?;
        validate_sdp(sdp, self.inner.relay_allowed)?;
        let desc = RtcSessionDescriptionInit::new(if offer {
            RtcSdpType::Offer
        } else {
            RtcSdpType::Answer
        });
        desc.set_sdp(sdp);
        self.promise(self.inner.pc.set_remote_description(&desc))
            .await?;
        Ok(())
    }

    pub fn ready(&self) -> bool {
        self.inner.error.borrow().is_none()
            && self.inner.channels.borrow().iter().all(|c| {
                c.as_ref()
                    .is_some_and(|c| c.ready_state() == RtcDataChannelState::Open)
            })
    }

    pub fn send(&self, kind: u8, bytes: &[u8]) -> Result<(), JsValue> {
        match kind {
            0 => self.send_control(bytes),
            1 if self.send_input(bytes)? => Ok(()),
            1 => Err(error("input send buffer full")),
            _ => Err(error("invalid peer packet")),
        }
    }

    pub fn take_packet(&self) -> Option<Vec<u8>> {
        self.inner
            .packets
            .borrow_mut()
            .pop_front()
            .map(|(kind, bytes)| {
                let mut packet = Vec::with_capacity(bytes.len() + 1);
                packet.push(kind);
                packet.extend(bytes);
                packet
            })
    }

    pub fn failure(&self) -> Option<String> {
        self.inner.error.borrow().clone()
    }
    pub fn close(&self) {
        fail(&self.inner, "closed");
    }

    async fn gathered(&self) -> Result<String, JsValue> {
        let started = async_util::now()?;
        self.ensure_alive()?;
        while self.inner.pc.ice_gathering_state() != RtcIceGatheringState::Complete {
            self.ensure_alive()?;
            if async_util::now()? - started > 15000.0 {
                return Err(error("ICE gathering timed out"));
            }
            self.inner
                .cancellation
                .wait(async_util::Delay::new()?)
                .await?;
        }
        self.ensure_alive()?;
        let sdp = self
            .inner
            .pc
            .local_description()
            .ok_or_else(|| error("missing description"))?
            .sdp();
        validate_sdp(&sdp, self.inner.relay_allowed)?;
        Ok(sdp)
    }
}

impl BrowserPeer {
    pub fn try_receive(&self) -> Result<Option<(u8, Vec<u8>)>, JsValue> {
        self.ensure_alive()?;
        Ok(self.inner.packets.borrow_mut().pop_front())
    }

    pub fn send_control(&self, bytes: &[u8]) -> Result<(), JsValue> {
        if self.send_packet(0, bytes)? {
            Ok(())
        } else {
            Err(error("control send buffer full"))
        }
    }

    pub fn send_input(&self, bytes: &[u8]) -> Result<bool, JsValue> {
        self.send_packet(1, bytes)
    }

    fn send_packet(&self, kind: usize, bytes: &[u8]) -> Result<bool, JsValue> {
        self.ensure_alive()?;
        if bytes.is_empty() || bytes.len() > MAX_PEER_PACKET_BYTES {
            return Err(error("invalid peer packet"));
        }
        let channel = self.inner.channels.borrow()[kind]
            .clone()
            .ok_or_else(|| error("channel unavailable"))?;
        if !self.ready() {
            return Err(error("peer not ready"));
        }
        if (channel.buffered_amount() as usize).saturating_add(bytes.len()) > 16 * 1024 {
            return Ok(false);
        }
        match channel.send_with_u8_array(bytes) {
            Ok(()) => Ok(true),
            Err(value)
                if Reflect::get(&value, &"name".into())
                    .ok()
                    .and_then(|name| name.as_string())
                    .as_deref()
                    == Some("OperationError")
                    && self.ready() =>
            {
                Ok(false)
            }
            Err(_) => {
                fail(&self.inner, "peer send failed");
                Err(error("peer send failed"))
            }
        }
    }

    fn ensure_alive(&self) -> Result<(), JsValue> {
        if self.inner.error.borrow().is_some() {
            Err(error("peer closed"))
        } else {
            Ok(())
        }
    }

    async fn promise(&self, promise: js_sys::Promise) -> Result<JsValue, JsValue> {
        self.inner
            .cancellation
            .wait(async {
                JsFuture::from(promise)
                    .await
                    .map_err(|_| error("RTC operation failed"))
            })
            .await
    }

    async fn candidate(&self, signal: &Signal) -> Result<(), JsValue> {
        self.ensure_alive()?;
        let Signal::Candidate {
            candidate,
            sdp_mid,
            sdp_m_line_index,
        } = signal
        else {
            return Err(error("expected ICE candidate"));
        };
        validate_candidate(candidate, self.inner.relay_allowed)?;
        let init = web_sys::RtcIceCandidateInit::new(candidate);
        init.set_sdp_mid(sdp_mid.as_deref());
        init.set_sdp_m_line_index(*sdp_m_line_index);
        self.promise(
            self.inner
                .pc
                .add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&init)),
        )
        .await?;
        Ok(())
    }
}

fn attach(inner: &Rc<Inner>, channel: RtcDataChannel) -> Result<(), JsValue> {
    let (kind, id) = match channel.label().as_str() {
        CONTROL_LABEL => (0, CONTROL_CHANNEL_ID),
        INPUT_LABEL => (1, INPUT_CHANNEL_ID),
        _ => return Err(error("unknown channel")),
    };
    if Reflect::get(&channel, &"protocol".into())?
        .as_string()
        .as_deref()
        != Some(CHANNEL_PROTOCOL)
        || Reflect::get(&channel, &"ordered".into())?.as_bool() != Some(kind == 0)
        || Reflect::get(&channel, &"negotiated".into())?.as_bool() != Some(true)
        || channel.id() != Some(id)
        || channel.max_retransmits() != if kind == 0 { None } else { Some(0) }
        || channel.max_packet_life_time().is_some()
        || inner.channels.borrow()[kind].is_some()
    {
        return Err(error("channel contract mismatch"));
    }
    channel.set_binary_type(RtcDataChannelType::Arraybuffer);
    let weak = Rc::downgrade(inner);
    let receive = Closure::wrap(Box::new(move |value: JsValue| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if inner.error.borrow().is_some() {
            return;
        }
        let event: MessageEvent = value.unchecked_into();
        let data = event.data();
        if !data.is_instance_of::<ArrayBuffer>() {
            fail(&inner, "non-binary packet");
            return;
        }
        let array = Uint8Array::new(&data);
        let length = array.length() as usize;
        if length == 0 || length > MAX_PEER_PACKET_BYTES || inner.packets.borrow().len() >= 64 {
            fail(&inner, "receive limit exceeded");
            return;
        }
        inner
            .packets
            .borrow_mut()
            .push_back((kind as u8, array.to_vec()));
    }) as Box<dyn FnMut(JsValue)>);
    channel.set_onmessage(Some(receive.as_ref().unchecked_ref()));
    let weak = Rc::downgrade(inner);
    let closed = Closure::wrap(Box::new(move |_: JsValue| {
        if let Some(inner) = weak.upgrade() {
            fail(&inner, "channel closed");
        }
    }) as Box<dyn FnMut(JsValue)>);
    channel.set_onclose(Some(closed.as_ref().unchecked_ref()));
    channel.set_onerror(Some(closed.as_ref().unchecked_ref()));
    inner.channels.borrow_mut()[kind] = Some(channel);
    inner.callbacks.borrow_mut().extend([receive, closed]);
    Ok(())
}

fn fail(inner: &Inner, reason: &str) {
    if inner.error.borrow().is_none() {
        *inner.error.borrow_mut() = Some(reason.into());
    }
    inner.cancellation.cancel();
    inner.pc.set_ondatachannel(None);
    inner.pc.set_onconnectionstatechange(None);
    for channel in inner.channels.borrow().iter().flatten() {
        channel.set_onmessage(None);
        channel.set_onclose(None);
        channel.set_onerror(None);
        channel.close();
    }
    inner.packets.borrow_mut().clear();
    inner.pc.close();
}

fn error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}
fn validate_sdp(sdp: &str, relay_allowed: bool) -> Result<(), JsValue> {
    if !(Signal::Offer { sdp: sdp.into() }).valid() {
        return Err(error("invalid SDP size"));
    }
    let mut media = 0;
    let mut candidates = 0;
    for line in sdp.lines() {
        if line.starts_with("m=") {
            media += 1;
            if !line.starts_with("m=application ") {
                return Err(error("data channels only"));
            }
        }
        if let Some(candidate) = line.strip_prefix("a=candidate:") {
            candidates += 1;
            let fields: Vec<_> = candidate.split_ascii_whitespace().collect();
            if candidates > MAX_CANDIDATES
                || fields.get(6) != Some(&"typ")
                || !fields.get(7).is_some_and(|t| {
                    matches!(*t, "host" | "srflx" | "prflx") || relay_allowed && *t == "relay"
                })
            {
                return Err(error("ICE candidate violates relay policy"));
            }
        }
    }
    if media != 1 {
        return Err(error("expected one data channel media section"));
    }
    Ok(())
}

fn validate_candidate(candidate: &str, relay_allowed: bool) -> Result<(), JsValue> {
    if candidate.is_empty() {
        return Ok(());
    }
    let fields: Vec<_> = candidate.split_ascii_whitespace().collect();
    if fields.get(6) != Some(&"typ")
        || !fields.get(7).is_some_and(|t| {
            matches!(*t, "host" | "srflx" | "prflx") || relay_allowed && *t == "relay"
        })
    {
        return Err(error("ICE candidate violates relay policy"));
    }
    Ok(())
}

impl Drop for BrowserPeer {
    fn drop(&mut self) {
        self.close();
    }
}
