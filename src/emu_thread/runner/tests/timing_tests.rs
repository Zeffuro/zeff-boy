use super::*;

#[test]
fn request_timing_preserves_identity_across_a_blocked_send() {
    let session = crate::emu_thread::audio_timing::Session::start(std::time::Instant::now());
    let shared = crate::emu_thread::audio_timing::current();
    let (tx, rx) = crossbeam_channel::bounded(1);
    tx.send(empty_result()).unwrap();
    let drain = rx.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let sender = std::thread::spawn(move || {
        crate::emu_thread::audio_timing::install(shared);
        let mut result = empty_result();
        result.audio_timing_requests = vec![7];
        done_tx
            .send(EmuThread::send_frame(&tx, &drain, result, true))
            .unwrap();
    });
    let deadline = std::time::Instant::now();
    let mut observed = Vec::new();
    while !observed
        .iter()
        .any(|r: &crate::emu_thread::audio_timing::Record| r.stage == "send_begin")
    {
        observed.extend(session.take_records());
        assert!(deadline.elapsed() < std::time::Duration::from_secs(5));
        std::thread::yield_now();
    }
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err()
    );
    observed.extend(session.take_records());
    assert!(!observed.iter().any(|r| r.stage == "send_end"));
    rx.recv().unwrap();
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    );
    assert_eq!(rx.recv().unwrap().audio_timing_requests, [7]);
    sender.join().unwrap();
    observed.extend(session.finish());
    assert_eq!(
        observed
            .iter()
            .filter(|r| r.request == 7 && r.stage == "send_end" && r.aux == 1)
            .count(),
        1
    );
}

#[test]
fn request_timing_preserves_ids_and_pcm_through_backpressure_merge() {
    let session = crate::emu_thread::audio_timing::Session::start(std::time::Instant::now());
    let (tx, rx) = crossbeam_channel::bounded(1);
    let mut first = empty_result();
    first.audio_timing_requests = vec![1];
    first.completed_step_requests = 1;
    first.audio_samples = vec![1.0, 2.0];
    let mut second = empty_result();
    second.audio_timing_requests = vec![2];
    second.completed_step_requests = 1;
    second.audio_samples = vec![3.0, 4.0];
    assert!(EmuThread::send_frame(&tx, &rx, first, false));
    assert!(EmuThread::send_frame(&tx, &rx, second, false));
    let merged = rx.recv().unwrap();
    assert_eq!(merged.audio_timing_requests, [1, 2]);
    assert_eq!(merged.completed_step_requests, 2);
    assert_eq!(merged.audio_samples, [1.0, 2.0, 3.0, 4.0]);
    assert!(merged.delivery_merged);
    let records = session.finish();
    for id in [1, 2] {
        let record = records
            .iter()
            .find(|r| r.request == id && r.stage == "delivery_merged")
            .unwrap();
        assert_eq!((record.scalars, record.aux), (4, 2));
    }
}
