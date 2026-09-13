use super::*;

fn buffered_fixture(rate: u32) -> (Arc<Shared>, rtrb::Producer<Frame>, Callback) {
    let shared = Arc::new(Shared::new());
    shared.ready.store(1, Ordering::Release);
    shared.volume.store(100, Ordering::Release);
    shared.duration.store(rate as usize * 10, Ordering::Release);
    let (producer, callback) = queue(&shared, rate);
    (shared, producer, callback)
}

fn produce(producer: &mut rtrb::Producer<Frame>, first: usize, count: usize) {
    for position in first..first + count {
        producer.push(frame(1, position)).unwrap();
    }
}

#[test]
fn callback_bursts_and_producer_jitter_preserve_every_pcm_frame_at_all_rates() {
    for rate in [44_100, 48_000, 96_000] {
        for burst in [128, 480, 960, 1920, 2048, 4096] {
            let (shared, mut producer, mut callback) = buffered_fixture(rate);
            shared.callback_frames.store(burst, Ordering::Release);
            let preroll = shared.preroll_frames();
            produce(&mut producer, 0, preroll);
            let mut produced = preroll;
            let mut consumed = 0;
            for iteration in 0..24 {
                // Refill every second callback: one complete callback of scheduling jitter.
                if iteration % 2 == 0 {
                    let pending = produced - consumed;
                    produce(&mut producer, produced, preroll - pending);
                    produced += preroll - pending;
                }
                let mut pcm = vec![0.0f32; burst * 2];
                callback.fill(&mut pcm, 2);
                assert!(
                    pcm.as_chunks::<2>().0.iter().all(|f| *f == [0.5, -0.25]),
                    "rate={rate}, burst={burst}, iteration={iteration}"
                );
                consumed += burst;
                assert_eq!(
                    shared.cursor.load(Ordering::Acquire) as u32 as usize,
                    consumed
                );
            }
            assert_eq!(shared.underruns.load(Ordering::Acquire), 0);
            assert!(producer.slots() <= queue_frames(rate));
        }
    }
}

#[test]
fn starvation_rebuffers_whole_callbacks_without_consuming_partial_audio() {
    let (shared, mut producer, mut callback) = buffered_fixture(48_000);
    let burst = 960;
    let preroll = shared.preroll_frames();
    produce(&mut producer, 0, preroll);
    let mut pcm = vec![0.0f32; burst * 2];
    callback.fill(&mut pcm, 2);
    callback.fill(&mut pcm, 2);
    let cursor = shared.cursor.load(Ordering::Acquire);
    callback.fill(&mut pcm, 2);
    assert!(pcm.iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.cursor.load(Ordering::Acquire), cursor);
    assert_eq!(shared.underruns.load(Ordering::Acquire), 1);
    produce(&mut producer, preroll, 1);
    callback.fill(&mut pcm, 2);
    assert!(pcm.iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.cursor.load(Ordering::Acquire), cursor);
    assert_eq!(shared.underruns.load(Ordering::Acquire), 1);
    produce(&mut producer, preroll + 1, burst * 2 - 1);
    callback.fill(&mut pcm, 2);
    assert!(
        pcm.as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, -0.25])
    );
    assert_eq!(
        shared.cursor.load(Ordering::Acquire) as u32 as usize,
        burst * 3
    );
}

#[test]
fn short_final_audio_drains_below_preroll_without_counting_eof_as_starvation() {
    let (shared, mut producer, mut callback) = buffered_fixture(96_000);
    shared.duration.store(17, Ordering::Release);
    produce(&mut producer, 0, 17);
    let mut pcm = [0.0f32; 128];
    callback.fill(&mut pcm, 2);
    assert!(
        pcm[..34]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, -0.25])
    );
    assert!(pcm[34..].iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.cursor.load(Ordering::Acquire) as u32, 17);
    callback.fill(&mut pcm, 2);
    assert!(pcm.iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.underruns.load(Ordering::Acquire), 0);
}

#[test]
fn callback_size_growth_rebuffers_and_unbounded_device_requests_fail_explicitly() {
    let (shared, mut producer, mut callback) = buffered_fixture(44_100);
    let preroll = shared.preroll_frames();
    produce(&mut producer, 0, preroll);
    callback.fill(&mut [0.0f32; 256], 2);
    let cursor = shared.cursor.load(Ordering::Acquire);
    let mut large = vec![0.0f32; 4096 * 2];
    callback.fill(&mut large, 2);
    assert!(large.iter().all(|sample| *sample == 0.0));
    assert_eq!(shared.cursor.load(Ordering::Acquire), cursor);
    assert_eq!(shared.preroll_frames(), 8192);
    let mut oversized = vec![0.0f32; (queue_frames(44_100) + 1) * 2];
    callback.fill(&mut oversized, 2);
    assert!(shared.device_error.load(Ordering::Acquire));
    assert_eq!(shared.cursor.load(Ordering::Acquire), cursor);
}
