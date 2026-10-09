use super::*;

fn trace(delay: u64) -> (Trace, Vec<(u64, Instant)>) {
    let mut trace = Trace::new(120, InputDelay::new(delay).unwrap()).unwrap();
    let start = Instant::now();
    let mut presented = Vec::new();
    for frame in 0..120 {
        let at = start + Duration::from_millis(frame * 20);
        trace.record_input(frame, at).unwrap();
        trace
            .record_confirmed(frame + 1, at + Duration::from_millis(2))
            .unwrap();
        if frame >= 2 {
            trace
                .record_handoff(frame + 1, at + Duration::from_millis(3))
                .unwrap();
        }
        presented.push((frame + 1, at + Duration::from_millis(4)));
    }
    (trace, presented)
}

#[test]
fn delayed_effect_pairing_excludes_input_warmup_and_unexecuted_tail() {
    for delay in [0, 2, 3, 8] {
        let (trace, presented) = trace(delay);
        let report = trace.report(&presented, 60).unwrap();
        assert_eq!(report["paired_samples"], 60 - delay);
        assert_eq!(report["tail_input_samples_without_effect"], delay);
        for (key, extra) in [
            ("input_to_predicted_response", 4),
            ("input_to_confirmed_pcm", 2),
            ("input_to_audio_handoff", 3),
        ] {
            assert_eq!(report[key]["samples"], 60 - delay);
            for percentile in ["p50_ms", "p95_ms", "p99_ms", "max_ms"] {
                assert_eq!(report[key][percentile], (delay * 20 + extra) as f64);
            }
        }
        assert_eq!(report["confirmed_pcm_to_audio_handoff"]["p99_ms"], 1.0);
        let startup = trace.report(&presented, 0).unwrap();
        assert_eq!(
            startup["confirmed_pcm_to_audio_handoff"]["max_ms"],
            if delay == 0 { 41.0 } else { 1.0 }
        );
    }
}

#[test]
fn nearest_rank_quantiles_cover_a_variable_response_tail() {
    let (trace, mut presented) = trace(2);
    for input in 60..118 {
        presented[input + 2].1 =
            trace.inputs[input] + Duration::from_millis(40 + (input as u64 - 59));
    }
    let report = trace.report(&presented, 60).unwrap();
    let predicted = &report["input_to_predicted_response"];
    assert_eq!(predicted["p50_ms"], 69.0);
    assert_eq!(predicted["p95_ms"], 96.0);
    assert_eq!(predicted["p99_ms"], 98.0);
    assert_eq!(predicted["max_ms"], 98.0);
}

#[test]
fn stalled_retry_preserves_first_enqueue_and_counts_wait() {
    let start = Instant::now();
    let mut trace = Trace::new(1, InputDelay::new(0).unwrap()).unwrap();
    trace.record_input(0, start).unwrap();
    trace
        .record_input(0, start + Duration::from_millis(40))
        .unwrap();
    trace
        .record_confirmed(1, start + Duration::from_millis(42))
        .unwrap();
    trace
        .record_handoff(1, start + Duration::from_millis(43))
        .unwrap();
    let report = trace
        .report(&[(1, start + Duration::from_millis(44))], 0)
        .unwrap();
    assert_eq!(report["input_to_predicted_response"]["p50_ms"], 44.0);
    assert_eq!(report["input_to_confirmed_pcm"]["p50_ms"], 42.0);
}

#[test]
fn invalid_events_fail_before_growth_and_keep_first_error() {
    let at = Instant::now();
    for frame in [1, 1000, u64::MAX] {
        let mut trace = Trace::new(1, InputDelay::default()).unwrap();
        let first = trace.record_input(frame, at).unwrap_err().to_string();
        assert!(trace.inputs.is_empty());
        assert_eq!(trace.record_input(0, at).unwrap_err().to_string(), first);
        assert_eq!(
            trace.record_confirmed(1, at).unwrap_err().to_string(),
            first
        );
    }
    for frame in [0, 2, u64::MAX] {
        let mut trace = Trace::new(1, InputDelay::default()).unwrap();
        assert!(trace.record_confirmed(frame, at).is_err());
        assert!(trace.confirmed.is_empty());
    }
    let mut trace = Trace::new(2, InputDelay::default()).unwrap();
    trace.record_input(0, at).unwrap();
    assert!(
        trace
            .record_input(1, at - Duration::from_millis(1))
            .is_err()
    );
    assert_eq!(trace.inputs.len(), 1);
    let mut trace = Trace::new(2, InputDelay::default()).unwrap();
    trace.record_confirmed(1, at).unwrap();
    assert!(trace.record_handoff(2, at).is_err());
    assert!(trace.handed_off.is_empty());
}

#[test]
fn missing_or_regressing_effect_observations_cannot_produce_report() {
    let (mut trace, mut presented) = trace(2);
    let saved = presented[63];
    presented[63].0 += 1;
    assert!(trace.report(&presented, 60).is_err());
    presented[63] = saved;
    presented[63].1 = presented[62].1 - Duration::from_millis(1);
    assert!(trace.report(&presented, 60).is_err());
    presented[63] = saved;
    assert!(trace.report(&presented[..119], 60).is_err());
    assert!(trace.report(&presented, 118).is_err());
    trace.inputs[0] = presented[2].1 + Duration::from_millis(1);
    assert!(trace.report(&presented, 60).is_err());
    trace.inputs.pop();
    assert!(trace.report(&presented, 60).is_err());
}

#[test]
fn bounds_duplicates_and_equal_clock_observations_are_explicit() {
    assert!(Trace::new(0, InputDelay::default()).is_err());
    assert!(Trace::new(1001, InputDelay::default()).is_err());
    let at = Instant::now();
    let mut trace = Trace::new(1000, InputDelay::new(0).unwrap()).unwrap();
    for frame in 0..1000 {
        trace.record_input(frame, at).unwrap();
        trace.record_confirmed(frame + 1, at).unwrap();
        trace.record_handoff(frame + 1, at).unwrap();
    }
    let presented: Vec<_> = (1..=1000).map(|frame| (frame, at)).collect();
    assert_eq!(
        trace.report(&presented, 60).unwrap()["input_to_confirmed_pcm"]["max_ms"],
        0.0
    );
    assert!(trace.record_handoff(1000, at).is_err());
    assert_eq!(trace.handed_off.len(), 1000);
}
