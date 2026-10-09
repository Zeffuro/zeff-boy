use super::*;

mod output;

fn config(
    rate: u32,
    underruns: u64,
    elapsed: Duration,
    callback_samples: usize,
) -> QueueBudgetConfig {
    QueueBudgetConfig {
        rate,
        policy: AudioBufferPolicy::Auto,
        speed: 1,
        callback_samples,
        underruns,
        elapsed,
    }
}

fn queue(
    rate: u32,
) -> (
    NativeSampleQueue,
    rtrb::Consumer<QueuedAudioSample>,
    AudioPlaybackState,
) {
    let (producer, consumer) = rtrb::RingBuffer::new(ring_buffer_capacity_for_policy(
        rate,
        AudioBufferPolicy::Auto,
    ));
    let state = AudioPlaybackState::new(
        Arc::new(AtomicUsize::new(playback_preroll_samples(rate, 200))),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    (NativeSampleQueue::new(producer), consumer, state)
}

#[test]
fn netplay_audio_budget_recovery_is_scoped_and_survives_discard_and_replacement() {
    let (mut queue, _, state) = queue(48_000);
    let apply = |queue: &mut NativeSampleQueue, underruns, millis| {
        queue.configure_budget(
            config(48_000, underruns, Duration::from_millis(millis), 512),
            &state.preroll_samples,
        )
    };
    queue.netplay_budget.set_enabled(true);
    for (index, underruns) in [1, 0, 0, 1, 0, 0, 1].into_iter().enumerate() {
        assert_eq!(apply(&mut queue, underruns, index as u64 * 16), index == 6);
        assert_eq!(queue.limit, if index == 6 { 19_200 } else { 7_200 });
    }
    assert_eq!(state.preroll_samples.load(Ordering::Relaxed), 9_600);
    queue.netplay_budget.discard();
    assert!(!apply(&mut queue, 0, 200));
    assert_eq!(queue.limit, 19_200);
    let mut replacement = NetplayBufferBudget::default();
    replacement.inherit(&queue.netplay_budget);
    assert_eq!(
        replacement.policy(AudioBufferPolicy::Auto),
        AudioBufferPolicy::Auto
    );
    assert!(!queue.netplay_budget.set_enabled(true));
    assert!(queue.netplay_budget.set_enabled(false));
    assert_eq!(
        queue.netplay_budget.policy(AudioBufferPolicy::Auto),
        AudioBufferPolicy::Auto
    );
    queue.netplay_budget.set_enabled(true);
    assert!(!apply(&mut queue, 2, 300));
    assert_eq!(queue.limit, 7_200);
    queue.netplay_budget.discard();
    assert!(!apply(&mut queue, 1, 301));
    assert!(!apply(&mut queue, 1, 5_302));
    assert!(!apply(&mut queue, 1, 5_303));
    assert!(apply(&mut queue, 1, 5_304));
    for requested in [
        AudioBufferPolicy::LowLatency,
        AudioBufferPolicy::Balanced,
        AudioBufferPolicy::Stable,
    ] {
        assert_eq!(replacement.policy(requested), requested);
    }
}

#[test]
fn netplay_audio_budget_oversized_unprimed_callback_recovers_without_underruns() {
    for rate in [44_100, 48_000, 96_000] {
        let (mut queue, mut consumer, mut state) = queue(rate);
        queue.netplay_budget.set_enabled(true);
        queue.configure_budget(config(rate, 0, Duration::ZERO, 0), &state.preroll_samples);
        let expected: Vec<_> = (0..rate as usize / 5)
            .map(|index| (index + 1) as f32 / 65_536.0)
            .collect();
        queue.staged_samples.extend(&expected);
        queue.pump();
        let before = queue.staged_samples.len();
        let mut pcm = vec![0.0; expected.len()];
        fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
        assert!(pcm.iter().all(|sample| *sample == 0.0));
        assert_eq!(queue.staged_samples.len(), before);
        assert_eq!(state.underruns.load(Ordering::Relaxed), 0);
        assert_eq!(
            state.callback_samples.load(Ordering::Relaxed),
            expected.len()
        );
        assert!(queue.configure_budget(
            config(rate, 0, Duration::ZERO, expected.len()),
            &state.preroll_samples
        ));
        queue.pump();
        fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
        assert_eq!(pcm, expected);
        assert!(queue.staged_samples.is_empty());
        let oversized = queue.producer.buffer().capacity() + 2;
        queue.staged_samples.extend([0.25; 128]);
        queue.pump();
        let mut pcm = vec![0.0; oversized];
        fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
        assert_eq!(&pcm[..128], &[0.25; 128]);
        assert!(pcm[128..].iter().all(|sample| *sample == 0.0));
        assert_eq!(state.underruns.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn netplay_audio_budget_entry_and_stale_generation_never_drop_fifo_samples() {
    for rate in [44_100, 48_000, 96_000] {
        for stale in [false, true] {
            let (mut queue, mut consumer, mut state) = queue(rate);
            let old: Vec<_> = vec![0.25; queue.limit];
            queue.staged_samples.extend(&old);
            queue.pump();
            if stale {
                queue.generation += 1;
                state.generation.store(queue.generation, Ordering::Release);
            }
            queue.netplay_budget.set_enabled(true);
            queue.configure_budget(config(rate, 0, Duration::ZERO, 256), &state.preroll_samples);
            let fresh: Vec<_> = (0..25_600)
                .map(|index| (index + 1) as f32 / 65_536.0)
                .collect();
            queue.staged_samples.extend(&fresh);
            queue.pump();
            assert_eq!(queue.staged_samples.len(), fresh.len());
            let mut expected = if stale { Vec::new() } else { old };
            expected.extend(&fresh);
            let padding = queue.limit * 2;
            queue
                .staged_samples
                .extend(std::iter::repeat_n(0.0, padding));
            let mut actual = Vec::new();
            for callback in 0..500 {
                if !(3..78).contains(&callback) {
                    queue.pump();
                }
                let mut pcm = [0.0; 256];
                fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
                actual.extend(pcm.into_iter().filter(|sample| *sample != 0.0));
                if actual.len() == expected.len() {
                    break;
                }
            }
            assert_eq!(actual, expected, "{rate} {stale}");
            assert_eq!(consumer.slots() & 1, 0);
            if stale {
                assert!(state.underruns.load(Ordering::Relaxed) > 0);
            }
        }
    }
}

#[test]
fn netplay_audio_budget_in_place_recovery_rebuffers_without_consuming_pcm() {
    let (mut queue, mut consumer, mut state) = queue(48_000);
    queue.netplay_budget.set_enabled(true);
    queue.configure_budget(
        config(48_000, 0, Duration::ZERO, 512),
        &state.preroll_samples,
    );
    let expected: Vec<_> = (0..19_200)
        .map(|index| (index + 1) as f32 / 65_536.0)
        .collect();
    queue.staged_samples.extend(&expected);
    queue.staged_samples.extend([0.0; 19_200]);
    queue.pump();
    let mut pcm = [0.0; 512];
    fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
    assert_eq!(pcm, expected[..512]);
    let mut actual = pcm.to_vec();
    let queued = consumer.slots();
    assert!(queue.configure_budget(
        config(48_000, 3, Duration::from_secs(1), 512),
        &state.preroll_samples
    ));
    fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
    assert_eq!(pcm, [0.0; 512]);
    assert_eq!(consumer.slots(), queued);
    for _ in 0..100 {
        queue.pump();
        fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
        actual.extend(pcm.into_iter().filter(|sample| *sample != 0.0));
        if actual.len() == expected.len() {
            break;
        }
    }
    assert_eq!(actual, expected);
}

#[test]
fn netplay_audio_budget_resampler_feedback_matches_the_existing_low_latency_queue() {
    let (mut adaptive, _, state) = queue(96_000);
    adaptive.netplay_budget.set_enabled(true);
    adaptive.configure_budget(config(96_000, 0, Duration::ZERO, 0), &state.preroll_samples);
    let capacity = ring_buffer_capacity_for_policy(96_000, AudioBufferPolicy::LowLatency);
    let (producer, _) = rtrb::RingBuffer::new(capacity);
    let mut explicit = NativeSampleQueue::new(producer);
    let mut adaptive_resampler = resampler::AudioResampler::new(48_000, 96_000).unwrap();
    let mut explicit_resampler = resampler::AudioResampler::new(48_000, 96_000).unwrap();
    for index in 0..60 {
        let occupancy = capacity / 2 + index % 7 * 2;
        adaptive.staged_samples.resize(occupancy, 0.25);
        explicit.staged_samples.resize(occupancy, 0.25);
        assert_eq!(adaptive.fill_ratio(false), explicit.fill_ratio(false));
        let input = vec![0.125; 1600];
        assert_eq!(
            adaptive_resampler.process(&input, adaptive.fill_ratio(false)),
            explicit_resampler.process(&input, explicit.fill_ratio(false))
        );
    }
    assert_eq!(
        adaptive.fill_ratio(true),
        adaptive.staged_samples.len() as f32 / 38_400.0
    );
    adaptive.netplay_budget.set_enabled(false);
    adaptive.configure_budget(config(96_000, 0, Duration::ZERO, 0), &state.preroll_samples);
    assert_eq!(adaptive.limit, 38_400);
    assert_eq!(adaptive.fill_ratio(false), adaptive.fill_ratio(true));
}
