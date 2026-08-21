use super::*;
use cpal::SampleFormat;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[test]
fn ring_buffer_capacity_44100() {
    assert_eq!(ring_buffer_capacity(44100), 17640);
}

#[test]
fn ring_buffer_capacity_48000() {
    assert_eq!(ring_buffer_capacity(48000), 19200);
}

#[test]
fn sample_format_rank_prefers_float_then_signed_then_unsigned() {
    assert!(sample_format_rank(SampleFormat::F32) < sample_format_rank(SampleFormat::I16));
    assert!(sample_format_rank(SampleFormat::I16) < sample_format_rank(SampleFormat::U16));
    assert!(sample_format_rank(SampleFormat::U16) < sample_format_rank(SampleFormat::U8));
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
    push_samples(&mut producer, &[0.5, -0.5, 0.25]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 3];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.5, -0.5, 0.25]);
}

#[test]
fn fill_mono_initial_shortage_preserves_audio_until_complete() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2]);
    let (mut playback, underruns) = playback_state(0);

    let mut data = vec![9.9f32; 5];
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.0; 5]);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(underruns.load(Ordering::Relaxed), 0);

    push_samples(&mut producer, &[0.3, 0.4, 0.5]);
    fill_output_f32(&mut data, 1, &mut consumer, &mut playback);
    assert_eq!(data, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
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
fn fill_stereo_maps_lr_pairs() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2, 0.3, 0.4]);
    let (mut playback, _) = playback_state(0);

    let mut data = vec![0.0f32; 4]; // 2 frames * 2 channels
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
