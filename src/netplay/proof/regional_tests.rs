use super::*;
use zeff_nes_core::hardware::cartridge::TimingMode;

#[test]
fn pal_real_workers_match_delayed_reference_pause_and_restore_without_session_saves() {
    assert_regional_workers(TimingMode::Pal, "pal");
}

#[test]
fn dendy_real_workers_match_delayed_reference_pause_and_restore_without_session_saves() {
    assert_regional_workers(TimingMode::Dendy, "dendy");
}

fn assert_regional_workers(timing: TimingMode, label: &str) {
    let directory = crate::test_support::test_directory("netplay-pal-worker-proof").unwrap();
    let report = run_media(
        directory.path(),
        24,
        [9; 32],
        &media::Media::fixture_timing(timing),
    )
    .unwrap();
    assert_eq!(report["frames"], 24);
    assert_eq!(report["resolved_timing"], label);
    assert_eq!(report["compatibility_contract"], "00".repeat(32));
    assert_eq!(report["pause_rounds"], 9);
    assert_eq!(report["battery"], true);
    assert_eq!(report["persistent_changed"], true);
    assert_eq!(report["delayed_reference"], true);
    assert_eq!(report["exact_restore"], true);
    assert_eq!(report["save_protection"], true);
}

#[test]
fn pal_native_divergence_and_disconnect_restore_publication_after_asynchronous_failure() {
    assert_regional_failure(TimingMode::Pal);
}

#[test]
fn dendy_native_divergence_and_disconnect_restore_publication_after_asynchronous_failure() {
    assert_regional_failure(TimingMode::Dendy);
}

fn assert_regional_failure(timing: TimingMode) {
    let media = media::Media::fixture_timing(timing);
    for fault in [
        fault_tests::Fault::NativeDivergence,
        fault_tests::Fault::Disconnect,
    ] {
        fault_tests::run_fault(fault, &media, true);
    }
}
