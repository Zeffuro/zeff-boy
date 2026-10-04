use crate::{CHANNEL_PROTOCOL, CONTROL_LABEL, INPUT_LABEL};
use js_sys::{Array, ArrayBuffer, Promise, Reflect, Uint8Array};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcDataChannelType, RtcIceGatheringState, RtcPeerConnection, RtcSdpType,
    RtcSessionDescriptionInit,
};
use zeff_netplay_protocol::{IceServer, MAX_CANDIDATES, MAX_PEER_PACKET_BYTES, Signal};

type Callback = Closure<dyn FnMut(JsValue)>;
struct Inner {
    pc: RtcPeerConnection,
    channels: RefCell<[Option<RtcDataChannel>; 2]>,
    packets: RefCell<VecDeque<(u8, Vec<u8>)>>,
    callbacks: RefCell<Vec<Callback>>,
    error: RefCell<Option<String>>,
    relay_allowed: bool,
}

#[wasm_bindgen]
pub struct BrowserPeer {
    inner: Rc<Inner>,
}

#[wasm_bindgen]
impl BrowserPeer {
    #[wasm_bindgen(constructor)]
    pub fn new(ice_json: &str, relay_allowed: bool) -> Result<BrowserPeer, JsValue> {
        if ice_json.len() > 8192 {
            return Err(error("ICE configuration too large"));
        }
        let servers: Vec<IceServer> =
            serde_json::from_str(ice_json).map_err(|_| error("invalid ICE configuration"))?;
        if servers.len() > 8
            || servers.iter().any(|s| {
                s.urls.len() > 4
                    || s.urls.is_empty()
                    || s.urls.iter().any(|u| {
                        u.len() > 256
                            || !(u.starts_with("stun:")
                                || u.starts_with("stuns:")
                                || relay_allowed
                                    && (u.starts_with("turn:") || u.starts_with("turns:")))
                    })
            })
        {
            return Err(error("ICE URLs violate relay policy"));
        }
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
        Ok(Self { inner })
    }

    pub fn create_channels(&self) -> Result<(), JsValue> {
        for (index, label) in [CONTROL_LABEL, INPUT_LABEL].iter().enumerate() {
            let options = RtcDataChannelInit::new();
            options.set_protocol(CHANNEL_PROTOCOL);
            options.set_ordered(index == 0);
            options.set_negotiated(true);
            options.set_id(index as u16);
            if index == 1 {
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
        let offer = JsFuture::from(self.inner.pc.create_offer()).await?;
        let desc: RtcSessionDescriptionInit = offer.unchecked_into();
        JsFuture::from(self.inner.pc.set_local_description(&desc)).await?;
        self.gathered().await
    }

    pub async fn answer(&self) -> Result<String, JsValue> {
        let answer = JsFuture::from(self.inner.pc.create_answer()).await?;
        let desc: RtcSessionDescriptionInit = answer.unchecked_into();
        JsFuture::from(self.inner.pc.set_local_description(&desc)).await?;
        self.gathered().await
    }

    pub async fn apply_remote(&self, sdp: &str, offer: bool) -> Result<(), JsValue> {
        validate_sdp(sdp, self.inner.relay_allowed)?;
        let desc = RtcSessionDescriptionInit::new(if offer {
            RtcSdpType::Offer
        } else {
            RtcSdpType::Answer
        });
        desc.set_sdp(sdp);
        JsFuture::from(self.inner.pc.set_remote_description(&desc)).await?;
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
        if kind > 1 || bytes.is_empty() || bytes.len() > MAX_PEER_PACKET_BYTES {
            return Err(error("invalid peer packet"));
        }
        let channels = self.inner.channels.borrow();
        let channel = channels[kind as usize]
            .as_ref()
            .ok_or_else(|| error("channel unavailable"))?;
        if !self.ready() || channel.buffered_amount() as usize + bytes.len() > 16 * 1024 {
            return Err(error("peer closed or send buffer full"));
        }
        channel.send_with_u8_array(bytes)
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
        let started = js_sys::Date::now();
        while self.inner.pc.ice_gathering_state() != RtcIceGatheringState::Complete {
            if js_sys::Date::now() - started > 15000.0 {
                return Err(error("ICE gathering timed out"));
            }
            JsFuture::from(Promise::new(&mut |resolve, _| {
                let _ = web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10);
            }))
            .await?;
        }
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

fn attach(inner: &Rc<Inner>, channel: RtcDataChannel) -> Result<(), JsValue> {
    let kind = match channel.label().as_str() {
        CONTROL_LABEL => 0,
        INPUT_LABEL => 1,
        _ => return Err(error("unknown channel")),
    };
    if Reflect::get(&channel, &"protocol".into())?
        .as_string()
        .as_deref()
        != Some(CHANNEL_PROTOCOL)
        || Reflect::get(&channel, &"ordered".into())?.as_bool() != Some(kind == 0)
        || Reflect::get(&channel, &"negotiated".into())?.as_bool() != Some(true)
        || channel.id() != Some(kind as u16)
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
    inner.channels.borrow_mut()[kind] = Some(channel);
    inner.callbacks.borrow_mut().extend([receive, closed]);
    Ok(())
}

fn fail(inner: &Inner, reason: &str) {
    if inner.error.borrow().is_none() {
        *inner.error.borrow_mut() = Some(reason.into());
        inner.pc.close();
    }
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

impl Drop for BrowserPeer {
    fn drop(&mut self) {
        self.inner.pc.set_ondatachannel(None);
        for channel in self.inner.channels.borrow().iter().flatten() {
            channel.set_onmessage(None);
            channel.set_onclose(None);
        }
        self.inner.pc.close();
    }
}
