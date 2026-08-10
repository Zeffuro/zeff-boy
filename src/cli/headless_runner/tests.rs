use std::path::{Path, PathBuf};

use super::screenshots::screenshot_sequence_path;
use super::stuck::{StuckObservation, format_stuck_report};
use super::{AudioStats, HeadlessOptions, StuckReport, StuckTracker, fail_on_stuck_if_needed};

fn report(expected_wait: bool) -> StuckReport {
    StuckReport {
        frame: 120,
        window_frames: 60,
        unique_pcs: 1,
        framebuffer_changed: false,
        first_pc: 0x0800_1234,
        last_pc: 0x0800_1234,
        classification: expected_wait.then(|| "gba-swi-halt-idle".to_owned()),
        expected_wait,
    }
}

#[test]
fn stuck_report_formats_expected_gba_wait_as_idle() {
    let text = format_stuck_report("gba", &report(true), 8);

    assert!(text.contains("idle-detected"));
    assert!(text.contains("classification=gba-swi-halt-idle"));
}

#[test]
fn screenshot_sequence_path_uses_frame_number() {
    assert_eq!(
        screenshot_sequence_path(Path::new("shots"), 42),
        PathBuf::from("shots").join("frame_000042.png")
    );
}

#[test]
fn audio_stats_track_nonzero_samples_and_peak() {
    let mut stats = AudioStats::default();
    stats.observe(&[]);
    stats.observe(&[0.0, 0.25, -0.5, 0.000_000_1]);

    assert_eq!(stats.frames_with_samples, 1);
    assert_eq!(stats.sample_count, 4);
    assert_eq!(stats.nonzero_samples, 2);
    assert_eq!(stats.peak_abs, 0.5);
    assert!(stats.mean_abs() > 0.18);
}

fn stuck_observation<'a>(
    frame: u64,
    pc: u64,
    framebuffer: &'a [u8],
    progress_marker: Option<u64>,
) -> StuckObservation<'a> {
    StuckObservation {
        frame,
        pc,
        framebuffer,
        progress_marker,
        classification: None,
        expected_wait: false,
    }
}

#[test]
fn stuck_tracker_ignores_low_pc_windows_with_progress_markers() {
    let mut tracker = StuckTracker::from_options(&HeadlessOptions {
        stuck_window_frames: 3,
        stuck_pc_threshold: 1,
        ..HeadlessOptions::default()
    })
    .unwrap();
    let framebuffer = [0u8; 16];

    assert!(
        tracker
            .observe(stuck_observation(1, 0x40032, &framebuffer, Some(1)))
            .is_none()
    );
    assert!(
        tracker
            .observe(stuck_observation(2, 0x40032, &framebuffer, Some(2)))
            .is_none()
    );
    assert!(
        tracker
            .observe(stuck_observation(3, 0x40032, &framebuffer, Some(3)))
            .is_none()
    );
}

#[test]
fn stuck_tracker_flags_low_pc_windows_without_marker_progress() {
    let mut tracker = StuckTracker::from_options(&HeadlessOptions {
        stuck_window_frames: 3,
        stuck_pc_threshold: 1,
        ..HeadlessOptions::default()
    })
    .unwrap();
    let framebuffer = [0u8; 16];

    tracker.observe(stuck_observation(1, 0x40032, &framebuffer, Some(9)));
    tracker.observe(stuck_observation(2, 0x40032, &framebuffer, Some(9)));
    let report = tracker
        .observe(stuck_observation(3, 0x40032, &framebuffer, Some(9)))
        .unwrap();

    assert_eq!(report.unique_pcs, 1);
    assert!(!report.framebuffer_changed);
    assert!(!report.expected_wait);
}

#[test]
fn fail_on_stuck_ignores_expected_waits() {
    let mut opts = HeadlessOptions {
        fail_on_stuck: true,
        ..HeadlessOptions::default()
    };
    let mut tracker = StuckTracker::from_options(&HeadlessOptions {
        stuck_window_frames: 1,
        ..HeadlessOptions::default()
    })
    .unwrap();
    tracker.current_report = Some(report(true));

    assert!(fail_on_stuck_if_needed("gba", Some(&tracker), &opts).is_ok());

    opts.fail_on_stuck = true;
    tracker.current_report = Some(report(false));
    assert!(fail_on_stuck_if_needed("gba", Some(&tracker), &opts).is_err());
}
