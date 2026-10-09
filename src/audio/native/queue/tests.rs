use super::*;

fn playback(generation: Arc<AtomicU64>, preroll: usize) -> AudioPlaybackState {
    AudioPlaybackState::new(
        Arc::new(AtomicUsize::new(preroll)),
        Arc::new(AtomicU64::new(0)),
        generation,
    )
}

#[test]
fn pending_burst_reaches_callbacks_without_another_audio_admission() {
    for rate in [44_100, 48_000] {
        for policy in [
            AudioBufferPolicy::Auto,
            AudioBufferPolicy::LowLatency,
            AudioBufferPolicy::Balanced,
            AudioBufferPolicy::Stable,
        ] {
            let capacity = ring_buffer_capacity_for_policy(rate, policy);
            let callback = 256;
            let expected: Vec<_> = (0..capacity * 3 / callback * callback)
                .map(|index| (index + 1) as f32 / 131_072.0)
                .collect();
            for pump in [false, true] {
                let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
                let mut queue = NativeSampleQueue::new(producer);
                let mut state = playback(
                    Arc::new(AtomicU64::new(0)),
                    playback_preroll_samples(rate, policy.normal_queue_ms()),
                );
                queue.staged_samples.extend(&expected);
                queue.pump();
                let mut actual = Vec::new();
                for _ in 0..expected.len() / callback {
                    if pump {
                        queue.pump();
                        queue.pump();
                    }
                    let mut pcm = vec![0.0; callback];
                    fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
                    actual.extend(pcm);
                }
                if pump {
                    assert_eq!(actual, expected, "{rate} {policy:?}");
                    assert!(queue.staged_samples.is_empty());
                    assert_eq!(consumer.slots(), 0);
                    assert_eq!(state.underruns.load(Ordering::Relaxed), 0);
                } else {
                    let played = capacity / callback * callback;
                    assert_eq!(&actual[..played], &expected[..played]);
                    assert!(actual[played..].iter().all(|sample| *sample == 0.0));
                    assert!(!queue.staged_samples.is_empty());
                    assert_eq!(state.underruns.load(Ordering::Relaxed), 1);
                }
            }
        }
    }
}

#[test]
fn generation_change_never_pumps_discarded_staging_or_replays_old_ring_pcm() {
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let generation = Arc::new(AtomicU64::new(0));
    let mut state = playback(Arc::clone(&generation), 2);
    let mut queue = NativeSampleQueue::new(producer);
    queue.staged_samples.extend([0.5; 12]);
    queue.pump();
    queue.generation = queue.generation.wrapping_add(1);
    generation.store(queue.generation, Ordering::Release);
    queue.staged_samples.clear();
    queue.staged_samples.extend([0.25, -0.25, 0.125, -0.125]);
    queue.pump();
    assert_eq!(queue.staged_samples.len(), 4);
    let mut pcm = [1.0; 4];
    fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
    assert_eq!(pcm, [0.0; 4]);
    queue.pump();
    queue.pump();
    fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
    assert_eq!(pcm, [0.25, -0.25, 0.125, -0.125]);
    assert!(queue.staged_samples.is_empty());
    assert_eq!(consumer.slots(), 0);
}
