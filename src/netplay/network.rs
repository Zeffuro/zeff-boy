use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use crossbeam_channel::{Receiver as EventReceiver, Sender as EventSender, TrySendError};
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay::wire::{self, Identity, Message};

const OUTBOUND_CAPACITY: usize = 64;
const INBOUND_CAPACITY: usize = 64;
const IDLE_POLL: Duration = Duration::from_millis(10);

mod failure;
mod rtc;

#[derive(Debug)]
pub(crate) enum Event {
    Ready,
    Message(Message),
    Failed(String),
}

pub(crate) struct Network {
    outbound: Option<EventSender<Message>>,
    inbound: EventReceiver<Event>,
    socket: Option<Arc<TcpStream>>,
    cancelled: Arc<AtomicBool>,
    owner_cancelled: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl Network {
    pub(crate) fn spawn_transport(
        transport: super::Transport,
        player: Player,
        identity: Identity,
        secret: [u8; 32],
        scope: ConnectionScope,
        delay: InputDelay,
    ) -> Result<Self> {
        match transport {
            super::Transport::Tcp(stream) => Self::spawn(stream, player, identity, secret, scope),
            super::Transport::Direct(peer) => rtc::spawn(*peer, player, identity, secret, delay),
        }
    }

    pub(crate) fn spawn(
        stream: TcpStream,
        player: Player,
        identity: Identity,
        secret: [u8; 32],
        scope: ConnectionScope,
    ) -> Result<Self> {
        let socket = Arc::new(
            stream
                .try_clone()
                .context("cloning netplay cancellation socket")?,
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let owner_cancelled = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let (outbound, requests) = crossbeam_channel::bounded(OUTBOUND_CAPACITY);
        let (events, inbound) = crossbeam_channel::bounded(INBOUND_CAPACITY);
        let worker_socket = Arc::clone(&socket);
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_owner_cancelled = Arc::clone(&owner_cancelled);
        let worker_failure = Arc::clone(&failure);
        let worker = thread::Builder::new()
            .name("nes-netplay-wire".to_owned())
            .spawn(move || {
                let result = run(
                    stream,
                    player,
                    identity,
                    secret,
                    scope,
                    requests,
                    &events,
                    &worker_cancelled,
                    &worker_owner_cancelled,
                );
                if let Err(error) = result {
                    let cause = record_failure(&worker_failure, format!("{error:#}"));
                    let _ = events.try_send(Event::Failed(cause));
                }
                worker_cancelled.store(true, Ordering::Release);
                let _ = worker_socket.shutdown(Shutdown::Both);
            })
            .context("starting netplay network worker")?;
        Ok(Self {
            outbound: Some(outbound),
            inbound,
            socket: Some(socket),
            cancelled,
            owner_cancelled,
            failure,
            worker: Some(worker),
        })
    }

    pub(crate) fn send(&self, message: Message) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            bail!(self.cancelled_reason());
        }
        let sender = self
            .outbound
            .as_ref()
            .context("netplay network is closed")?;
        match sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(error) => {
                let cause = match error {
                    TrySendError::Full(_) => "netplay outbound queue overflow",
                    TrySendError::Disconnected(_) => "netplay outbound worker disconnected",
                };
                record_failure(&self.failure, cause.to_owned());
                self.cancelled.store(true, Ordering::Release);
                if let Some(socket) = &self.socket {
                    let _ = socket.shutdown(Shutdown::Both);
                }
                bail!("{cause}")
            }
        }
    }

    pub(crate) fn poll(&self) -> Result<Option<Event>> {
        match self.inbound.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                let failure = self
                    .failure
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                bail!(
                    "{}",
                    failure
                        .as_deref()
                        .unwrap_or("netplay network worker disconnected")
                )
            }
        }
    }

    pub(crate) fn events(&self) -> &EventReceiver<Event> {
        &self.inbound
    }

    pub(crate) fn cancel(&mut self) {
        self.owner_cancelled.store(true, Ordering::Release);
        self.cancelled.store(true, Ordering::Release);
        if let Some(socket) = &self.socket {
            let _ = socket.shutdown(Shutdown::Both);
        }
        self.outbound.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn record_failure(failure: &Mutex<Option<String>>, cause: String) -> String {
    let mut failure = failure.lock().unwrap_or_else(|poison| poison.into_inner());
    failure.get_or_insert(cause).clone()
}

fn publish(events: &EventSender<Event>, event: Event) -> Result<()> {
    match events.try_send(event) {
        Ok(()) => Ok(()),
        Err(crossbeam_channel::TrySendError::Full(_)) => bail!("netplay inbound queue overflow"),
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
            bail!("netplay event owner disconnected")
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    stream: TcpStream,
    player: Player,
    identity: Identity,
    secret: [u8; 32],
    scope: ConnectionScope,
    requests: EventReceiver<Message>,
    events: &EventSender<Event>,
    cancelled: &Arc<AtomicBool>,
    owner_cancelled: &Arc<AtomicBool>,
) -> Result<()> {
    let socket = Arc::new(
        stream
            .try_clone()
            .context("cloning netplay reader cancellation socket")?,
    );
    let connection = wire::admit_cancellable_scoped(
        stream,
        player,
        &identity,
        &secret,
        scope,
        Arc::clone(cancelled),
    )?;
    ensure!(
        !cancelled.load(Ordering::Acquire),
        "netplay network cancelled during admission"
    );
    let (mut sender, mut receiver) = connection.split()?;
    publish(events, Event::Ready)?;
    let reader_events = events.clone();
    let (finished, completion) = crossbeam_channel::bounded(1);
    let worker = thread::Builder::new()
        .name("nes-netplay-read".into())
        .spawn(move || {
            let result: Result<()> = (|| {
                loop {
                    let message = receiver.receive().context("receiving netplay message")?;
                    let close = matches!(message, Message::Close { .. });
                    publish(&reader_events, Event::Message(message))?;
                    if close {
                        return Ok(());
                    }
                }
            })();
            let _ = finished.try_send(result);
        })
        .context("starting netplay reader")?;
    let reader = JoinedReader {
        socket,
        worker: Some(worker),
    };
    let mut intentional_stop = false;
    let result: Result<()> = (|| {
        while !cancelled.load(Ordering::Acquire) {
            crossbeam_channel::select_biased! {
                recv(completion) -> result => {
                    if owner_cancelled.load(Ordering::Acquire) { intentional_stop = true; return Ok(()); }
                    return result.context("netplay reader terminated")?;
                }
                recv(requests) -> message => {
                    let Ok(message) = message else { intentional_stop = true; return Ok(()); };
                    if owner_cancelled.load(Ordering::Acquire) { intentional_stop = true; return Ok(()); }
                    sender.send(&message).context("sending netplay message")?;
                    if matches!(message, Message::Close { .. }) { intentional_stop = true; return Ok(()); }
                }
                default(IDLE_POLL) => {}
            }
        }
        intentional_stop = owner_cancelled.load(Ordering::Acquire);
        Ok(())
    })();
    cancelled.store(true, Ordering::Release);
    drop(sender);
    drop(reader);
    if result.is_ok()
        && !intentional_stop
        && let Ok(reader_result) = completion.try_recv()
    {
        reader_result?;
    }
    result
}

struct JoinedReader {
    socket: Arc<TcpStream>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for JoinedReader {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
