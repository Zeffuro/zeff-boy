use super::*;

fn refill(producer: &mut rtrb::Producer<Frame>, shared: &Shared, pending: &mut PendingPcm) {
    while producer.slots() > 0 {
        if pending.is_empty() {
            pending.start = pending.position();
            pending.frames = pending.pcm.len() / 2;
            pending.next = 0;
            for frame in pending.pcm.as_chunks_mut::<2>().0 {
                frame.copy_from_slice(&[16384, -8192]);
            }
        }
        pending.drain(producer, shared, 1).unwrap();
    }
}

#[test]
fn full_reserve_rebuffer_fills_the_non_block_aligned_remaining_slots() {
    let shared = Arc::new(Shared::new());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let (_, mut producer, _) = output::open(&shared, OutputMode::Capture(sender)).unwrap();
    let mut callback = receiver.recv().unwrap();
    let capacity = shared.queue_capacity.load(Ordering::Acquire);
    shared.callback_frames.store(13_000, Ordering::Release);
    shared.duration.store(200_000, Ordering::Release);
    shared.volume.store(100, Ordering::Release);
    let mut pending = PendingPcm::new(0);
    refill(&mut producer, &shared, &mut pending);
    shared.ready.store(1, Ordering::Release);
    let mut pcm = vec![0.0f32; 26_000];
    callback.fill(&mut pcm, 2);
    assert!(
        pcm.as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, -0.25])
    );
    assert_eq!(producer.slots(), 13_000);
    callback.fill(&mut pcm, 2);
    assert!(pcm.iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.underruns.load(Ordering::Acquire), 1);
    refill(&mut producer, &shared, &mut pending);
    assert_eq!(producer.slots(), 0);
    assert_eq!(pending.position(), capacity + 13_000);
    assert!(!pending.is_empty());
    callback.fill(&mut pcm, 2);
    assert!(
        pcm.as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, -0.25])
    );
    assert_eq!(shared.cursor.load(Ordering::Acquire) as u32, 26_000);
}

#[test]
fn pending_frames_keep_positions_and_stop_on_seek_or_cancellation() {
    let shared = Shared::new();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(3);
    let mut pending = PendingPcm::new(80);
    pending.pcm[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    pending.frames = 4;
    pending.drain(&mut producer, &shared, 1).unwrap();
    assert_eq!(pending.position(), 83);
    for (position, pcm) in [(80, [1, 2]), (81, [3, 4]), (82, [5, 6])] {
        let frame = consumer.pop().unwrap();
        assert_eq!(frame.position, position);
        assert_eq!(frame.pcm, pcm);
    }
    shared.cursor.store((2 << 32) | 160, Ordering::Release);
    pending.drain(&mut producer, &shared, 1).unwrap();
    assert_eq!(consumer.slots(), 0);
    assert_eq!(pending.position(), 83);
    shared.cursor.store((1 << 32) | 80, Ordering::Release);
    shared.stop();
    pending.drain(&mut producer, &shared, 1).unwrap();
    assert_eq!(consumer.slots(), 0);
    assert_eq!(pending.position(), 83);
}
