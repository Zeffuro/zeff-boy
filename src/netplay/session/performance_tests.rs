use super::*;
use std::hint::black_box;
use zeff_netplay::rollback::InputDelay;

mod fixtures;
use fixtures::{CASES, Case};

const BASE: u64 = checks::HASH_INTERVAL - PREDICTION_WINDOW;

#[derive(Default)]
struct Samples {
    advance: Vec<f64>,
    capture: Vec<f64>,
    restore: Vec<f64>,
    checkpoint: Vec<f64>,
    replay: Vec<f64>,
}

fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000.0
}

fn summary(samples: &[f64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |p: usize| sorted[((sorted.len() - 1) * p).div_ceil(100)];
    serde_json::json!({
        "samples": sorted.len(), "p50_us": percentile(50),
        "p95_us": percentile(95), "p99_us": percentile(99),
    })
}

fn ports(frame: u64, mask: u16) -> [u16; 2] {
    [
        (frame.wrapping_mul(17).wrapping_add(1) as u16) & mask,
        (frame.wrapping_mul(29).wrapping_add(0x105) as u16) & mask,
    ]
}

fn checkpoint(
    backend: &EmuBackend,
    snapshot: &Snapshot,
    audio: &[f32],
    config: [u8; 32],
) -> Message {
    identity::checkpoint_with_snapshot(
        backend,
        snapshot.frame(),
        audio,
        config,
        snapshot.pce(),
        snapshot.ws(),
    )
    .unwrap()
}

fn measure(case: Case, rounds: usize) -> serde_json::Value {
    let label = case.label();
    let (_directory, mut backend) = case.load();
    let (_reference_directory, mut reference) = case.load();
    let admission =
        identity::identity_with_delay(&backend, [7; 32], InputDelay::default()).unwrap();
    assert_eq!(
        admission,
        identity::identity_with_delay(&reference, [7; 32], InputDelay::default()).unwrap()
    );
    let original_state = backend.encode_state_bytes().unwrap();
    let original_persistent = identity::persistent_hash(&backend).unwrap();
    let publication = core::persistence(&mut backend, None).unwrap();
    let mut lease = Lease::begin(&mut backend, case.player()).unwrap();
    let original = lease.capture(&backend).unwrap();
    core::persistence(&mut backend, Some(false)).unwrap();
    core::persistence(&mut reference, Some(false)).unwrap();
    let mut reference_lease = Lease::begin(&mut reference, case.player()).unwrap();
    let mask = lease.input_mask();
    for frame in 0..BASE {
        let input = ports(frame, mask);
        let actual = lease.advance(&mut backend, input).unwrap();
        let expected = reference_lease.advance(&mut reference, input).unwrap();
        assert_eq!(actual, expected, "warmup {label}/{frame}");
    }
    let base = lease.capture(&backend).unwrap();
    assert_eq!(base.frame(), BASE);
    let mut expected = Vec::new();
    for frame in BASE..checks::HASH_INTERVAL {
        let audio = reference_lease
            .advance(&mut reference, ports(frame, mask))
            .unwrap();
        let snapshot = reference_lease.capture(&reference).unwrap();
        expected.push((
            checkpoint(&reference, &snapshot, &audio, admission.config),
            audio
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
        ));
    }
    let expected_state = reference.encode_state_bytes().unwrap();
    let expected_persistent = identity::persistent_hash(&reference).unwrap();
    let continuation = reference_lease
        .advance(&mut reference, ports(checks::HASH_INTERVAL, mask))
        .unwrap();
    let continued_snapshot = reference_lease.capture(&reference).unwrap();
    expected.push((
        checkpoint(
            &reference,
            &continued_snapshot,
            &continuation,
            admission.config,
        ),
        continuation.iter().map(|sample| sample.to_bits()).collect(),
    ));
    let mut samples = Samples::default();
    let mut max_retained = 0;
    for _ in 0..rounds {
        lease.restore(&mut backend, &base).unwrap();
        let mut window = Vec::with_capacity(PREDICTION_WINDOW as usize);
        for _ in 0..PREDICTION_WINDOW {
            lease.advance(&mut backend, [0; 2]).unwrap();
            window.push(lease.capture(&backend).unwrap());
        }
        let replay_started = Instant::now();
        let restore_started = Instant::now();
        lease.restore(&mut backend, &base).unwrap();
        samples.restore.push(micros(restore_started));
        window.clear();
        let mut pcm = Vec::with_capacity(PREDICTION_WINDOW as usize);
        for frame in BASE..checks::HASH_INTERVAL {
            let advance_started = Instant::now();
            let audio = lease.advance(&mut backend, ports(frame, mask)).unwrap();
            samples.advance.push(micros(advance_started));
            let capture_started = Instant::now();
            let snapshot = lease.capture(&backend).unwrap();
            samples.capture.push(micros(capture_started));
            if snapshot.frame() == checks::HASH_INTERVAL {
                let hash_started = Instant::now();
                black_box(checkpoint(&backend, &snapshot, &audio, admission.config));
                samples.checkpoint.push(micros(hash_started));
            }
            window.push(snapshot);
            pcm.push(audio);
        }
        samples.replay.push(micros(replay_started));
        assert_eq!(window.len(), PREDICTION_WINDOW as usize);
        let owned = |state: &Snapshot| state.retained_bytes() - state.shared_media_bytes();
        let retained = original.retained_bytes()
            + owned(&base)
            + window.iter().map(owned).sum::<usize>()
            + pcm
                .iter()
                .map(|audio| audio.capacity() * size_of::<f32>())
                .sum::<usize>();
        max_retained = max_retained.max(retained);
        assert_eq!(
            backend.encode_state_bytes().unwrap(),
            expected_state,
            "{label}"
        );
        assert_eq!(
            identity::persistent_hash(&backend).unwrap(),
            expected_persistent
        );
        for (index, (state, audio)) in window.iter().zip(&pcm).enumerate() {
            lease.restore(&mut backend, state).unwrap();
            let restored = lease.capture(&backend).unwrap();
            assert_eq!(
                audio
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>(),
                expected[index].1,
                "PCM {label}/{index}"
            );
            assert_eq!(
                checkpoint(&backend, &restored, audio, admission.config),
                expected[index].0,
                "checkpoint {label}/{index}"
            );
            let continued = lease
                .advance(&mut backend, ports(state.frame(), mask))
                .unwrap();
            let continued_state = lease.capture(&backend).unwrap();
            assert_eq!(
                continued
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>(),
                expected[index + 1].1,
                "continued PCM {label}/{index}"
            );
            assert_eq!(
                checkpoint(&backend, &continued_state, &continued, admission.config),
                expected[index + 1].0,
                "continued checkpoint {label}/{index}"
            );
        }
        assert!(!core::persistence(&mut backend, None).unwrap());
        assert!(backend.flush_battery_sram().unwrap().is_none());
    }
    lease
        .restore_checkpoint(&mut backend, &original, original_state.clone())
        .unwrap();
    core::persistence(&mut backend, Some(publication)).unwrap();
    assert_eq!(backend.encode_state_bytes().unwrap(), original_state);
    assert_eq!(
        identity::persistent_hash(&backend).unwrap(),
        original_persistent
    );
    drop(lease);
    let (_offline_directory, mut offline) = case.load();
    core::persistence(&mut backend, Some(false)).unwrap();
    core::persistence(&mut offline, Some(false)).unwrap();
    backend.step_frame();
    offline.step_frame();
    assert_eq!(backend.frame_count(), 1, "offline advance {label}");
    assert_eq!(offline.frame_count(), 1, "reference advance {label}");
    let mut actual_pcm = Vec::new();
    let mut expected_pcm = Vec::new();
    backend.drain_audio_samples_into(&mut actual_pcm);
    offline.drain_audio_samples_into(&mut expected_pcm);
    assert_eq!(
        actual_pcm
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        expected_pcm
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        "offline continuation {label}"
    );
    assert_eq!(
        backend.encode_state_bytes().unwrap(),
        offline.encode_state_bytes().unwrap(),
        "offline state {label}"
    );
    assert_eq!(
        identity::persistent_hash(&backend).unwrap(),
        identity::persistent_hash(&offline).unwrap()
    );
    serde_json::json!({
        "scope": "rollback_lease_operations", "fixture": label,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "rounds": rounds, "replay_frames": PREDICTION_WINDOW, "warmup_frames": BASE,
        "restore_snapshot_retained_bytes": original.retained_bytes(),
        "reported_snapshots_and_pcm_bytes": max_retained,
        "advance": summary(&samples.advance), "capture": summary(&samples.capture),
        "restore": summary(&samples.restore), "checkpoint": summary(&samples.checkpoint),
        "restore_replay_capture_checkpoint": summary(&samples.replay),
    })
}

#[test]
fn rollback_cost_fixture_replays_exact_pcm_checkpoints_and_restores_all_adapters() {
    for case in CASES {
        measure(case, 1);
    }
}

#[test]
#[ignore = "rollback cost baseline, run alone with --ignored --nocapture --test-threads=1"]
fn rollback_cost_matrix() {
    for case in CASES {
        println!("{}", measure(case, 8));
    }
}
