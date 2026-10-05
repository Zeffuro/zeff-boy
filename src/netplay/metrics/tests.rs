use super::*;

#[test]
fn delayed_ack_duplicate_and_stale_ack_preserve_sample() {
    let now = Instant::now();
    let mut metrics = Metrics::direct();
    metrics.input_attempt([2, 3].into_iter(), now);
    metrics.acknowledge(4, now + Duration::from_millis(90));
    metrics.acknowledge(4, now + Duration::from_millis(120));
    metrics.acknowledge(3, now + Duration::from_millis(150));
    let stats = metrics.snapshot(now + Duration::from_millis(190));
    assert_eq!(stats.ack_wait, Some(Duration::from_millis(90)));
    assert_eq!(stats.ack_age, Some(Duration::from_millis(100)));
    assert_eq!(stats.ack_samples, 1);
    assert!(metrics.flights.is_empty());
}

#[test]
fn repeated_and_coalesced_ack_measures_oldest_first_attempt_once() {
    let now = Instant::now();
    let mut metrics = Metrics::direct();
    metrics.input_attempt([2].into_iter(), now);
    metrics.input_attempt([2, 3].into_iter(), now + Duration::from_millis(25));
    metrics.acknowledge(4, now + Duration::from_millis(200));
    assert_eq!(metrics.snapshot(now).ack_samples, 1);
    assert_eq!(
        metrics.snapshot(now).ack_wait,
        Some(Duration::from_millis(200))
    );
    assert_eq!(metrics.snapshot(now).repeated_batches, 1);
    metrics.input_attempt([4].into_iter(), now + Duration::from_millis(205));
    metrics.acknowledge(5, now + Duration::from_millis(220));
    assert_eq!(
        metrics.snapshot(now).ack_wait,
        Some(Duration::from_millis(15))
    );
    assert_eq!(metrics.snapshot(now).ack_samples, 2);
}

#[test]
fn history_and_counters_are_bounded() {
    let now = Instant::now();
    let mut metrics = Metrics::direct();
    metrics.input_attempt(0..200, now);
    assert_eq!(metrics.flights.len(), 64);
    assert_eq!(*metrics.flights.first_key_value().unwrap().0, 136);
    metrics.acknowledge(200, now);
    metrics.input_attempt(0..200, now);
    assert!(metrics.flights.is_empty());
    metrics.stats.sent = u64::MAX;
    metrics.stats.received = u64::MAX;
    metrics.stats.sent_bytes = u64::MAX;
    metrics.stats.received_bytes = u64::MAX;
    metrics.sent(10);
    metrics.received(20);
    assert_eq!(metrics.stats.sent, u64::MAX);
    assert_eq!(metrics.stats.received, u64::MAX);
    assert_eq!(metrics.stats.sent_bytes, u64::MAX);
    assert_eq!(metrics.stats.received_bytes, u64::MAX);
}
