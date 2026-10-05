use std::sync::atomic::AtomicU64;

use super::*;
use tokio::sync::mpsc;

const CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug)]
pub(super) struct Profile {
    pub name: &'static str,
    pub travel: Duration,
    pub impaired: bool,
}

pub(super) const LOCAL: Profile = Profile {
    name: "local",
    travel: Duration::ZERO,
    impaired: false,
};
pub(super) const WAN: Profile = Profile {
    name: "200ms",
    travel: Duration::from_millis(100),
    impaired: false,
};
pub(super) const IMPAIRED: Profile = Profile {
    name: "200ms-jitter-loss-burst",
    travel: WAN.travel,
    impaired: true,
};

#[derive(Default)]
pub(super) struct Counters {
    pub dropped: AtomicU64,
    pub periodic_drops: AtomicU64,
    pub burst_drops: AtomicU64,
    pub reordered: AtomicU64,
    pub max_pending: AtomicU64,
    pub closed: AtomicBool,
}

struct Envelope {
    due: Instant,
    input_sequence: Option<u64>,
    packet: Packet,
}

#[derive(Default)]
struct Sending {
    first_input: Option<Instant>,
    burst_started: Option<Instant>,
    sequence: u64,
    control_due: Option<Instant>,
}

pub(super) struct Link {
    incoming: mpsc::Receiver<Envelope>,
    outgoing: mpsc::Sender<Envelope>,
    pending: Vec<Envelope>,
    sending: Mutex<Sending>,
    profile: Profile,
    pub counters: Arc<Counters>,
    last_input: u64,
}

impl Link {
    fn send(&self, kind: PacketKind, bytes: &[u8]) -> Result<()> {
        let now = Instant::now();
        let mut state = self.sending.lock().unwrap();
        let mut due = now + self.profile.travel;
        let input_sequence = if matches!(kind, PacketKind::Input) {
            state.sequence += 1;
            let age = now.duration_since(*state.first_input.get_or_insert(now));
            if self.profile.impaired {
                if age >= Duration::from_millis(600) && state.burst_started.is_none() {
                    state.burst_started = Some(now);
                }
                if state
                    .burst_started
                    .is_some_and(|start| now.duration_since(start) < Duration::from_millis(180))
                {
                    self.counters.burst_drops.fetch_add(1, Ordering::Relaxed);
                    self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                if state.sequence.is_multiple_of(13) {
                    self.counters.periodic_drops.fetch_add(1, Ordering::Relaxed);
                    self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                if state.sequence.is_multiple_of(7) {
                    due += Duration::from_millis(60);
                }
            }
            Some(state.sequence)
        } else {
            due = due.max(state.control_due.unwrap_or(due));
            state.control_due = Some(due);
            None
        };
        // Schedule delivery without blocking the sender's network worker.
        self.outgoing.try_send(Envelope {
            due,
            input_sequence,
            packet: Packet {
                kind,
                bytes: bytes.into(),
            },
        })?;
        Ok(())
    }

    fn enqueue(&mut self, envelope: Envelope) -> Result<()> {
        ensure!(self.pending.len() < CAPACITY, "shaped link queue overflow");
        self.pending.push(envelope);
        self.counters
            .max_pending
            .fetch_max(self.pending.len() as u64, Ordering::Relaxed);
        Ok(())
    }
}

impl Peer for Link {
    async fn send_control(&self, bytes: &[u8]) -> Result<()> {
        self.send(PacketKind::Control, bytes)
    }

    async fn send_input(&self, bytes: &[u8]) -> Result<()> {
        self.send(PacketKind::Input, bytes)
    }

    async fn receive(&mut self, wait: Duration) -> Result<Packet> {
        tokio::time::timeout(wait, async {
            loop {
                while let Ok(envelope) = self.incoming.try_recv() {
                    self.enqueue(envelope)?;
                }
                let next = self
                    .pending
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, packet)| packet.due)
                    .map(|(index, packet)| (index, packet.due));
                if let Some((index, due)) = next {
                    if due <= Instant::now() {
                        let envelope = self.pending.remove(index);
                        if let Some(sequence) = envelope.input_sequence {
                            if sequence < self.last_input {
                                self.counters.reordered.fetch_add(1, Ordering::Relaxed);
                            }
                            self.last_input = self.last_input.max(sequence);
                        }
                        return Ok(envelope.packet);
                    }
                    tokio::select! {
                        envelope = self.incoming.recv() => {
                            self.enqueue(envelope.context("shaped peer closed")?)?;
                        }
                        _ = tokio::time::sleep(due.saturating_duration_since(Instant::now())) => {}
                    }
                } else {
                    let envelope = self.incoming.recv().await.context("shaped peer closed")?;
                    self.enqueue(envelope)?;
                }
            }
        })
        .await?
    }

    async fn close(self) -> Result<()> {
        self.counters.closed.store(true, Ordering::Release);
        Ok(())
    }
}

pub(super) fn pair(profile: Profile) -> [Link; 2] {
    let (a_tx, a_rx) = mpsc::channel(CAPACITY);
    let (b_tx, b_rx) = mpsc::channel(CAPACITY);
    let peer = |incoming, outgoing| Link {
        incoming,
        outgoing,
        pending: Vec::new(),
        sending: Mutex::default(),
        profile,
        counters: Arc::default(),
        last_input: 0,
    };
    [peer(a_rx, b_tx), peer(b_rx, a_tx)]
}
