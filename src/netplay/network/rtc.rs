use std::collections::VecDeque;
use std::future::Future;
use std::time::Instant;

use super::*;
use zeff_netplay::datagram::InputChannel;
use zeff_netplay::wire::{Admission, PacketCodec};
use zeff_netplay_connect::{DataConnection, Packet, PacketKind};

const POLL: Duration = Duration::from_millis(5);
const RETRY: Duration = Duration::from_millis(25);
const ADMISSION: Duration = Duration::from_secs(2);
const SILENCE: Duration = Duration::from_secs(3);

trait Peer: Send + 'static {
    async fn send_control(&self, bytes: &[u8]) -> Result<()>;
    async fn send_input(&self, bytes: &[u8]) -> Result<()>;
    async fn receive(&mut self, wait: Duration) -> Result<Packet>;
    async fn close(self) -> Result<()>;
}

impl Peer for DataConnection {
    async fn send_control(&self, bytes: &[u8]) -> Result<()> {
        self.send_control(bytes).await
    }
    async fn send_input(&self, bytes: &[u8]) -> Result<()> {
        self.try_send_input(bytes).await
    }
    async fn receive(&mut self, wait: Duration) -> Result<Packet> {
        self.receive(wait).await
    }
    async fn close(self) -> Result<()> {
        self.close().await
    }
}

pub(super) fn spawn(
    peer: crate::netplay::DirectPeer,
    player: Player,
    identity: Identity,
    secret: [u8; 32],
    delay: InputDelay,
) -> Result<Network> {
    spawn_peer(
        peer.connection,
        peer.runtime,
        player,
        identity,
        secret,
        delay,
    )
}

fn spawn_peer(
    connection: impl Peer,
    runtime: tokio::runtime::Runtime,
    player: Player,
    identity: Identity,
    secret: [u8; 32],
    delay: InputDelay,
) -> Result<Network> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let owner_cancelled = Arc::new(AtomicBool::new(false));
    let failure = Arc::new(Mutex::new(None));
    let metrics = Arc::new(Mutex::new(Metrics::direct()));
    let (outbound, requests) = crossbeam_channel::bounded(OUTBOUND_CAPACITY);
    let (events, inbound) = crossbeam_channel::bounded(INBOUND_CAPACITY);
    let state = Worker {
        player,
        identity,
        secret,
        delay,
        requests,
        events,
        cancelled: cancelled.clone(),
        owner_cancelled: owner_cancelled.clone(),
        metrics: metrics.clone(),
    };
    let worker_failure = failure.clone();
    let worker_cancelled = cancelled.clone();
    let worker = thread::Builder::new()
        .name("netplay-direct".into())
        .spawn(move || {
            let result = runtime.block_on(async {
                let mut connection = connection;
                let result = state.run(&mut connection).await;
                let _ = tokio::time::timeout(Duration::from_millis(250), connection.close()).await;
                result
            });
            if let Err(error) = result
                && !state.owner_cancelled.load(Ordering::Acquire)
            {
                let cause = record_failure(&worker_failure, format!("{error:#}"));
                let _ = state.events.try_send(Event::Failed(cause));
            }
            worker_cancelled.store(true, Ordering::Release);
            runtime.shutdown_timeout(Duration::from_millis(100));
        })
        .context("starting direct netplay worker")?;
    Ok(Network {
        outbound: Some(outbound),
        inbound,
        socket: None,
        cancelled,
        owner_cancelled,
        failure,
        metrics,
        worker: Some(worker),
    })
}

struct Worker {
    player: Player,
    identity: Identity,
    secret: [u8; 32],
    delay: InputDelay,
    requests: EventReceiver<Message>,
    events: EventSender<Event>,
    cancelled: Arc<AtomicBool>,
    owner_cancelled: Arc<AtomicBool>,
    metrics: Arc<Mutex<Metrics>>,
}

impl Worker {
    async fn run(&self, connection: &mut impl Peer) -> Result<()> {
        let (mut codec, pending) = tokio::time::timeout(ADMISSION, async {
            let mut admission = Admission::new(self.player, self.identity.clone(), self.secret)?;
            cancellable(&self.cancelled, connection.send_control(admission.hello())).await?;
            let mut pending = VecDeque::new();
            while !admission.complete() {
                if let Some(packet) = self.receive(connection).await? {
                    match packet.kind {
                        PacketKind::Control => {
                            if let Some(reply) = admission.receive(&packet.bytes)? {
                                cancellable(&self.cancelled, connection.send_control(&reply))
                                    .await?;
                            }
                        }
                        PacketKind::Input => {
                            ensure!(
                                pending.len() < INBOUND_CAPACITY,
                                "pre-admission input queue overflow"
                            );
                            pending.push_back(packet);
                        }
                    }
                }
            }
            Ok::<_, anyhow::Error>((admission.into_codec()?, pending))
        })
        .await
        .context("direct netplay admission timed out")??;
        let mut inputs = InputChannel::new(self.player, self.delay.frames());
        publish(&self.events, Event::Ready)?;
        for packet in pending {
            self.packet(&mut codec, &mut inputs, packet)?;
        }
        let mut heard = Instant::now();
        let mut retry = Instant::now();
        loop {
            ensure!(
                !self.cancelled.load(Ordering::Acquire),
                "direct netplay cancelled"
            );
            for _ in 0..16 {
                let message = match self.requests.try_recv() {
                    Ok(message) => message,
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
                };
                let close = matches!(message, Message::Close { .. });
                match message {
                    Message::Input {
                        player,
                        frame,
                        buttons,
                    } => {
                        ensure!(player == self.player, "input player does not own this port");
                        inputs.push(frame, buttons)?;
                        self.send_inputs(connection, &codec, &mut inputs).await?;
                    }
                    message => {
                        let packet = codec.encode(&message)?;
                        cancellable(&self.cancelled, connection.send_control(&packet)).await?;
                        self.metrics
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .sent(packet.len());
                    }
                }
                if close {
                    return Ok(());
                }
            }
            if retry.elapsed() >= RETRY {
                self.send_inputs(connection, &codec, &mut inputs).await?;
                retry = Instant::now();
            }
            if let Some(packet) = self.receive(connection).await? {
                let close = self.packet(&mut codec, &mut inputs, packet)?;
                heard = Instant::now();
                if close {
                    return Ok(());
                }
            }
            ensure!(
                heard.elapsed() < SILENCE,
                "direct netplay peer receive timed out"
            );
        }
    }

    fn packet(
        &self,
        codec: &mut PacketCodec,
        inputs: &mut InputChannel,
        packet: Packet,
    ) -> Result<bool> {
        let packet_bytes = packet.bytes.len();
        match packet.kind {
            PacketKind::Control => {
                let message = codec.decode(&packet.bytes)?;
                ensure!(
                    !matches!(message, Message::Input { .. }),
                    "input on reliable control channel"
                );
                let close = matches!(message, Message::Close { .. });
                publish(&self.events, Event::Message(message))?;
                Ok(close)
            }
            PacketKind::Input => {
                for message in inputs.receive(codec, &packet.bytes)? {
                    publish(&self.events, Event::Message(message))?;
                }
                self.metrics
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .acknowledge(inputs.acknowledged(), crate::platform::Instant::now());
                Ok(false)
            }
        }
        .inspect(|_| {
            self.metrics
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .received(packet_bytes)
        })
    }

    async fn send_inputs(
        &self,
        connection: &impl Peer,
        codec: &PacketCodec,
        inputs: &mut InputChannel,
    ) -> Result<()> {
        if let Some(packet) = inputs.encode(codec)? {
            let attempted_at = crate::platform::Instant::now();
            cancellable(&self.cancelled, connection.send_input(&packet)).await?;
            let mut metrics = self
                .metrics
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            metrics.sent(packet.len());
            metrics.input_attempt(inputs.pending_batch(), attempted_at);
        }
        Ok(())
    }

    async fn receive(&self, connection: &mut impl Peer) -> Result<Option<Packet>> {
        ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "direct netplay cancelled"
        );
        match connection.receive(POLL).await {
            Ok(packet) => Ok(Some(packet)),
            Err(error)
                if error
                    .downcast_ref::<tokio::time::error::Elapsed>()
                    .is_some() =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

async fn cancellable<T>(
    cancelled: &AtomicBool,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    let operation = tokio::time::timeout(ADMISSION, operation);
    tokio::pin!(operation);
    loop {
        ensure!(
            !cancelled.load(Ordering::Acquire),
            "direct netplay cancelled"
        );
        tokio::select! {
            result = &mut operation => return result?,
            _ = tokio::time::sleep(POLL) => {}
        }
    }
}

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wan_tests;
