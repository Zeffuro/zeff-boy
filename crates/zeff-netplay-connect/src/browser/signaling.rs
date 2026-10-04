use super::{
    BrowserPeer, Callback, Inner,
    async_util::{Cancellation, Delay},
    error, fail,
};
use futures_util::future::{Either, select};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{MessageEvent, WebSocket};
use zeff_netplay_protocol::{
    ClientMessage, MAX_CANDIDATES, MAX_MESSAGE_BYTES, Role, ServerMessage, Signal, VERSION,
    valid_hex,
};

struct Pending {
    socket: WebSocket,
    queue: RefCell<VecDeque<ServerMessage>>,
    callbacks: RefCell<Vec<Callback>>,
    closed: Cell<bool>,
    cancellation: Cancellation,
    peer: RefCell<Option<Rc<Inner>>>,
}

#[derive(Clone)]
pub struct LobbyCancellation(Rc<Pending>);
impl LobbyCancellation {
    pub fn cancel(&self) {
        close(&self.0);
    }
}

pub struct BrowserLobby {
    pending: Rc<Pending>,
    auth: Option<ClientMessage>,
    welcome: Option<ServerMessage>,
    host: bool,
    expected_room: Option<String>,
}

impl BrowserLobby {
    pub fn new(url: &str, auth: &ClientMessage) -> Result<Self, JsValue> {
        validate_url(url)?;
        let (host, expected_room) = match auth {
            ClientMessage::Create {
                version,
                access_token,
                identity,
            } if *version == VERSION && access_token.len() <= 256 && identity.valid() => {
                (true, None)
            }
            ClientMessage::Join {
                version,
                access_token,
                room,
                identity,
            } if *version == VERSION
                && access_token.len() <= 256
                && identity.valid()
                && valid_hex(room, 24) =>
            {
                (false, Some(room.clone()))
            }
            _ => return Err(error("invalid lobby authentication")),
        };
        let socket = WebSocket::new(url).map_err(|_| error("lobby socket unavailable"))?;
        let pending = Rc::new(Pending {
            socket,
            queue: RefCell::new(VecDeque::new()),
            callbacks: RefCell::new(Vec::new()),
            closed: Cell::new(false),
            cancellation: Default::default(),
            peer: RefCell::new(None),
        });
        let weak = Rc::downgrade(&pending);
        let receive = Closure::wrap(Box::new(move |value: JsValue| {
            let Some(pending) = weak.upgrade() else {
                return;
            };
            if pending.cancellation.cancelled() {
                return;
            }
            let event: MessageEvent = value.unchecked_into();
            let message = event
                .data()
                .as_string()
                .filter(|s| s.len() <= MAX_MESSAGE_BYTES)
                .and_then(|s| serde_json::from_str::<ServerMessage>(&s).ok());
            if let Some(message) = message.filter(|_| pending.queue.borrow().len() < 32) {
                pending.queue.borrow_mut().push_back(message);
            } else {
                close(&pending);
            }
        }) as Box<dyn FnMut(JsValue)>);
        pending
            .socket
            .set_onmessage(Some(receive.as_ref().unchecked_ref()));
        let weak = Rc::downgrade(&pending);
        let closed = Closure::wrap(Box::new(move |_: JsValue| {
            if let Some(pending) = weak.upgrade() {
                pending.closed.set(true);
            }
        }) as Box<dyn FnMut(JsValue)>);
        pending
            .socket
            .set_onclose(Some(closed.as_ref().unchecked_ref()));
        let weak = Rc::downgrade(&pending);
        let failed = Closure::wrap(Box::new(move |_: JsValue| {
            if let Some(pending) = weak.upgrade() {
                close(&pending);
            }
        }) as Box<dyn FnMut(JsValue)>);
        pending
            .socket
            .set_onerror(Some(failed.as_ref().unchecked_ref()));
        pending
            .callbacks
            .borrow_mut()
            .extend([receive, closed, failed]);
        Ok(Self {
            pending,
            auth: Some(auth.clone()),
            welcome: None,
            host,
            expected_room,
        })
    }

    pub fn cancellation(&self) -> LobbyCancellation {
        LobbyCancellation(self.pending.clone())
    }

    pub fn room(&self) -> Option<&str> {
        match &self.welcome {
            Some(ServerMessage::Welcome { room, .. }) => Some(room),
            _ => None,
        }
    }

    pub async fn open(&mut self) -> Result<(), JsValue> {
        let cancellation = self.pending.cancellation.clone();
        let result = bounded(&cancellation, 20_000.0, async {
            if self.auth.is_none() {
                return Err(error("lobby already opened"));
            }
            while self.pending.socket.ready_state() != WebSocket::OPEN {
                if self.pending.closed.get() {
                    return Err(error("lobby socket closed"));
                }
                Delay::new()?.await?;
            }
            send(
                &self.pending,
                &self
                    .auth
                    .take()
                    .ok_or_else(|| error("missing authentication"))?,
            )?;
            let welcome = receive(&self.pending).await?;
            let ServerMessage::Welcome {
                version,
                room,
                role,
                ..
            } = &welcome
            else {
                return Err(error("lobby rejected authentication"));
            };
            if *version != VERSION
                || !valid_hex(room, 24)
                || (*role == Role::Host) != self.host
                || self
                    .expected_room
                    .as_ref()
                    .is_some_and(|expected| expected != room)
            {
                return Err(error("invalid lobby welcome"));
            }
            self.welcome = Some(welcome);
            Ok(())
        })
        .await;
        if result.is_err() {
            close(&self.pending);
        }
        result
    }

    pub async fn establish(mut self) -> Result<BrowserPeer, JsValue> {
        let cancellation = self.pending.cancellation.clone();
        let result = bounded(&cancellation, 120_000.0, self.establish_inner()).await;
        match result {
            Ok(peer) => {
                self.pending.peer.borrow_mut().take();
                close(&self.pending);
                Ok(peer)
            }
            Err(value) => {
                close(&self.pending);
                Err(value)
            }
        }
    }

    async fn establish_inner(&mut self) -> Result<BrowserPeer, JsValue> {
        let ServerMessage::Welcome {
            ice_servers,
            relay_allowed,
            ..
        } = self
            .welcome
            .take()
            .ok_or_else(|| error("open lobby first"))?
        else {
            return Err(error("invalid welcome"));
        };
        let ice =
            serde_json::to_string(&ice_servers).map_err(|_| error("invalid ICE configuration"))?;
        let peer = BrowserPeer::new(&ice, relay_allowed)?;
        *self.pending.peer.borrow_mut() = Some(peer.inner.clone());
        peer.create_channels()?;
        if self.host {
            if receive(&self.pending).await? != ServerMessage::PeerJoined {
                return Err(error("expected peer to join"));
            }
            let sdp = peer.offer().await?;
            send(
                &self.pending,
                &ClientMessage::Signal {
                    signal: Signal::Offer { sdp },
                },
            )?;
        }
        let mut candidates = Vec::new();
        let mut count = 0;
        loop {
            match receive(&self.pending).await? {
                ServerMessage::PeerJoined if !self.host => {}
                ServerMessage::Signal { signal } if signal.valid() => match signal {
                    Signal::Offer { sdp } if !self.host => {
                        peer.apply_remote(&sdp, true).await?;
                        for candidate in candidates.drain(..) {
                            peer.candidate(&candidate).await?;
                        }
                        let sdp = peer.answer().await?;
                        send(
                            &self.pending,
                            &ClientMessage::Signal {
                                signal: Signal::Answer { sdp },
                            },
                        )?;
                        break;
                    }
                    Signal::Answer { sdp } if self.host => {
                        peer.apply_remote(&sdp, false).await?;
                        for candidate in candidates.drain(..) {
                            peer.candidate(&candidate).await?;
                        }
                        break;
                    }
                    signal @ Signal::Candidate { .. } => {
                        candidate_count(&mut count)?;
                        candidates.push(signal);
                    }
                    _ => return Err(error("unexpected session description")),
                },
                _ => return Err(error("negotiation ended before session description")),
            }
        }
        while !peer.ready() {
            if peer.failure().is_some() {
                return Err(error("peer failed before readiness"));
            }
            let message = self.pending.queue.borrow_mut().pop_front();
            if let Some(message) = message {
                apply_candidate(&peer, message, &mut count).await?;
            } else if self.pending.closed.get() {
                return Err(error("lobby socket closed"));
            }
            Delay::new()?.await?;
        }
        send(&self.pending, &ClientMessage::Finish {})?;
        loop {
            let message = receive(&self.pending).await?;
            if message == ServerMessage::Complete {
                break;
            }
            apply_candidate(&peer, message, &mut count).await?;
        }
        if !peer.ready() {
            return Err(error("peer closed before completion"));
        }
        Ok(peer)
    }
}

impl Drop for BrowserLobby {
    fn drop(&mut self) {
        close(&self.pending);
    }
}

fn close(pending: &Pending) {
    pending.cancellation.cancel();
    pending.socket.set_onmessage(None);
    pending.socket.set_onclose(None);
    pending.socket.set_onerror(None);
    let _ = pending.socket.close();
    if let Some(peer) = pending.peer.borrow_mut().take() {
        fail(&peer, "cancelled");
    }
    pending.queue.borrow_mut().clear();
    pending.closed.set(true);
}

fn send(pending: &Pending, message: &ClientMessage) -> Result<(), JsValue> {
    let json = serde_json::to_string(message).map_err(|_| error("invalid lobby message"))?;
    if pending.cancellation.cancelled()
        || pending.socket.ready_state() != WebSocket::OPEN
        || json.len() > MAX_MESSAGE_BYTES
        || (pending.socket.buffered_amount() as usize).saturating_add(json.len())
            > 2 * MAX_MESSAGE_BYTES
    {
        return Err(error("lobby unavailable or send limit exceeded"));
    }
    pending
        .socket
        .send_with_str(&json)
        .map_err(|_| error("lobby send failed"))
}

async fn receive(pending: &Pending) -> Result<ServerMessage, JsValue> {
    loop {
        if let Some(message) = pending.queue.borrow_mut().pop_front() {
            return Ok(message);
        }
        if pending.closed.get() {
            return Err(error("lobby socket closed"));
        }
        Delay::new()?.await?;
    }
}

fn candidate_count(count: &mut usize) -> Result<(), JsValue> {
    *count += 1;
    if *count > MAX_CANDIDATES {
        Err(error("too many remote candidates"))
    } else {
        Ok(())
    }
}

async fn apply_candidate(
    peer: &BrowserPeer,
    message: ServerMessage,
    count: &mut usize,
) -> Result<(), JsValue> {
    match message {
        ServerMessage::Signal {
            signal: signal @ Signal::Candidate { .. },
        } if signal.valid() => {
            candidate_count(count)?;
            peer.candidate(&signal).await
        }
        _ => Err(error("unexpected lobby message")),
    }
}

async fn bounded<T>(
    cancellation: &Cancellation,
    milliseconds: f64,
    future: impl std::future::Future<Output = Result<T, JsValue>>,
) -> Result<T, JsValue> {
    let timeout = async {
        let start = super::async_util::now()?;
        while super::async_util::now()? - start < milliseconds {
            Delay::new()?.await?;
        }
        Err(error("lobby deadline expired"))
    };
    futures_util::pin_mut!(future, timeout);
    cancellation
        .wait(async {
            match select(future, timeout).await {
                Either::Left((result, _)) | Either::Right((result, _)) => result,
            }
        })
        .await
}

fn validate_url(value: &str) -> Result<(), JsValue> {
    if value.len() > 512 {
        return Err(error("invalid lobby address"));
    }
    let url = web_sys::Url::new(value).map_err(|_| error("invalid lobby address"))?;
    let host = url.hostname();
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !(url.protocol() == "wss:" || url.protocol() == "ws:" && loopback)
        || host.is_empty()
        || !url.username().is_empty()
        || !url.password().is_empty()
        || !url.search().is_empty()
        || !url.hash().is_empty()
        || value.contains(['?', '#', '@'])
    {
        return Err(error("use credential-free wss, or ws on loopback"));
    }
    Ok(())
}
