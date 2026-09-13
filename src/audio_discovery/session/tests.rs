use super::*;
use crate::audio_discovery::ScanStatus;
use crate::audio_discovery::test_support::gba_fixture;
use zeff_emu_common::system::System;

fn input(marker: u8) -> Arc<ScanInput> {
    let mut bytes = gba_fixture();
    bytes[0x310] = marker;
    Arc::new(ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "loaded-effective-rom-v1",
        display_name: None,
    })
}

fn delayed(
    session: &mut DiscoverySession,
    source: Arc<ScanInput>,
) -> mpsc::SyncSender<ScanManifest> {
    session.bind_source(Some(source.clone()));
    let (sender, receiver) = mpsc::sync_channel(1);
    session.pending = Some(PendingScan {
        source,
        cancel: Arc::new(AtomicBool::new(false)),
        receiver,
    });
    sender
}

fn settle(session: &mut DiscoverySession) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.is_busy() {
        session.poll();
        assert!(std::time::Instant::now() < deadline, "scan did not finish");
        std::thread::yield_now();
    }
}

#[test]
fn background_scan_uses_the_same_manifest_and_parser_as_direct_analysis() {
    let source = input(7);
    let expected = source.analyze(ScanLimits::default(), &AtomicBool::new(false));
    let mut session = DiscoverySession::default();
    session.bind_source(Some(source.clone()));
    session.start();
    settle(&mut session);
    let manifest = session.manifest.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(manifest).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(manifest.scan.status, ScanStatus::Complete);
    assert_eq!(manifest.scan.candidates.len(), 1);
    assert!(manifest.source.is_none());
    assert!(manifest.transforms.is_none());
    assert!(Arc::ptr_eq(session.source.as_ref().unwrap(), &source));
}

#[test]
fn replacement_scan_is_accepted_while_old_job_cancels_and_cannot_publish_old_result() {
    let old = input(1);
    let next = input(2);
    let old_manifest = old.analyze(ScanLimits::default(), &AtomicBool::new(false));
    let old_bytes = Arc::downgrade(&old.bytes);
    let mut session = DiscoverySession::default();
    let sender = delayed(&mut session, old.clone());
    let cancel = session.pending.as_ref().unwrap().cancel.clone();
    drop(old);
    session.bind_source(Some(next.clone()));
    assert!(cancel.load(Ordering::Relaxed));
    assert!(session.can_start());
    session.start();
    assert!(session.is_queued());
    assert!(!session.can_start());
    sender.send(old_manifest).unwrap();
    session.poll();
    assert!(session.manifest.is_none());
    assert!(old_bytes.upgrade().is_none());
    settle(&mut session);
    assert_eq!(
        session
            .manifest
            .as_ref()
            .unwrap()
            .scan
            .media
            .sha256
            .as_deref(),
        Some(zeff_firmware::sha256_hex(&next.bytes).as_str())
    );
    assert!(session.error.is_none());
}

#[test]
fn identical_bytes_in_a_new_worker_still_invalidate_the_previous_session() {
    let source = input(3);
    let mut session = DiscoverySession::default();
    session.bind_source(Some(source.clone()));
    session.start();
    settle(&mut session);
    assert!(session.manifest.is_some());
    session.bind_source(Some(Arc::new(ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: source.system,
        standalone_audio: source.standalone_audio,
        bytes: source.bytes.clone(),
        provenance: source.provenance.clone(),
        analysis_profile: source.analysis_profile,
        display_name: source.display_name.clone(),
    })));
    assert!(session.manifest.is_none());
    assert!(session.can_start());
}

#[test]
fn cancel_wins_against_a_completed_but_unpublished_result() {
    let source = input(4);
    let mut session = DiscoverySession::default();
    let sender = delayed(&mut session, source.clone());
    sender
        .send(source.analyze(ScanLimits::default(), &AtomicBool::new(false)))
        .unwrap();
    session.cancel();
    session.poll();
    assert!(session.manifest.is_none());
    assert!(session.cancelled);
    assert!(session.can_start());
}

#[test]
fn stop_clears_queued_work_and_drops_old_media_after_cancellation() {
    let old = input(5);
    let weak = Arc::downgrade(&old.bytes);
    let mut session = DiscoverySession::default();
    let sender = delayed(&mut session, old.clone());
    drop(old);
    session.bind_source(Some(input(6)));
    session.start();
    session.bind_source(None);
    drop(sender);
    session.poll();
    assert!(!session.is_busy());
    assert!(!session.can_start());
    assert!(session.source.is_none());
    assert!(session.manifest.is_none());
    assert!(session.error.is_none());
    assert!(weak.upgrade().is_none());
}

#[test]
fn worker_failure_allows_retry_and_dropping_the_session_cancels_work() {
    let mut session = DiscoverySession::default();
    let sender = delayed(&mut session, input(8));
    drop(sender);
    session.poll();
    assert!(session.error.is_some());
    assert!(session.can_start());
    let _sender = delayed(&mut session, input(9));
    let cancel = session.pending.as_ref().unwrap().cancel.clone();
    drop(session);
    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn queued_scan_retains_the_limits_selected_when_it_was_requested() {
    let old = input(1);
    let mut session = DiscoverySession::default();
    let sender = delayed(&mut session, old.clone());
    session.bind_source(Some(input(2)));
    session.limits = ScanLimits {
        max_candidates: 300,
        max_work: 1,
    };
    session.start();
    session.limits = ScanLimits::default();
    sender
        .send(old.analyze(ScanLimits::default(), &AtomicBool::new(false)))
        .unwrap();
    settle(&mut session);
    let report = &session.manifest.as_ref().unwrap().scan;
    assert_eq!(
        report.limits,
        ScanLimits {
            max_candidates: 300,
            max_work: 1
        }
    );
    assert_eq!(
        report.status,
        ScanStatus::Incomplete(crate::audio_discovery::ScanStop::WorkLimit)
    );
}
