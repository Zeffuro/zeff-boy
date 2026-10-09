use super::*;
use crate::audio::AudioQueueConfig;

fn admission() -> AudioQueueConfig {
    AudioQueueConfig {
        master_volume: 1.0,
        playback_speed: 1,
        mute_during_fast_forward: false,
        low_pass_enabled: false,
        low_pass_cutoff_hz: 20_000,
    }
}

#[test]
fn netplay_audio_output_recovers_from_callbacks_without_another_admission() {
    let (mut output, mut driver) = AudioOutput::test_output(AudioBufferPolicy::Auto);
    output.set_netplay_active(true);
    assert_eq!(driver.preroll(), 3_552);
    let expected: Vec<_> = (0..30_000)
        .map(|index| (index + 1) as f32 / 65_536.0)
        .collect();
    let mut admission_pcm = expected.clone();
    admission_pcm.extend([0.0; 19_200]);
    output.queue_samples(&admission_pcm, &admission());
    assert_eq!(driver.buffered(), 7_200);
    let mut actual = Vec::new();
    for index in 0..3 {
        actual.extend(
            driver
                .render(4_800)
                .into_iter()
                .filter(|sample| *sample != 0.0),
        );
        assert_eq!(driver.render(4_800), vec![0.0; 4_800]);
        assert_eq!(driver.underruns(), 1);
        output.pump_queued_samples();
        assert_eq!(driver.underruns(), 0);
        assert_eq!(driver.preroll(), if index == 2 { 9_600 } else { 3_552 });
    }
    for _ in 0..100 {
        output.pump_queued_samples();
        actual.extend(
            driver
                .render(512)
                .into_iter()
                .filter(|sample| *sample != 0.0),
        );
        if actual.len() == expected.len() {
            break;
        }
    }
    assert_eq!(actual, expected);
    assert!(output.take_buffer_fallback_request().is_none());
    output.discard_queued_samples();
    assert_eq!(driver.preroll(), 9_600);
    output.set_netplay_active(true);
    assert_eq!(driver.preroll(), 9_600);
    let (mut replacement, replacement_driver) = AudioOutput::test_output(AudioBufferPolicy::Auto);
    replacement.inherit_netplay_buffer(&output);
    assert_eq!(replacement_driver.preroll(), 9_600);
    replacement.set_netplay_active(false);
    replacement.set_netplay_active(true);
    assert_eq!(replacement_driver.preroll(), 3_552);
    output.set_netplay_active(false);
    output.set_netplay_active(true);
    assert_eq!(driver.preroll(), 3_552);
}

#[test]
fn netplay_audio_output_recovers_from_unprimed_callback_demand_and_preserves_explicit_presets() {
    let (mut output, mut driver) = AudioOutput::test_output(AudioBufferPolicy::Auto);
    output.set_netplay_active(true);
    let expected = vec![0.25; 9_600];
    output.queue_samples(&expected, &admission());
    assert_eq!(driver.render(9_600), vec![0.0; 9_600]);
    assert_eq!(driver.underruns(), 0);
    output.pump_queued_samples();
    assert_eq!(driver.preroll(), 9_600);
    assert_eq!(driver.render(9_600), expected);
    for policy in [
        AudioBufferPolicy::LowLatency,
        AudioBufferPolicy::Balanced,
        AudioBufferPolicy::Stable,
    ] {
        let (mut output, driver) = AudioOutput::test_output(policy);
        output.set_netplay_active(true);
        output.queue_samples(&[0.25; 20_000], &admission());
        assert_eq!(
            driver.preroll(),
            playback_preroll_samples(48_000, policy.normal_queue_ms())
        );
        assert!(!output.queue.netplay_budget.enabled());
    }
}
