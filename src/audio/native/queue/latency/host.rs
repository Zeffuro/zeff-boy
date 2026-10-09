use super::*;
use std::time::Instant;

#[derive(Debug)]
struct OutputBuildFailure(cpal::Error);

impl std::fmt::Display for OutputBuildFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for OutputBuildFailure {}

#[test]
#[ignore = "opens a real silent host output stream for callback timing"]
fn native_audio_queue_host_timing_diagnostic() {
    run_diagnostic(&[
        (AudioBufferPolicy::Auto, false),
        (AudioBufferPolicy::LowLatency, false),
        (AudioBufferPolicy::Balanced, false),
        (AudioBufferPolicy::Stable, false),
        (AudioBufferPolicy::Stable, false),
        (AudioBufferPolicy::Balanced, false),
        (AudioBufferPolicy::LowLatency, false),
        (AudioBufferPolicy::Auto, false),
    ]);
}

#[test]
#[ignore = "opens a real silent host stream for netplay budget comparison"]
fn native_netplay_audio_budget_host_timing_diagnostic() {
    run_diagnostic(&[
        (AudioBufferPolicy::Auto, false),
        (AudioBufferPolicy::Auto, true),
        (AudioBufferPolicy::Auto, true),
        (AudioBufferPolicy::Auto, false),
    ]);
}

fn run_diagnostic(policies: &[(AudioBufferPolicy, bool)]) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no default host audio output");
    let mut cases = Vec::new();
    let mut rejected = Vec::new();
    let mut selected = None;
    for config in AudioOutput::select_output_configs(&device, Some(48_000)).unwrap() {
        if config.sample_format() != SampleFormat::F32 {
            continue;
        }
        match capture(&device, &config, policies[0].0, policies[0].1) {
            Ok(first) => {
                cases.push(first);
                selected = Some(config);
                break;
            }
            Err(error) if error.downcast_ref::<OutputBuildFailure>().is_some() => {
                rejected.push(json!({
                    "sample_rate": config.sample_rate(), "channels": config.channels(),
                    "error": error.to_string(),
                }));
            }
            Err(error) => panic!("host capture failed: {error}"),
        }
    }
    let config = selected.expect("diagnostic requires an available F32 output configuration");
    for &(policy, netplay) in &policies[1..] {
        cases.push(capture(&device, &config, policy, netplay).unwrap());
    }
    let report = json!({
        "host": format!("{:?}", host.id()),
        "device": output_device_info(&device).unwrap().name,
        "format": "F32", "channels": config.channels(), "sample_rate": config.sample_rate(),
        "rejected_output_configs": rejected,
        "test_artifact": true, "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "cases": cases,
        "limits": "silent synthetic markers through production queue/callback, host-predicted timing only, App prebuffer, DSP/resampling, gameplay, acoustic output and display excluded",
    });
    if let Some(path) = std::env::var_os("ZEFF_AUDIO_LATENCY_REPORT") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
            .unwrap();
    }
    println!("{report}");
}

fn capture(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    policy: AudioBufferPolicy,
    netplay: bool,
) -> anyhow::Result<Value> {
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    anyhow::ensure!(channels != 0, "host has no output channels");
    let capacity = ring_buffer_capacity_for_policy(rate, policy);
    let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
    let mut queue = NativeSampleQueue::new(producer);
    queue.netplay_budget.set_enabled(netplay);
    let underruns = Arc::new(AtomicU64::new(0));
    let mut state = AudioPlaybackState::new(
        Arc::new(AtomicUsize::new(playback_preroll_samples(
            rate,
            policy.normal_queue_ms(),
        ))),
        Arc::clone(&underruns),
        Arc::new(AtomicU64::new(0)),
    );
    let (mut observer, mut output, errors) = observer(rate, MARKERS + 1);
    let preroll = Arc::clone(&state.preroll_samples);
    let callback_samples = Arc::clone(&state.callback_samples);
    let stream_errors = Arc::new(AtomicU64::new(0));
    let failed = Arc::clone(&stream_errors);
    let minimum = Arc::new(AtomicUsize::new(usize::MAX));
    let maximum = Arc::new(AtomicUsize::new(0));
    let callback_min = Arc::clone(&minimum);
    let callback_max = Arc::clone(&maximum);
    let stream = device
        .build_output_stream(
            config.config(),
            move |pcm: &mut [f32], info| {
                callback_min.fetch_min(pcm.len() / channels, Ordering::Relaxed);
                callback_max.fetch_max(pcm.len() / channels, Ordering::Relaxed);
                fill_output_f32(pcm, channels as u16, &mut consumer, &mut state);
                observer.observe(pcm, channels, info.timestamp());
                pcm.fill(0.0);
            },
            move |_| {
                failed.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(OutputBuildFailure)?;
    stream.play()?;
    let hz = 60;
    let started = Instant::now();
    let mut queued = Vec::with_capacity(MARKERS);
    let mut records = Vec::with_capacity(MARKERS);
    let mut next = 0;
    let mut padded = false;
    let mut previous_underruns = 0;
    while records.len() < MARKERS {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(6),
            "host marker capture timed out"
        );
        anyhow::ensure!(
            errors.load(Ordering::Relaxed) == 0 && stream_errors.load(Ordering::Relaxed) == 0,
            "host capture lost records, PCM or clock validity"
        );
        while next < MARKERS
            && started.elapsed()
                >= Duration::from_nanos(next as u64 * 1_000_000_000 / u64::from(hz))
        {
            queued.push(stream.now());
            queue.staged_samples.extend(std::iter::repeat_n(
                (next + 1) as f32,
                marker_frames(next, rate, hz) * 2,
            ));
            next += 1;
        }
        if next == MARKERS && !padded {
            queue
                .staged_samples
                .extend(std::iter::repeat_n(0.0, capacity * 2));
            padded = true;
        }
        let total_underruns = underruns.load(Ordering::Relaxed);
        queue.configure_budget(
            QueueBudgetConfig {
                rate,
                policy,
                speed: 1,
                callback_samples: callback_samples.load(Ordering::Relaxed),
                underruns: total_underruns - previous_underruns,
                elapsed: started.elapsed(),
            },
            &preroll,
        );
        previous_underruns = total_underruns;
        queue.pump();
        while let Ok(span) = output.pop() {
            anyhow::ensure!(records.len() < MARKERS, "host emitted extra marker spans");
            records.push(span);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(stream);
    anyhow::ensure!(
        errors.load(Ordering::Relaxed) == 0 && stream_errors.load(Ordering::Relaxed) == 0,
        "host callback failed during final capture"
    );
    let mut report = summarize(&queued, &records, rate, hz, 30)?;
    report["policy"] = json!(format!("{policy:?}"));
    report["preroll_scalar_samples"] = json!(preroll.load(Ordering::Relaxed));
    report["netplay_budget_enabled"] = json!(netplay);
    report["logical_ring_limit_samples"] = json!(queue.limit);
    report["callback_frames_min"] = json!(minimum.load(Ordering::Relaxed));
    report["callback_frames_max"] = json!(maximum.load(Ordering::Relaxed));
    report["underrun_incidents"] = json!(underruns.load(Ordering::Relaxed));
    Ok(report)
}
