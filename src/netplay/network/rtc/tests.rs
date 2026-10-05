use std::sync::atomic::AtomicUsize;

use super::*;
use crate::netplay::network::tests::{event, identity, input};
use tokio::sync::mpsc;

struct FakePeer {
    incoming: mpsc::Receiver<Packet>,
    outgoing: mpsc::Sender<Packet>,
    drop_inputs: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
}

impl Peer for FakePeer {
    async fn send_control(&self, bytes: &[u8]) -> Result<()> {
        self.outgoing
            .send(Packet {
                kind: PacketKind::Control,
                bytes: bytes.into(),
            })
            .await?;
        Ok(())
    }
    async fn send_input(&self, bytes: &[u8]) -> Result<()> {
        let mut remaining = self.drop_inputs.load(Ordering::Relaxed);
        while remaining > 0 {
            match self.drop_inputs.compare_exchange_weak(
                remaining,
                remaining - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(value) => remaining = value,
            }
        }
        self.outgoing
            .send(Packet {
                kind: PacketKind::Input,
                bytes: bytes.into(),
            })
            .await?;
        Ok(())
    }
    async fn receive(&mut self, wait: Duration) -> Result<Packet> {
        tokio::time::timeout(wait, self.incoming.recv())
            .await?
            .context("fake peer closed")
    }
    async fn close(self) -> Result<()> {
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
}

fn peers(capacity: usize) -> (FakePeer, FakePeer) {
    let (a_tx, a_rx) = mpsc::channel(capacity);
    let (b_tx, b_rx) = mpsc::channel(capacity);
    let peer = |incoming, outgoing| FakePeer {
        incoming,
        outgoing,
        drop_inputs: Arc::default(),
        closed: Arc::default(),
    };
    (peer(a_rx, b_tx), peer(b_rx, a_tx))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn network(peer: FakePeer, player: Player) -> Network {
    spawn_peer(
        peer,
        runtime(),
        player,
        identity(),
        [9; 32],
        InputDelay::default(),
    )
    .unwrap()
}

#[test]
fn retransmits_last_dropped_input_during_pause_without_new_commands() {
    let (a, b) = peers(64);
    a.drop_inputs.store(2, Ordering::Relaxed);
    let a_closed = a.closed.clone();
    let b_closed = b.closed.clone();
    let mut a = network(a, Player::One);
    let mut b = network(b, Player::Two);
    assert!(matches!(event(&a), Event::Ready));
    assert!(matches!(event(&b), Event::Ready));
    a.send(input(Player::One, 2)).unwrap();
    assert!(matches!(event(&b), Event::Message(message) if message == input(Player::One, 2)));
    b.send(Message::Chat {
        text: "paused".into(),
    })
    .unwrap();
    assert!(matches!(event(&a), Event::Message(Message::Chat { text }) if text == "paused"));
    let deadline = Instant::now() + Duration::from_secs(1);
    while a.stats().received < 2 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let stats = a.stats();
    assert!(stats.sent >= 3 && stats.sent_bytes >= 3 * 86);
    assert!(stats.repeated_batches >= 2);
    assert_eq!(stats.ack_samples, 1);
    assert!(stats.ack_wait.unwrap() >= RETRY);
    a.cancel();
    b.cancel();
    assert!(a_closed.load(Ordering::Acquire));
    assert!(b_closed.load(Ordering::Acquire));
}

#[test]
fn cancellation_interrupts_admission_and_stalled_send() {
    for stalled_send in [false, true] {
        let (a, _silent_peer) = peers(1);
        let closed = a.closed.clone();
        if stalled_send {
            a.outgoing
                .try_send(Packet {
                    kind: PacketKind::Control,
                    bytes: vec![1],
                })
                .unwrap();
        }
        let mut a = network(a, Player::One);
        thread::sleep(Duration::from_millis(20));
        let start = Instant::now();
        a.cancel();
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(closed.load(Ordering::Acquire));
        assert!(a.failure.lock().unwrap().is_none());
        assert!(a.send(input(Player::One, 2)).is_err());
    }
}

async fn admit_manual(peer: &mut FakePeer) -> PacketCodec {
    let mut admission = Admission::new(Player::Two, identity(), [9; 32]).unwrap();
    peer.send_control(admission.hello()).await.unwrap();
    while !admission.complete() {
        let packet = peer.receive(Duration::from_secs(2)).await.unwrap();
        assert_eq!(packet.kind, PacketKind::Control);
        if let Some(reply) = admission.receive(&packet.bytes).unwrap() {
            peer.send_control(&reply).await.unwrap();
        }
    }
    admission.into_codec().unwrap()
}

#[test]
fn control_channel_rejects_inputs_and_idle_peer_expires() {
    for wrong_channel in [true, false] {
        let (a, mut peer) = peers(64);
        let a = network(a, Player::One);
        let runtime = runtime();
        let mut codec = runtime.block_on(admit_manual(&mut peer));
        assert!(matches!(event(&a), Event::Ready));
        if wrong_channel {
            let packet = codec.encode(&input(Player::Two, 2)).unwrap();
            runtime.block_on(peer.send_control(&packet)).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(Event::Failed(cause)) = a.poll().unwrap() {
                assert!(cause.contains(if wrong_channel {
                    "input on reliable"
                } else {
                    "receive timed out"
                }));
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn transferred_runtime_preserves_existing_tasks() {
    let (a, mut peer) = peers(64);
    let runtime = runtime();
    let progress_sender = a.outgoing.clone();
    let original_task_ran = Arc::new(AtomicBool::new(false));
    let marker = original_task_ran.clone();
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        marker.store(true, Ordering::Release);
        drop(progress_sender);
    });
    let mut a = spawn_peer(
        a,
        runtime,
        Player::One,
        identity(),
        [9; 32],
        InputDelay::default(),
    )
    .unwrap();
    let peer_runtime = self::runtime();
    peer_runtime.block_on(admit_manual(&mut peer));
    assert!(matches!(event(&a), Event::Ready));
    let deadline = Instant::now() + Duration::from_secs(1);
    while !original_task_ran.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    a.cancel();
}

#[test]
fn early_unordered_input_waits_for_authenticated_ready_barrier() {
    let (a, mut peer) = peers(64);
    let a = network(a, Player::One);
    runtime().block_on(async {
        let mut admission = Admission::new(Player::Two, identity(), [9; 32]).unwrap();
        peer.send_control(admission.hello()).await.unwrap();
        let hello = peer.receive(ADMISSION).await.unwrap();
        let authentication = admission.receive(&hello.bytes).unwrap().unwrap();
        peer.send_control(&authentication).await.unwrap();
        let authentication = peer.receive(ADMISSION).await.unwrap();
        let ready = admission.receive(&authentication.bytes).unwrap().unwrap();
        let host_ready = peer.receive(ADMISSION).await.unwrap();
        assert!(admission.receive(&host_ready.bytes).unwrap().is_none());
        let codec = admission.into_codec().unwrap();
        let mut inputs = InputChannel::new(Player::Two, 2);
        inputs.push(2, 0xa5).unwrap();
        peer.send_input(&inputs.encode(&codec).unwrap().unwrap())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(a.poll().unwrap().is_none());
        peer.send_control(&ready).await.unwrap();
    });
    assert!(matches!(event(&a), Event::Ready));
    assert!(matches!(event(&a), Event::Message(message) if message == input(Player::Two, 2)));
}

#[test]
fn event_overflow_preserves_failure_when_diagnostic_cannot_fit() {
    let (a, b) = peers(64);
    let a = network(a, Player::One);
    let mut b = network(b, Player::Two);
    assert!(matches!(event(&a), Event::Ready));
    assert!(matches!(event(&b), Event::Ready));
    for frame in 2..67 {
        if a.send(input(Player::One, frame)).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !b.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..INBOUND_CAPACITY {
        assert!(matches!(
            b.poll().unwrap(),
            Some(Event::Message(Message::Input { .. }))
        ));
    }
    assert!(
        b.poll()
            .unwrap_err()
            .to_string()
            .contains("inbound queue overflow")
    );
    b.cancel();
}
