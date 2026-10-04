use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use crossbeam_channel::{Receiver as EventReceiver, Sender as EventSender, TrySendError};
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::wire::{self, Identity, Message};

const OUTBOUND_CAPACITY: usize = 64;
const INBOUND_CAPACITY: usize = 64;
const IDLE_POLL: Duration = Duration::from_millis(10);

mod failure;

#[derive(Debug)]
pub(crate) enum Event {
    Ready,
    Message(Message),
    Failed(String),
}

pub(crate) struct Network {
    outbound: Option<EventSender<Message>>,
    inbound: EventReceiver<Event>,
    socket: Arc<TcpStream>,
    cancelled: Arc<AtomicBool>,
    owner_cancelled: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl Network {
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
            socket,
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
                let _ = self.socket.shutdown(Shutdown::Both);
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
        let _ = self.socket.shutdown(Shutdown::Both);
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
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    fn identity() -> Identity {
        Identity {
            build: [1; 32],
            build_info: Default::default(),
            source: [2; 32],
            effective: [2; 32],
            media_len: 123,
            config: [3; 32],
            initial: [4; 32],
            persistent: [5; 32],
            state_format: 11,
        }
    }

    fn streams() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (host, _) = listener.accept().unwrap();
        (host, client)
    }

    fn event(network: &Network) -> Event {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(event) = network.poll().unwrap() {
                return event;
            }
            assert!(Instant::now() < deadline, "network event deadline");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn input(player: Player, frame: u64) -> Message {
        Message::Input {
            player,
            frame,
            buttons: 0xa5,
        }
    }

    #[test]
    fn admission_and_ordered_input_checkpoint_exchange() {
        let (host, client) = streams();
        let one = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let two = Network::spawn(
            client,
            Player::Two,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        assert!(matches!(event(&one), Event::Ready));
        assert!(matches!(event(&two), Event::Ready));
        for frame in 2..5 {
            one.send(input(Player::One, frame)).unwrap();
            two.send(input(Player::Two, frame)).unwrap();
            assert!(
                matches!(event(&one), Event::Message(message) if message == input(Player::Two, frame))
            );
            assert!(
                matches!(event(&two), Event::Message(message) if message == input(Player::One, frame))
            );
            let checkpoint = Message::Checkpoint {
                frame,
                logical: [1; 32],
                video: [2; 32],
                audio: [3; 32],
                persistent: [4; 32],
            };
            one.send(checkpoint.clone()).unwrap();
            two.send(checkpoint.clone()).unwrap();
            assert!(matches!(event(&one), Event::Message(message) if message == checkpoint));
            assert!(matches!(event(&two), Event::Message(message) if message == checkpoint));
        }
    }

    #[test]
    fn admission_failure_is_reported_and_disconnect_remains_observable() {
        let (host, client) = streams();
        let one = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let two = Network::spawn(
            client,
            Player::Two,
            identity(),
            [8; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        for network in [&one, &two] {
            assert!(
                matches!(event(network), Event::Failed(cause) if cause.contains("authentication"))
            );
            let deadline = Instant::now() + Duration::from_secs(1);
            while network.poll().is_ok() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
        }
    }

    #[test]
    fn cancellation_interrupts_pending_handshake_and_read() {
        let (host, _silent_peer) = streams();
        let mut pending = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let start = Instant::now();
        pending.cancel();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(pending.send(input(Player::One, 2)).is_err());

        let (host, client) = streams();
        let mut one = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
        assert!(matches!(event(&one), Event::Ready));
        one.send(input(Player::One, 2)).unwrap();
        assert_eq!(peer.receive().unwrap(), input(Player::One, 2));
        let start = Instant::now();
        one.cancel();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn outbound_pipeline_and_unsolicited_inbound_need_no_matching_reply() {
        let (host, client) = streams();
        let network = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
        assert!(matches!(event(&network), Event::Ready));
        for frame in 2..10 {
            network.send(input(Player::One, frame)).unwrap();
        }
        for frame in 2..10 {
            assert_eq!(peer.receive().unwrap(), input(Player::One, frame));
        }
        peer.send(&Message::Progress {
            frame: 8,
            confirmed: 6,
        })
        .unwrap();
        assert!(matches!(
            event(&network),
            Event::Message(Message::Progress {
                frame: 8,
                confirmed: 6
            })
        ));
        peer.send(&input(Player::Two, 19)).unwrap();
        assert!(
            matches!(event(&network), Event::Message(message) if message == input(Player::Two, 19))
        );
    }

    #[test]
    fn unsolicited_close_stops_both_workers_without_waiting_for_acknowledgement() {
        let (host, client) = streams();
        let mut network = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
        assert!(matches!(event(&network), Event::Ready));
        peer.send(&Message::Close { frame: 9 }).unwrap();
        assert!(matches!(
            event(&network),
            Event::Message(Message::Close { frame: 9 })
        ));
        let start = Instant::now();
        network.cancel();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(network.failure.lock().unwrap().is_none());
        assert!(network.poll().is_err());
    }

    #[test]
    fn concurrent_close_cancel_and_socket_shutdown_join_without_spinning() {
        for _ in 0..8 {
            let (host, client) = streams();
            let mut one = Network::spawn(
                host,
                Player::One,
                identity(),
                [9; 32],
                ConnectionScope::Loopback,
            )
            .unwrap();
            let mut two = Network::spawn(
                client,
                Player::Two,
                identity(),
                [9; 32],
                ConnectionScope::Loopback,
            )
            .unwrap();
            assert!(matches!(event(&one), Event::Ready));
            assert!(matches!(event(&two), Event::Ready));
            one.send(Message::Close { frame: 10 }).unwrap();
            let start = Instant::now();
            let peer = thread::spawn(move || two.cancel());
            one.cancel();
            peer.join().unwrap();
            assert!(start.elapsed() < Duration::from_secs(1));
        }
    }

    #[test]
    fn silent_peer_expires_receive_deadline_without_network_generated_heartbeat() {
        let (host, client) = streams();
        let network = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let _peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
        assert!(matches!(event(&network), Event::Ready));
        assert!(matches!(event(&network), Event::Failed(_)));
    }

    #[test]
    fn outbound_overflow_cancels_without_blocking() {
        let (host, _silent_peer) = streams();
        let mut network = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        for frame in 0..OUTBOUND_CAPACITY {
            network.send(input(Player::One, frame as u64)).unwrap();
        }
        let start = Instant::now();
        assert!(
            network
                .send(input(Player::One, 4))
                .unwrap_err()
                .to_string()
                .contains("overflow")
        );
        network.cancel();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn inbound_overflow_preserves_cause_when_failed_event_cannot_fit() {
        let (host, client) = streams();
        let mut network = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
        assert!(matches!(event(&network), Event::Ready));
        network.send(input(Player::One, 2)).unwrap();
        assert_eq!(peer.receive().unwrap(), input(Player::One, 2));
        for frame in 10..(10 + INBOUND_CAPACITY as u64 + 1) {
            if peer.send(&input(Player::Two, frame)).is_err() {
                break;
            }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !network.worker.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..INBOUND_CAPACITY {
            assert!(matches!(network.poll().unwrap(), Some(Event::Message(_))));
        }
        assert!(
            network
                .poll()
                .unwrap_err()
                .to_string()
                .contains("inbound queue overflow")
        );
        network.cancel();
    }
}
