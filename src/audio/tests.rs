use super::*;
use crate::settings::AudioBufferPolicy;
use cpal::SampleFormat;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[test]
#[ignore = "requires native audio endpoints; enumerates capabilities without creating a stream"]
fn native_host_output_enumeration_diagnostic() {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let devices = crate::audio::output_devices().expect("output-device enumeration failed");
    let default_name = host
        .default_output_device()
        .and_then(|device| device.description().ok())
        .map_or_else(
            || "unavailable".to_owned(),
            |description| description.name().to_owned(),
        );
    let mut default_configs = 0;
    let mut supported_ranges = 0;
    let mut config_errors = 0;
    for device in &devices {
        let id: cpal::DeviceId = device.id.parse().expect("enumerated device ID must parse");
        assert_eq!(id.to_string(), device.id);
        let resolved = host
            .device_by_id(&id)
            .expect("enumerated device ID must resolve");
        assert_eq!(
            resolved.id().expect("resolved device must expose its ID"),
            id
        );
        match resolved.default_output_config() {
            Ok(_) => default_configs += 1,
            Err(error) => {
                config_errors += 1;
                println!(
                    "audio endpoint {:?}: default configuration unavailable ({error})",
                    device.name
                );
            }
        }
        match resolved.supported_output_configs() {
            Ok(configs) => {
                let count = configs.count();
                supported_ranges += count;
                if count == 0 {
                    println!(
                        "audio endpoint {:?}: no supported configuration ranges reported",
                        device.name
                    );
                }
            }
            Err(error) => {
                config_errors += 1;
                println!(
                    "audio endpoint {:?}: configuration enumeration unavailable ({error})",
                    device.name
                );
            }
        }
    }
    println!(
        "audio host: backend={} endpoints={} verified_ids={} default_configs={} supported_ranges={} config_errors={} current_default={default_name:?}; no streams created",
        host.id().name(),
        devices.len(),
        devices.len(),
        default_configs,
        supported_ranges,
        config_errors,
    );
}

#[test]
fn playback_speed_selects_complete_stereo_frames() {
    let mut output = vec![99.0];
    copy_stereo_at_speed(&[0.0, 0.1, 1.0, 1.1, 2.0, 2.1, 3.0, 3.1], 2, &mut output);
    assert_eq!(output, vec![0.0, 0.1, 2.0, 2.1]);

    copy_stereo_at_speed(&[0.0, 0.1, 1.0, 1.1, 2.0], 1, &mut output);
    assert_eq!(output, vec![0.0, 0.1, 1.0, 1.1]);
}

#[test]
fn ring_buffer_capacity_44100() {
    assert_eq!(ring_buffer_capacity(44100), 17640);
}

#[test]
fn ring_buffer_capacity_48000() {
    assert_eq!(ring_buffer_capacity(48000), 19200);
}

#[test]
fn queue_presets_have_bounded_distinct_capacities() {
    assert_eq!(
        ring_buffer_capacity_for_policy(48_000, AudioBufferPolicy::LowLatency),
        7_200
    );
    assert_eq!(
        ring_buffer_capacity_for_policy(48_000, AudioBufferPolicy::Balanced),
        14_400
    );
    assert_eq!(
        ring_buffer_capacity_for_policy(48_000, AudioBufferPolicy::Stable),
        28_800
    );
    assert_eq!(ring_buffer_capacity(48_000), 19_200);
}

#[test]
fn repeated_low_latency_underruns_request_auto_queue_recovery() {
    assert!(buffer_fallback_for_underruns(AudioBufferPolicy::LowLatency, 2).is_none());
    let fallback = buffer_fallback_for_underruns(AudioBufferPolicy::LowLatency, 3).unwrap();
    assert_eq!(fallback.requested, AudioBufferPolicy::LowLatency);
    assert_eq!(fallback.active, AudioBufferPolicy::Auto);
    assert_eq!(fallback.underrun_reports, 3);
    assert!(buffer_fallback_for_underruns(AudioBufferPolicy::Stable, 10).is_none());
}

#[test]
fn underrun_recovery_counts_incidents_across_healthy_producer_frames() {
    let mut recovery = UnderrunRecovery::default();
    for (index, underruns) in [1, 0, 0, 1, 0, 0, 1].into_iter().enumerate() {
        let fallback = recovery.observe(
            AudioBufferPolicy::LowLatency,
            underruns,
            std::time::Duration::from_millis(index as u64 * 16),
        );
        if index == 6 {
            assert_eq!(fallback.unwrap().underrun_reports, 3);
        } else {
            assert!(fallback.is_none());
        }
    }
}

#[test]
fn underrun_recovery_expires_old_noise_after_five_seconds() {
    use std::time::Duration;
    let mut recovery = UnderrunRecovery::default();
    assert!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 2, Duration::ZERO)
            .is_none()
    );
    assert!(
        recovery
            .observe(
                AudioBufferPolicy::LowLatency,
                0,
                Duration::from_millis(5_001)
            )
            .is_none()
    );
    assert!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(6))
            .is_none()
    );
    assert!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(10))
            .is_none()
    );
    assert_eq!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(11))
            .unwrap()
            .underrun_reports,
        3
    );
    assert!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(12))
            .is_none()
    );
}

#[test]
fn underrun_recovery_keeps_recent_incidents_when_the_oldest_expires() {
    use std::time::Duration;
    let mut recovery = UnderrunRecovery::default();
    for second in [0, 4, 6] {
        assert!(
            recovery
                .observe(
                    AudioBufferPolicy::LowLatency,
                    1,
                    Duration::from_secs(second)
                )
                .is_none()
        );
    }
    assert_eq!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(8))
            .unwrap()
            .underrun_reports,
        3
    );
}

#[test]
fn underrun_recovery_counts_coalesced_callbacks_and_ignores_other_policies() {
    use std::time::Duration;
    let mut recovery = UnderrunRecovery::default();
    for policy in [
        AudioBufferPolicy::Auto,
        AudioBufferPolicy::Balanced,
        AudioBufferPolicy::Stable,
    ] {
        assert!(
            recovery
                .observe(AudioBufferPolicy::LowLatency, 2, Duration::ZERO)
                .is_none()
        );
        assert!(
            recovery
                .observe(policy, u64::MAX, Duration::from_secs(1))
                .is_none()
        );
        assert!(
            recovery
                .observe(AudioBufferPolicy::LowLatency, 1, Duration::from_secs(2))
                .is_none()
        );
        recovery = UnderrunRecovery::default();
    }
    assert_eq!(
        recovery
            .observe(AudioBufferPolicy::LowLatency, u64::MAX, Duration::ZERO)
            .unwrap()
            .underrun_reports,
        u8::MAX
    );
}

#[test]
fn stream_errors_are_reported_for_host_recovery() {
    assert!(stream_error_message(0).is_none());
    assert_eq!(
        stream_error_message(2).as_deref(),
        Some("Audio output device reported 2 stream error(s); retrying System default.")
    );
}

#[test]
fn sample_format_rank_prefers_float_then_signed_then_unsigned() {
    assert!(sample_format_rank(SampleFormat::F32) < sample_format_rank(SampleFormat::I16));
    assert!(sample_format_rank(SampleFormat::I16) < sample_format_rank(SampleFormat::U16));
    assert!(sample_format_rank(SampleFormat::U16) < sample_format_rank(SampleFormat::U8));
}

#[test]
fn preferred_emulator_rate_is_independent_of_device_rate() {
    assert_eq!(emulator_source_sample_rate(Some(48_000), 96_000), 48_000);
    assert_eq!(emulator_source_sample_rate(None, 96_000), 96_000);
}

fn push_samples(producer: &mut rtrb::Producer<QueuedAudioSample>, samples: &[f32]) {
    push_samples_for_generation(producer, samples, 0);
}

fn push_samples_for_generation(
    producer: &mut rtrb::Producer<QueuedAudioSample>,
    samples: &[f32],
    generation: u64,
) {
    if let Ok(mut chunk) = producer.write_chunk_uninit(samples.len()) {
        let (first, second) = chunk.as_mut_slices();
        for (dst, &src) in first.iter_mut().zip(samples.iter()) {
            dst.write(QueuedAudioSample {
                generation,
                value: src,
            });
        }
        for (dst, &src) in second.iter_mut().zip(samples[first.len()..].iter()) {
            dst.write(QueuedAudioSample {
                generation,
                value: src,
            });
        }
        unsafe {
            chunk.commit_all();
        }
    }
}

fn pop_samples(consumer: &mut rtrb::Consumer<QueuedAudioSample>, count: usize) -> Vec<f32> {
    let chunk = consumer.read_chunk(count).unwrap();
    let (first, second) = chunk.as_slices();
    let samples = first
        .iter()
        .chain(second)
        .map(|sample| sample.value)
        .collect();
    chunk.commit_all();
    samples
}

#[test]
fn staged_audio_preserves_order_when_a_catch_up_batch_exceeds_ring_space() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
    let mut staged = (4..14).map(|value| value as f32).collect();
    push_samples(&mut producer, &[0.0, 1.0, 2.0, 3.0]);

    flush_staged_samples(&mut producer, &mut staged, 0);
    assert_eq!(
        pop_samples(&mut consumer, 8),
        (0..8).map(|v| v as f32).collect::<Vec<_>>()
    );
    assert_eq!(
        staged.iter().copied().collect::<Vec<_>>(),
        vec![8.0, 9.0, 10.0, 11.0, 12.0, 13.0]
    );

    flush_staged_samples(&mut producer, &mut staged, 0);
    assert_eq!(
        pop_samples(&mut consumer, 6),
        vec![8.0, 9.0, 10.0, 11.0, 12.0, 13.0]
    );
    assert!(staged.is_empty());
}

#[test]
fn long_stall_recovery_keeps_only_the_latest_complete_stereo_window() {
    assert_eq!(long_stall_recovery_range(12, 8, 20, 8), None);
    assert_eq!(long_stall_recovery_range(13, 8, 20, 8), Some(0..8));
    assert_eq!(long_stall_recovery_range(0, 31, 20, 8), Some(22..30));
    assert_eq!(long_stall_recovery_range(usize::MAX, 7, 20, 8), Some(0..6));
}

fn playback_state(preroll_samples: usize) -> (AudioPlaybackState, Arc<AtomicU64>) {
    let underruns = Arc::new(AtomicU64::new(0));
    (
        AudioPlaybackState::new(
            Arc::new(AtomicUsize::new(preroll_samples)),
            Arc::clone(&underruns),
            Arc::new(AtomicU64::new(0)),
        ),
        underruns,
    )
}

#[test]
fn fill_mono_exact() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.5, -0.5, 0.25, 0.75, -0.5, -1.0]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 3];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0, 0.5, -0.75]);
    assert_eq!(consumer.slots(), 0);
}

#[test]
fn fill_mono_initial_shortage_preserves_audio_until_complete() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.0, 0.5]);
    let (mut playback, underruns) = playback_state(0);

    let mut data = vec![9.9f32; 3];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 3]);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(underruns.load(Ordering::Relaxed), 0);

    push_samples(&mut producer, &[0.25, 0.75, -1.0, 0.0]);
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.25, 0.5, -0.5]);
    assert_eq!(consumer.slots(), 0);
}

#[test]
fn fill_mono_empty_buffer_is_silence() {
    let (_producer, mut consumer) = rtrb::RingBuffer::<QueuedAudioSample>::new(64);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![1.0f32; 4];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 4]);
}

#[test]
fn oversized_low_latency_callback_plays_available_audio_and_requests_recovery() {
    use std::time::Duration;
    let capacity = ring_buffer_capacity_for_policy(48_000, AudioBufferPolicy::LowLatency);
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(capacity);
    let (mut playback, underruns) = playback_state(capacity / 2);
    let mut recovery = UnderrunRecovery::default();
    let mut data = vec![9.0; 8_192];
    for callback in 0..3 {
        push_samples(&mut producer, &vec![0.25; capacity]);
        fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
        assert!(data[..capacity].iter().all(|sample| *sample == 0.25));
        assert!(data[capacity..].iter().all(|sample| *sample == 0.0));
        assert_eq!(consumer.slots(), 0);
        let incidents = underruns.swap(0, Ordering::Relaxed);
        assert_eq!(incidents, 1);
        let fallback = recovery.observe(
            AudioBufferPolicy::LowLatency,
            incidents,
            Duration::from_millis(callback * 100),
        );
        assert_eq!(fallback.is_some(), callback == 2);
    }
}

#[test]
fn oversized_callback_still_makes_progress_with_auto_buffering() {
    let capacity = ring_buffer_capacity(48_000);
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(capacity);
    let (mut playback, underruns) = playback_state(capacity / 2);
    push_samples(&mut producer, &[0.25, -0.5]);
    let mut data = vec![9.0; capacity + 2];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(&data[..2], &[0.25, -0.5]);
    assert!(data[2..].iter().all(|sample| *sample == 0.0));
    assert_eq!(consumer.slots(), 0);
    assert_eq!(underruns.load(Ordering::Relaxed), 1);

    push_samples(&mut producer, &[0.5, 0.5]);
    let mut normal = [9.0; 2];
    fill_output_f32(&mut normal, 2, &mut consumer, &mut playback);
    assert_eq!(normal, [0.0; 2]);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(underruns.load(Ordering::Relaxed), 1);
}

#[test]
fn oversized_mono_callback_consumes_only_complete_stereo_pairs() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
    let (mut playback, underruns) = playback_state(4);
    push_samples(&mut producer, &[0.0, 0.5, 0.25, 0.75, -1.0]);
    let mut data = [9.0; 5];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, [0.25, 0.5, 0.0, 0.0, 0.0]);
    assert_eq!(consumer.slots(), 1);
    push_samples(&mut producer, &[0.0]);
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, [-0.5, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(consumer.slots(), 0);
    assert_eq!(underruns.load(Ordering::Relaxed), 2);
}

#[test]
fn fill_stereo_maps_lr_pairs() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2, 0.3, 0.4]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 4];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn fill_stereo_underrun_rebuffers_without_consuming_partial_audio() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.5, 0.6]);
    let (mut playback, underruns) = playback_state(0);

    let mut first = vec![0.0; 2];
    fill_output_f32(&mut first, 2, &mut consumer, &mut playback);
    assert_eq!(first, vec![0.5, 0.6]);

    push_samples(&mut producer, &[0.7, 0.8]);

    let mut data = vec![9.0f32; 6];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 6]);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(underruns.load(Ordering::Relaxed), 1);

    push_samples(&mut producer, &[0.9, 1.0, 1.1, 1.2]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.7, 0.8, 0.9, 1.0, 1.1, 1.2]);
}

#[test]
fn fill_multichannel_mixes_to_surround() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.4, 0.6]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 4];
    fill_output_f32(&mut data, 4, &mut consumer, &mut playback);
    assert_eq!(data[0], 0.4);
    assert_eq!(data[1], 0.6);
    assert_eq!(data[2], 0.5);
    assert_eq!(data[3], 0.5);
}

#[test]
fn fill_stereo_empty_is_silence() {
    let (_producer, mut consumer) = rtrb::RingBuffer::<QueuedAudioSample>::new(64);
    let (mut playback, _) = playback_state(0);
    let mut data = vec![1.0f32; 4];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 4]);
}

#[test]
fn new_session_discards_queued_audio_from_the_previous_game() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    let generation = Arc::new(AtomicU64::new(0));
    let underruns = Arc::new(AtomicU64::new(0));
    let mut playback = AudioPlaybackState::new(
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&underruns),
        Arc::clone(&generation),
    );
    push_samples_for_generation(&mut producer, &[0.8, -0.8, 0.7, -0.7], 0);

    generation.store(1, Ordering::Release);
    push_samples_for_generation(&mut producer, &[0.1, 0.2, 0.3, 0.4], 1);

    let mut data = vec![9.0; 4];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);

    assert_eq!(data, vec![0.1, 0.2, 0.3, 0.4]);
    assert_eq!(consumer.slots(), 0);
    assert_eq!(underruns.load(Ordering::Relaxed), 0);
}

#[test]
fn fill_stereo_odd_samples_waits_for_a_complete_callback() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2, 0.3]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 4];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);

    assert_eq!(data, vec![0.0; 4]);
    assert_eq!(consumer.slots(), 3);
}

#[test]
fn playback_waits_for_preroll_without_consuming_or_signaling_underrun() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    let (mut playback, underruns) = playback_state(8);
    push_samples(&mut producer, &[0.1, 0.2, 0.3, 0.4]);
    let mut data = vec![1.0; 4];

    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 4]);
    assert_eq!(consumer.slots(), 4);
    assert_eq!(underruns.load(Ordering::Relaxed), 0);

    push_samples(&mut producer, &[0.5, 0.6, 0.7, 0.8]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.1, 0.2, 0.3, 0.4]);
    assert_eq!(consumer.slots(), 4);
}

#[test]
fn preroll_target_is_half_of_each_queue_policy() {
    assert_eq!(playback_preroll_samples(48_000, 200), 9_600);
    assert_eq!(playback_preroll_samples(48_000, 40), 1_920);
}

#[test]
fn returning_from_fast_forward_rebuffers_to_the_normal_preroll_target() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(32);
    let preroll = Arc::new(AtomicUsize::new(8));
    let underruns = Arc::new(AtomicU64::new(0));
    let mut playback = AudioPlaybackState::new(
        Arc::clone(&preroll),
        Arc::clone(&underruns),
        Arc::new(AtomicU64::new(0)),
    );
    let mut data = vec![-1.0; 4];

    push_samples(&mut producer, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0, 1.0, 2.0, 3.0]);

    preroll.store(2, Ordering::Relaxed);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![4.0, 5.0, 6.0, 7.0]);

    push_samples(&mut producer, &[8.0, 9.0, 10.0, 11.0]);
    preroll.store(8, Ordering::Relaxed);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 4]);
    assert_eq!(consumer.slots(), 4);
    assert_eq!(underruns.load(Ordering::Relaxed), 0);

    push_samples(&mut producer, &[12.0, 13.0, 14.0, 15.0]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![8.0, 9.0, 10.0, 11.0]);
}

#[test]
fn sustained_stall_recovery_preserves_the_complete_audio_sequence() {
    const PREROLL: usize = 9_600;
    const CALLBACK: usize = 960;
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(PREROLL * 2);
    let (mut playback, underruns) = playback_state(PREROLL);
    let first = (0..PREROLL).map(|value| value as f32).collect::<Vec<_>>();
    push_samples(&mut producer, &first[..PREROLL / 2]);

    let mut data = vec![-1.0; CALLBACK];
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; CALLBACK]);
    assert_eq!(consumer.slots(), PREROLL / 2);

    push_samples(&mut producer, &first[PREROLL / 2..]);
    let mut played = Vec::new();
    for _ in 0..PREROLL / CALLBACK {
        fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
        played.extend_from_slice(&data);
    }
    assert_eq!(played, first);

    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; CALLBACK]);
    assert_eq!(underruns.load(Ordering::Relaxed), 1);

    let second = (PREROLL..PREROLL * 2)
        .map(|value| value as f32)
        .collect::<Vec<_>>();
    push_samples(&mut producer, &second[..PREROLL / 2]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; CALLBACK]);
    assert_eq!(consumer.slots(), PREROLL / 2);
    push_samples(&mut producer, &second[PREROLL / 2..]);
    fill_output_f32(&mut data, 2, &mut consumer, &mut playback);
    assert_eq!(data, second[..CALLBACK]);
    assert_eq!(consumer.slots(), PREROLL - CALLBACK);
}

#[test]
fn low_pass_alpha_is_bounded() {
    let alpha = low_pass_alpha(48_000, 4_800);
    assert!(alpha > 0.0);
    assert!(alpha < 1.0);
}

#[test]
fn low_pass_filter_smooths_step_change() {
    let mut filter = OnePoleLowPass::default();
    let alpha = low_pass_alpha(48_000, 2_000);

    let first = filter.apply_sample(0.0, 0, alpha);
    let second = filter.apply_sample(1.0, 0, alpha);

    assert_eq!(first, 0.0);
    assert!(second > 0.0);
    assert!(second < 1.0);
}
