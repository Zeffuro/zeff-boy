use super::{App, EmuBackend, EmuCommand, Observation, Options, hold, media, sample, set_input};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const WARMUP: u64 = 60;
const MIN_SAMPLES: u64 = 60;
const POLL: Duration = Duration::from_millis(1);

pub(super) mod ledger;
mod oracle;

pub(super) fn play(
    app: &mut App,
    options: &Options,
    path: &Path,
    save: &Option<Vec<u8>>,
    reference: &mut EmuBackend,
    config: [u8; 32],
) -> Result<Value> {
    ensure!(
        options.frames >= WARMUP + MIN_SAMPLES,
        "cadence needs at least 120 frames"
    );
    ensure!(
        options.jitter_ms == 0 && options.fault.is_none() && !options.reject_build,
        "cadence requires zero proof jitter and no fault or rejection mode"
    );
    ensure!(
        app.netplay.running() && app.netplay.presented == 0 && !app.netplay.in_flight,
        "cadence requires a fresh admitted App session"
    );
    ledger(app)?.check_error()?;
    let nominal = Duration::from_nanos(app.nominal_frame_duration_ns());
    ensure!(!nominal.is_zero(), "invalid regional frame duration");
    let budget = nominal
        .checked_mul(options.frames.try_into()?)
        .context("cadence duration overflow")?
        + Duration::from_secs(10);
    let started = Instant::now();
    let deadline = started + budget;
    let mut fences_checked = false;
    app.netplay.next_frame = None;
    loop {
        app.drain_emu_responses();
        ledger(app)?.check_error()?;
        ensure!(
            app.netplay.running(),
            "cadence session stopped: {}",
            app.debug_windows.netplay.status
        );
        let frame = app.netplay.presented;
        ensure!(frame <= options.frames, "cadence submitted beyond target");
        if frame == options.frames {
            ensure!(!app.netplay.in_flight, "cadence target has a pending step");
            hold(app);
            break;
        }
        set_input(app, sample(frame, options.role));
        app.game_window_focused = options.role != 0 || !(5..8).contains(&frame);
        app.game_view_focused = options.role != 1 || !(9..12).contains(&frame);
        app.egui_wants_keyboard = options.role == 1 && (12..15).contains(&frame);
        if frame == 5 && !fences_checked {
            ensure!(
                app.send_emu_command_checked(EmuCommand::Reset).is_err(),
                "reset escaped App fence"
            );
            ensure!(
                app.send_emu_command_checked(EmuCommand::SetSampleRate(44_100))
                    .is_err(),
                "sample rate escaped App fence"
            );
            fences_checked = true;
        }
        app.pump_netplay();
        ensure!(
            Instant::now() < deadline,
            "cadence presentation deadline: {}",
            app.debug_windows.netplay.status
        );
        let sleep = app.netplay.deadline().map_or(POLL, |next| {
            next.saturating_duration_since(Instant::now()).min(POLL)
        });
        if !sleep.is_zero() {
            std::thread::sleep(sleep);
        }
    }
    let presented_elapsed = started.elapsed();
    let confirmation_started = Instant::now();
    let confirmation_deadline = confirmation_started + Duration::from_secs(5);
    loop {
        app.drain_emu_responses();
        ledger(app)?.check_error()?;
        ensure!(
            app.netplay.running(),
            "cadence confirmation stopped: {}",
            app.debug_windows.netplay.status
        );
        let confirmed = observation(app)?.frames;
        ensure!(
            confirmed <= options.frames,
            "cadence confirmed beyond target"
        );
        if confirmed == options.frames && app.netplay.published_confirmed >= options.frames {
            break;
        }
        ensure!(
            Instant::now() < confirmation_deadline,
            "cadence confirmation deadline"
        );
        std::thread::sleep(POLL);
    }
    let observed = observation(app)?;
    let confirmation_tail = confirmation_started.elapsed();
    ensure!(fences_checked, "cadence mutation fences were not checked");
    let reference_started = Instant::now();
    oracle::verify(app, options, reference, config, ledger(app)?)?;
    ensure!(
        media::optional_save(path)? == *save,
        "cadence published save bytes"
    );
    let reference_elapsed = reference_started.elapsed();
    ensure!(
        observed.presented.len() as u64 == options.frames,
        "cadence presentation observations missing"
    );
    let mut report = intervals(&observed.presented, nominal)?;
    report["presented_frames"] = json!(options.frames);
    report["confirmed_frames"] = json!(observed.frames);
    report["presentation_run_seconds"] = json!(presented_elapsed.as_secs_f64());
    report["confirmation_tail_ms"] = json!(confirmation_tail.as_secs_f64() * 1000.0);
    report["verification"] = json!({
        "mode": "offline reference after nominal presentation and confirmation tail",
        "reference_checked_frames": observed.frames,
        "submitted_samples": ledger(app)?.inputs().len(),
        "reference_seconds": reference_elapsed.as_secs_f64(),
        "timing_includes": "per-frame verification hashing and bounded input/PCM recording",
        "timing_excludes": "offline reference execution",
    });
    report["max_prediction_depth"] = json!(observed.depth_max);
    report["rollback_frames"] = json!(observed.rollback_frames);
    report["stalls"] = json!(observed.stalls);
    report["retained_payload_max_bytes"] = json!(observed.retained_payload_max);
    report["device_audio_underflows"] = json!("not measured (headless muted)");
    Ok(report)
}

fn observation(app: &App) -> Result<&Observation> {
    app.netplay.proof.as_ref().context("cadence observer lost")
}

fn ledger(app: &App) -> Result<&ledger::Ledger> {
    observation(app)?
        .ledger
        .as_ref()
        .context("cadence ledger is disabled")
}

fn intervals(points: &[(u64, Instant)], nominal: Duration) -> Result<Value> {
    ensure!(
        points.len() as u64 >= WARMUP + MIN_SAMPLES,
        "insufficient cadence observations"
    );
    for (index, (frame, _)) in points.iter().enumerate() {
        ensure!(*frame == index as u64 + 1, "cadence presentation frame gap");
    }
    let measured = &points[WARMUP as usize - 1..];
    let mut durations = Vec::with_capacity(measured.len() - 1);
    for pair in points.windows(2) {
        ensure!(pair[1].1 > pair[0].1, "cadence timestamps did not advance");
    }
    for pair in measured.windows(2) {
        durations.push(pair[1].1.duration_since(pair[0].1).as_secs_f64() * 1000.0);
    }
    let seconds = measured
        .last()
        .context("missing cadence end")?
        .1
        .duration_since(measured[0].1)
        .as_secs_f64();
    let fps = durations.len() as f64 / seconds;
    durations.sort_by(f64::total_cmp);
    Ok(json!({
        "timing_boundary": "advanced presentation responses consumed by headless App",
        "warmup_presented_frames": WARMUP,
        "measured_intervals": durations.len(),
        "nominal_frame_ms": nominal.as_secs_f64() * 1000.0,
        "nominal_fps": 1.0 / nominal.as_secs_f64(),
        "interval_quantile": "nearest rank",
        "interval_p50_ms": quantile(&durations, 50),
        "interval_p95_ms": quantile(&durations, 95),
        "interval_p99_ms": quantile(&durations, 99),
        "effective_fps": fps,
        "measured_seconds": seconds,
    }))
}

fn quantile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(sorted.len() * percent).div_ceil(100) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_is_excluded_and_quantiles_use_adjacent_frame_intervals() {
        let start = Instant::now();
        let mut at = start;
        let points: Vec<_> = (1..=120)
            .map(|frame| {
                at += Duration::from_millis(if frame <= 60 { 100 } else { frame - 60 });
                (frame, at)
            })
            .collect();
        let report = intervals(&points, Duration::from_millis(20)).unwrap();
        assert_eq!(report["measured_intervals"], 60);
        assert_eq!(report["interval_p50_ms"], 30.0);
        assert_eq!(report["interval_p95_ms"], 57.0);
        assert_eq!(report["interval_p99_ms"], 60.0);
        assert!((report["effective_fps"].as_f64().unwrap() - 60.0 / 1.830).abs() < 1e-9);
    }

    #[test]
    fn missing_frames_and_nonmonotonic_observations_are_refused() {
        let start = Instant::now();
        let mut points: Vec<_> = (1..=120)
            .map(|frame| (frame, start + Duration::from_millis(frame)))
            .collect();
        points[70].0 += 1;
        assert!(intervals(&points, POLL).is_err());
        points[70].0 -= 1;
        points[70].1 = points[69].1;
        assert!(intervals(&points, POLL).is_err());
        assert!(intervals(&points[..119], POLL).is_err());
    }
}
