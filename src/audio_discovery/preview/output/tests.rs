use super::*;

fn fixture() -> (Arc<Shared>, rtrb::Producer<Frame>, Callback) {
    let shared = Arc::new(Shared::new());
    shared.ready.store(1, Ordering::Release);
    shared.volume.store(100, Ordering::Release);
    shared.duration.store(1_000_000, Ordering::Release);
    let (producer, consumer) = rtrb::RingBuffer::new(queue_frames(48_000));
    let callback = Callback {
        consumer,
        shared: Arc::clone(&shared),
        held: None,
        generation: 1,
        buffering: false,
    };
    (shared, producer, callback)
}

#[test]
fn pause_after_pop_holds_the_frame_and_seek_discards_it_without_rewinding_cursor() {
    let (shared, mut producer, mut callback) = fixture();
    producer.push(frame(1, 0)).unwrap();
    assert_eq!(
        callback.next_after_pop(|| shared.playing.store(false, Ordering::Release)),
        [0.0; 2]
    );
    assert_eq!(shared.cursor.load(Ordering::Acquire), 1 << 32);
    shared.playing.store(true, Ordering::Release);
    assert_eq!(callback.next(), [0.5, -0.25]);
    assert_eq!(shared.cursor.load(Ordering::Acquire), (1 << 32) | 1);
    producer.push(frame(1, 1)).unwrap();
    assert_eq!(
        callback.next_after_pop(|| shared.cursor.store((2 << 32) | 80, Ordering::Release)),
        [0.0; 2]
    );
    assert_eq!(shared.cursor.load(Ordering::Acquire), (2 << 32) | 80);
    producer.push(frame(2, 80)).unwrap();
    shared.ready.store(2, Ordering::Release);
    assert_eq!(callback.next(), [0.5, -0.25]);
    assert_eq!(shared.cursor.load(Ordering::Acquire), (2 << 32) | 81);
}

fn frame(generation: u64, position: usize) -> Frame {
    Frame {
        generation,
        position,
        pcm: [16_384, -8192],
    }
}

#[test]
fn pause_underrun_and_cancel_zero_fill_without_advancing_current_audio() {
    let (shared, mut producer, mut callback) = fixture();
    producer.push(frame(1, 0)).unwrap();
    shared.playing.store(false, Ordering::Release);
    let mut data = [99.0f32; 4];
    callback.fill(&mut data, 2);
    assert_eq!(data, [0.0; 4]);
    assert_eq!(shared.cursor.load(Ordering::Acquire), 1 << 32);
    assert_eq!(producer.slots(), queue_frames(48_000) - 1);
    shared.duration.store(1, Ordering::Release);
    shared.playing.store(true, Ordering::Release);
    callback.fill(&mut data, 2);
    assert_eq!(data, [0.5, -0.25, 0.0, 0.0]);
    assert_eq!(shared.cursor.load(Ordering::Acquire), (1 << 32) | 1);
    producer.push(frame(1, 1)).unwrap();
    shared.stop();
    callback.fill(&mut data, 2);
    assert_eq!(data, [0.0; 4]);
    assert_eq!(producer.slots(), queue_frames(48_000) - 1);
}

#[test]
fn paused_seek_drains_stale_queue_but_preserves_new_position_and_frames() {
    let (shared, mut producer, mut callback) = fixture();
    for position in 0..queue_frames(48_000) {
        producer.push(frame(1, position)).unwrap();
    }
    shared.playing.store(false, Ordering::Release);
    shared.cursor.store((2 << 32) | 800, Ordering::Release);
    shared.duration.store(801, Ordering::Release);
    let mut data = [99.0f32; 2];
    callback.fill(&mut data, 2);
    assert_eq!(producer.slots(), queue_frames(48_000));
    assert_eq!(shared.cursor.load(Ordering::Acquire), (2 << 32) | 800);
    producer.push(frame(2, 800)).unwrap();
    shared.ready.store(2, Ordering::Release);
    callback.fill(&mut data, 2);
    assert_eq!(data, [0.0; 2]);
    assert_eq!(producer.slots(), queue_frames(48_000) - 1);
    shared.playing.store(true, Ordering::Release);
    callback.fill(&mut data, 2);
    assert_eq!(data, [0.5, -0.25]);
    assert_eq!(shared.cursor.load(Ordering::Acquire), (2 << 32) | 801);
}

#[test]
fn callback_converts_mono_multichannel_unsigned_and_volume() {
    let (shared, mut producer, mut callback) = fixture();
    producer.push(frame(1, 0)).unwrap();
    let mut mono = [0.0f32; 1];
    callback.fill(&mut mono, 1);
    assert_eq!(mono, [0.125]);
    producer.push(frame(1, 1)).unwrap();
    shared.volume.store(50, Ordering::Release);
    let mut surround = [1.0f32; 6];
    callback.fill(&mut surround, 6);
    assert_eq!(surround, [0.25, -0.125, 0.0, 0.0, 0.0, 0.0]);
    let mut unsigned = [0u16; 2];
    callback.fill(&mut unsigned, 2);
    assert_eq!(unsigned, [32768; 2]);
}

#[path = "tests/buffering.rs"]
mod buffering;
