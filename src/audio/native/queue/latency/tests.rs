use super::*;

fn simulate(rate: u32, hz: u32, policy: AudioBufferPolicy, block: usize) -> Value {
    let capacity = ring_buffer_capacity_for_policy(rate, policy);
    let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
    let mut queue = NativeSampleQueue::new(producer);
    let mut state = AudioPlaybackState::new(
        Arc::new(AtomicUsize::new(playback_preroll_samples(
            rate,
            policy.normal_queue_ms(),
        ))),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    let (mut observer, mut output, errors) = observer(rate, MARKERS + 1);
    let mut queued = Vec::new();
    let mut records = Vec::new();
    let mut next_input = 0;
    let mut callbacks = 0;
    let mut padded = false;
    while records.len() < MARKERS {
        let callback_at = frames_duration(callbacks * block, rate).unwrap();
        while next_input < MARKERS
            && Duration::from_nanos(next_input as u64 * 1_000_000_000 / u64::from(hz))
                <= callback_at
        {
            queued.push(
                StreamInstant::ZERO
                    .checked_add(Duration::from_nanos(
                        next_input as u64 * 1_000_000_000 / u64::from(hz),
                    ))
                    .unwrap(),
            );
            queue.staged_samples.extend(std::iter::repeat_n(
                (next_input + 1) as f32,
                marker_frames(next_input, rate, hz) * 2,
            ));
            next_input += 1;
        }
        if next_input == MARKERS && !padded {
            queue
                .staged_samples
                .extend(std::iter::repeat_n(0.0, capacity * 2));
            padded = true;
        }
        queue.pump();
        let callback = StreamInstant::ZERO.checked_add(callback_at).unwrap();
        let mut pcm = vec![0.0; block * 2];
        fill_output_f32(&mut pcm, 2, &mut consumer, &mut state);
        observer.observe(
            &pcm,
            2,
            OutputStreamTimestamp {
                callback,
                playback: callback.checked_add(Duration::from_millis(3)).unwrap(),
            },
        );
        while let Ok(span) = output.pop() {
            records.push(span);
        }
        callbacks += 1;
        assert!(callbacks <= rate as usize * 4 / block, "capture stalled");
    }
    assert_eq!(errors.load(Ordering::Relaxed), 0);
    assert_eq!(state.underruns.load(Ordering::Relaxed), 0);
    let report = summarize(&queued, &records, rate, hz, 30).unwrap();
    let latency = &report["queue_to_host_predicted_playback"];
    let preroll_ms = (policy.normal_queue_ms() / 2) as f64;
    let callback_ms = block as f64 / f64::from(rate) * 1000.0;
    let frame_ms = 1000.0 / f64::from(hz);
    assert!(
        latency["min_ms"].as_f64().unwrap() >= preroll_ms - frame_ms - callback_ms + 3.0,
        "{report}"
    );
    assert!(
        latency["max_ms"].as_f64().unwrap() <= preroll_ms + frame_ms + callback_ms + 3.0,
        "{report}"
    );
    report
}

#[test]
fn callback_queue_timing_preserves_every_marker_across_policies_rates_and_cadences() {
    for rate in [44_100, 48_000] {
        for hz in [50, 60, 75] {
            for policy in [
                AudioBufferPolicy::Auto,
                AudioBufferPolicy::LowLatency,
                AudioBufferPolicy::Balanced,
                AudioBufferPolicy::Stable,
            ] {
                for block in [128, 480, 1024] {
                    let report = simulate(rate, hz, policy, block);
                    assert_eq!(report["queue_to_callback_clock"]["samples"], 90);
                }
            }
        }
    }
}

#[test]
fn device_frame_offsets_and_callback_enqueue_overlap_are_preserved() {
    let rate = 48_000;
    let (mut observer, mut output, errors) = observer(rate, MARKERS);
    let mut queued = Vec::new();
    let mut records = Vec::new();
    for index in 0..MARKERS {
        let callback = StreamInstant::ZERO
            .checked_add(Duration::from_millis(index as u64 * 20 + 1))
            .unwrap();
        queued.push(callback.checked_add(Duration::from_micros(100)).unwrap());
        let pcm: Vec<_> = std::iter::repeat_n(0.0, 6)
            .chain(std::iter::repeat_n(
                (index + 1) as f32,
                marker_frames(index, rate, 60) * 6,
            ))
            .chain(std::iter::repeat_n(0.0, 6))
            .collect();
        observer.observe(
            &pcm,
            6,
            OutputStreamTimestamp {
                callback,
                playback: callback.checked_add(Duration::from_millis(3)).unwrap(),
            },
        );
        records.push(output.pop().unwrap());
    }
    assert_eq!(errors.load(Ordering::Relaxed), 0);
    let report = summarize(&queued, &records, rate, 60, 30).unwrap();
    assert_eq!(report["all_marker_callback_timestamp_overlaps"], 120);
    assert_eq!(report["queue_to_callback_clock"]["p50_ms"], -0.1);
    assert!(
        (report["queue_to_host_predicted_playback"]["p50_ms"]
            .as_f64()
            .unwrap()
            - (2.9 + 1000.0 / 48_000.0))
            .abs()
            < 1e-6
    );
    records[60].frames += 1;
    assert!(summarize(&queued, &records, rate, 60, 30).is_err());
    records[60].frames -= 1;
    records[60].id += 1;
    assert!(summarize(&queued, &records, rate, 60, 30).is_err());
    assert!(summarize(&queued, &records[..119], rate, 60, 30).is_err());
}

#[test]
fn malformed_pcm_clock_regression_and_record_overflow_are_detected() {
    let callback = StreamInstant::ZERO
        .checked_add(Duration::from_secs(1))
        .unwrap();
    let stamp = OutputStreamTimestamp {
        callback,
        playback: callback,
    };
    for pcm in [
        vec![f32::NAN; 2],
        vec![1.0, 2.0],
        vec![121.0; 2],
        vec![0.5; 2],
        vec![1.0],
    ] {
        let (mut observer, _, errors) = observer(48_000, 1);
        observer.observe(&pcm, 2, stamp);
        assert_ne!(errors.load(Ordering::Relaxed), 0);
    }
    let (mut observer, _output, errors) = observer(48_000, 1);
    observer.observe(&[1.0, 1.0, 2.0, 2.0, 0.0, 0.0], 2, stamp);
    assert_eq!(errors.load(Ordering::Relaxed), 1);
    observer.observe(
        &[0.0; 2],
        2,
        OutputStreamTimestamp {
            callback: StreamInstant::ZERO,
            playback: callback,
        },
    );
    assert_eq!(errors.load(Ordering::Relaxed), 2);
    assert!(frames_duration(1, 0).is_none());
    assert_eq!(
        frames_duration(480, 48_000),
        Some(Duration::from_millis(10))
    );
    if usize::BITS == 64 {
        assert!(frames_duration(usize::MAX, 48_000).is_none());
    }
}
