use super::harness::app_with_worker;
use crate::audio::AudioOutput;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{EmuThread, FrameResult};
use crate::settings::AudioBufferPolicy;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod timeline;

#[derive(Default)]
struct Arrivals {
    started: Option<Instant>,
    previous: Option<Instant>,
    gaps_ms: Vec<f64>,
    batches: Vec<(usize, usize)>,
    frames: usize,
    scalars: usize,
    nonzero: usize,
    faults: usize,
    first_ms: Option<f64>,
    last_ms: Option<f64>,
    epoch: Option<Instant>,
    timeline: Vec<[u64; 4]>,
}

thread_local! {
    static ARRIVALS: RefCell<Option<Arrivals>> = const { RefCell::new(None) };
}

pub(in crate::app) fn observe(result: &FrameResult) {
    ARRIVALS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(arrivals) = slot.as_mut() else {
            return;
        };
        let now = Instant::now();
        if let Some(previous) = arrivals.previous.replace(now) {
            arrivals
                .gaps_ms
                .push(now.duration_since(previous).as_secs_f64() * 1000.0);
        }
        let elapsed = now.duration_since(arrivals.started.unwrap()).as_secs_f64() * 1000.0;
        arrivals.first_ms.get_or_insert(elapsed);
        arrivals.last_ms = Some(elapsed);
        arrivals.frames += result.advanced_frames;
        arrivals.scalars += result.audio_samples.len();
        arrivals.nonzero += result.audio_samples.iter().filter(|s| **s != 0.0).count();
        arrivals.faults += usize::from(result.runtime_fault.is_some());
        arrivals
            .batches
            .push((result.advanced_frames, result.audio_samples.len()));
        if let Some(epoch) = arrivals.epoch {
            arrivals.timeline.push([
                now.duration_since(epoch).as_nanos() as u64,
                result.advanced_frames as u64,
                result.audio_samples.len() as u64,
                result.completed_step_requests as u64,
            ]);
        }
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    rom: PathBuf,
    state: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    cases: Vec<Case>,
    #[serde(default = "default_duration")]
    duration_seconds: u64,
    #[serde(default = "default_sample_rate")]
    sample_rate: u32,
}

fn default_duration() -> u64 {
    5
}

fn default_sample_rate() -> u32 {
    48_000
}

#[test]
#[ignore = "opt-in actual MD App/worker/DSP through a real silent CPAL stream"]
fn md_app_worker_silent_host_diagnostic() {
    let manifest_path =
        std::env::var_os("ZEFF_MD_AUDIO_HOST_CASES").expect("set ZEFF_MD_AUDIO_HOST_CASES");
    let report_path = PathBuf::from(
        std::env::var_os("ZEFF_MD_AUDIO_HOST_REPORT").expect("set ZEFF_MD_AUDIO_HOST_REPORT"),
    );
    assert_eq!(std::env::var("ZEFF_MUTE_AUDIO").as_deref(), Ok("1"));
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert!((1..=2).contains(&manifest.cases.len()));
    assert!((1..=60).contains(&manifest.duration_seconds));
    assert!([44_100, 48_000].contains(&manifest.sample_rate));
    let rate = manifest.sample_rate;
    let scratch = tempfile::tempdir().unwrap();
    let synthetic = scratch.path().join("host-control.md");
    std::fs::write(&synthetic, synthetic_rom()).unwrap();
    let control = Case {
        name: "synthetic_healthy".into(),
        rom: synthetic.clone(),
        state: None,
    };
    let mut reports = vec![run_case(&control, Duration::from_secs(3), rate, false)];
    save_progress(&report_path, &reports);
    let stalled = Case {
        name: "synthetic_stall_250ms".into(),
        rom: synthetic,
        state: None,
    };
    reports.push(run_case(&stalled, Duration::from_secs(3), rate, true));
    save_progress(&report_path, &reports);
    for case in &manifest.cases {
        assert!(
            case.state.is_some(),
            "actual passage requires an explicit state"
        );
        reports.push(run_case(
            case,
            Duration::from_secs(manifest.duration_seconds),
            rate,
            false,
        ));
        save_progress(&report_path, &reports);
    }
    let report = json!({
        "compiled_profile": env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "cases": reports,
        "limits": "actual MD/App tick/native worker/queue/resampler/callback; explicit 1ms CLI pump, no winit/GPU/render/device acoustic output; PCM zeroed only after callback inspection; output hash is observed DSP PCM, not an offline equivalence proof",
    });
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)
        .unwrap();
    output
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    println!("{report}");
    for case in report["cases"].as_array().unwrap() {
        assert_eq!(case["callback"]["stream_errors"], 0);
        assert_eq!(case["callback"]["lost_callback_records"], 0);
        assert_eq!(case["callback"]["invalid_stereo_scalars"], 0);
        assert!(case["callback"]["nonzero_stereo_scalars"].as_u64().unwrap() > 0);
        assert_eq!(case["worker"]["faults"], 0);
        assert!(case["worker"]["source_nonzero_scalars"].as_u64().unwrap() > 0);
    }
    let witness = &report["cases"][1];
    assert!(witness["callback"]["underrun_incidents"].as_u64().unwrap() > 0);
    assert!(
        witness["callback"]["rebuffer_silent_frames"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        witness["callback"]["consumed_frames_after_1500ms"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(witness["worker"]["max_batch_frames"].as_u64().unwrap() > 1);
}

fn run_case(case: &Case, duration: Duration, sample_rate: u32, stall: bool) -> Value {
    let bytes = std::fs::read(&case.rom).unwrap();
    let rom_hash = zeff_firmware::sha256_bytes(&bytes);
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &case.rom,
        &case.rom,
        Some(bytes),
        BackendLoadConfig {
            sample_rate: Some(sample_rate),
            md_controller: zeff_md_core::ControllerKind::ThreeButton,
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    let mut state_hash = None;
    if let Some(path) = &case.state {
        let bytes = std::fs::read(path).unwrap();
        state_hash = Some(zeff_firmware::sha256_bytes(&bytes));
        backend.load_state_from_bytes(bytes).unwrap();
    }
    backend.set_sample_rate(sample_rate);
    let initial_frame = backend.frame_count();
    let frame_ns = backend.nominal_frame_duration_ns();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 991, ActiveSystem::Md, case.rom.clone());
    app.settings.audio.output_sample_rate = sample_rate;
    app.last_audio_output_sample_rate = sample_rate;
    app.settings.audio.volume = 1.0;
    app.settings.audio.low_pass_enabled = false;
    app.settings.emulation.frame_skip = false;
    app.settings.rewind.enabled = false;
    let epoch = (std::env::var("ZEFF_MD_AUDIO_TIMELINE").as_deref() == Ok("1")).then(Instant::now);
    let (output, mut driver) =
        AudioOutput::silent_host_output(sample_rate, AudioBufferPolicy::Auto, epoch).unwrap();
    app.audio = Some(output);
    let start = Instant::now();
    app.timing.last_frame_time = crate::platform::Instant::now();
    ARRIVALS.with(|slot| {
        *slot.borrow_mut() = Some(Arrivals {
            started: Some(start),
            epoch,
            timeline: if epoch.is_some() {
                Vec::with_capacity(4096)
            } else {
                Vec::new()
            },
            ..Default::default()
        })
    });
    let mut previous = start;
    let mut pumps = Vec::new();
    let mut maximum_ring = 0;
    let mut maximum_staging = 0;
    let mut stall_at = None;
    let mut timeline = epoch.map(|epoch| timeline::Timeline::new(epoch, duration));
    while start.elapsed() < duration {
        let now = Instant::now();
        pumps.push(now.duration_since(previous).as_secs_f64() * 1000.0);
        previous = now;
        let record = timeline
            .as_ref()
            .map(|timeline| timeline.begin(app.audio.as_ref().unwrap()));
        app.tick();
        let record = record.map(|record| record.after_tick(app.audio.as_ref().unwrap()));
        app.pump_emulator_audio();
        if let (Some(timeline), Some(record)) = (&mut timeline, record) {
            timeline.finish(record, app.audio.as_ref().unwrap());
        }
        driver.poll();
        let (ring, staging) = app.audio.as_ref().unwrap().test_queue_levels();
        maximum_ring = maximum_ring.max(ring);
        maximum_staging = maximum_staging.max(staging);
        if stall && stall_at.is_none() && start.elapsed() >= Duration::from_secs(1) {
            stall_at = Some(start.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(Duration::from_millis(250));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let queue = app.audio.as_ref().unwrap().test_queue_snapshot();
    app.audio = None;
    let mut callback = driver.report();
    app.emu_thread.as_mut().unwrap().shutdown();
    let arrivals = ARRIVALS.with(|slot| slot.borrow_mut().take().unwrap());
    let timeline =
        timeline.map(|timeline| timeline.report(start, &arrivals.timeline, &mut callback));
    let samples_per_frame: Vec<_> = arrivals
        .batches
        .iter()
        .filter(|(n, _)| *n != 0)
        .map(|(frames, samples)| *samples as f64 / *frames as f64)
        .collect();
    json!({
        "name": case.name, "rom": case.rom, "state": case.state,
        "rom_sha256": hex(&rom_hash), "state_sha256": state_hash.as_ref().map(|h| hex(h)),
        "requested_duration_ms": duration.as_millis(), "actual_duration_ms": start.elapsed().as_secs_f64() * 1000.0,
        "source_rate": sample_rate, "initial_frame": initial_frame, "nominal_frame_duration_ns": frame_ns,
        "controller": "ThreeButton",
        "driver_pump_ms": distribution(pumps), "intentional_stall_at_ms": stall_at,
        "intentional_stall_duration_ms": if stall { 250 } else { 0 },
        "maximum_ring_scalars": maximum_ring, "maximum_staged_scalars": maximum_staging, "final_queue": queue,
        "worker": {
            "arrivals": arrivals.batches.len(), "advanced_frames": arrivals.frames,
            "source_scalars": arrivals.scalars, "source_nonzero_scalars": arrivals.nonzero,
            "faults": arrivals.faults, "first_arrival_ms": arrivals.first_ms, "last_arrival_ms": arrivals.last_ms,
            "max_batch_frames": arrivals.batches.iter().map(|(n, _)| n).max(),
            "arrival_gap_ms": distribution(arrivals.gaps_ms), "source_scalars_per_field": distribution(samples_per_frame),
        },
        "callback": callback,
        "timeline": timeline,
    })
}

fn save_progress(report_path: &std::path::Path, cases: &[Value]) {
    let progress = json!({
        "complete": false,
        "compiled_profile": env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "cases": cases,
    });
    std::fs::write(
        report_path.with_extension("progress.json"),
        serde_json::to_vec_pretty(&progress).unwrap(),
    )
    .unwrap();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn distribution(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    if samples.is_empty() {
        return json!({"samples": 0});
    }
    let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
    json!({"samples": samples.len(), "min": samples[0], "p50": at(50), "p95": at(95), "max": samples.last()})
}

fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0; 32 * 1024];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    let mut words = Vec::new();
    for value in [0x80_u16, 0x04, 0x90] {
        words.extend([0x13fc, value, 0x00c0, 0x0011]);
    }
    words.extend([0x60fe, 0x4e71]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}
