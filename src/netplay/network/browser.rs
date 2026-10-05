use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{Result, bail, ensure};
use zeff_netplay::datagram::InputChannel;
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay::wire::{Admission, Identity, Message, PacketCodec};
use zeff_netplay_connect::browser::BrowserPeer;

use super::metrics::{Metrics, Stats};
use crate::platform::Instant;

pub(crate) enum Event {
    Ready,
    Message(Message),
    Failed(String),
}

struct Pump {
    peer: BrowserPeer,
    admission: Option<Admission>,
    codec: Option<PacketCodec>,
    inputs: InputChannel,
    player: Player,
    pending: VecDeque<Vec<u8>>,
    events: VecDeque<Event>,
    started: Instant,
    heard: Instant,
    retry: Instant,
    closed: bool,
    metrics: Metrics,
}

pub(crate) struct Network(RefCell<Pump>);

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

impl Network {
    pub(crate) fn spawn_transport(
        transport: super::Transport,
        player: Player,
        identity: Identity,
        secret: [u8; 32],
        _: ConnectionScope,
        delay: InputDelay,
    ) -> Result<Self> {
        let super::Transport::Browser(peer) = transport;
        let admission = Admission::new(player, identity, secret)?;
        peer.send_control(admission.hello()).map_err(js_error)?;
        let now = Instant::now();
        Ok(Self(RefCell::new(Pump {
            peer,
            admission: Some(admission),
            codec: None,
            inputs: InputChannel::new(player, delay.frames()),
            player,
            pending: VecDeque::new(),
            events: VecDeque::new(),
            started: now,
            heard: now,
            retry: now,
            closed: false,
            metrics: Metrics::direct(),
        })))
    }

    pub(crate) fn send(&self, message: Message) -> Result<()> {
        let mut pump = self.0.borrow_mut();
        ensure!(!pump.closed, "Peer connection is closed");
        match message {
            Message::Input {
                player,
                frame,
                buttons,
            } => {
                ensure!(player == pump.player, "input player does not own this port");
                pump.inputs.push(frame, buttons)?;
                pump.send_inputs()?;
            }
            message => {
                let bytes = pump
                    .codec
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("admission pending"))?
                    .encode(&message)?;
                pump.peer.send_control(&bytes).map_err(js_error)?;
                pump.metrics.sent(bytes.len());
            }
        }
        Ok(())
    }

    pub(crate) fn poll(&self) -> Result<Option<Event>> {
        let mut pump = self.0.borrow_mut();
        if let Some(event) = pump.events.pop_front() {
            return Ok(Some(event));
        }
        ensure!(!pump.closed, "Peer connection is closed");
        if let Err(error) = pump.tick() {
            pump.peer.close();
            pump.closed = true;
            return Ok(Some(Event::Failed(error.to_string())));
        }
        Ok(pump.events.pop_front())
    }

    pub(crate) fn cancel(&mut self) {
        let pump = self.0.get_mut();
        pump.closed = true;
        pump.peer.close();
        pump.events.clear();
        pump.pending.clear();
    }

    pub(crate) fn stats(&self) -> Stats {
        self.0.borrow().metrics.snapshot(Instant::now())
    }
}

impl Pump {
    fn tick(&mut self) -> Result<()> {
        if self.codec.is_some() {
            ensure!(
                self.heard.elapsed() < Duration::from_secs(3),
                "Peer connection timed out"
            );
        } else {
            ensure!(
                self.started.elapsed() < Duration::from_secs(2),
                "Admission timed out"
            );
        }
        for _ in 0..16 {
            if self.events.len() >= 32 {
                break;
            }
            let Some((kind, bytes)) = self.peer.try_receive().map_err(js_error)? else {
                break;
            };
            self.heard = Instant::now();
            if let Some(admission) = &mut self.admission {
                if kind == 0 {
                    if let Some(reply) = admission.receive(&bytes)? {
                        self.peer.send_control(&reply).map_err(js_error)?;
                    }
                    if admission.complete() {
                        self.codec = Some(self.admission.take().unwrap().into_codec()?);
                        self.events.push_back(Event::Ready);
                    }
                } else {
                    ensure!(
                        kind == 1 && self.pending.len() < 64,
                        "pre-admission input queue overflow"
                    );
                    self.pending.push_back(bytes);
                }
            } else {
                self.packet(kind, bytes)?;
            }
        }
        if self.codec.is_some() {
            while self.events.len() < 32 {
                let Some(bytes) = self.pending.pop_front() else {
                    break;
                };
                self.packet(1, bytes)?;
            }
            if self.retry.elapsed() >= Duration::from_millis(25) {
                self.send_inputs()?;
                self.retry = Instant::now();
            }
            ensure!(
                self.heard.elapsed() < Duration::from_secs(3),
                "Peer connection timed out"
            );
        } else {
            ensure!(
                self.started.elapsed() < Duration::from_secs(2),
                "Admission timed out"
            );
        }
        Ok(())
    }

    fn packet(&mut self, kind: u8, bytes: Vec<u8>) -> Result<()> {
        let codec = self.codec.as_mut().unwrap();
        if kind == 0 {
            let message = codec.decode(&bytes)?;
            ensure!(
                !matches!(message, Message::Input { .. }),
                "input on reliable control channel"
            );
            self.events.push_back(Event::Message(message));
        } else if kind == 1 {
            let messages = self.inputs.receive(codec, &bytes)?;
            self.metrics
                .acknowledge(self.inputs.acknowledged(), Instant::now());
            ensure!(
                self.events.len() + messages.len() <= 64,
                "netplay event queue overflow"
            );
            self.events.extend(messages.into_iter().map(Event::Message));
        } else {
            bail!("Unknown peer channel");
        }
        self.metrics.received(bytes.len());
        Ok(())
    }

    fn send_inputs(&mut self) -> Result<()> {
        if let Some(codec) = &self.codec
            && let Some(packet) = self.inputs.encode(codec)?
        {
            let attempted_at = Instant::now();
            let _sent = self.peer.send_input(&packet).map_err(js_error)?;
            self.metrics.sent(packet.len());
            self.metrics
                .input_attempt(self.inputs.pending_batch(), attempted_at);
        }
        Ok(())
    }
}
