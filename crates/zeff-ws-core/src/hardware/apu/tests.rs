use super::*;

fn alternating_wave_ram() -> Vec<u8> {
    let mut ram = vec![0; 0x10000];
    for byte in ram.iter_mut().take(16) {
        *byte = 0xF0;
    }
    ram
}

#[test]
fn wavetable_channel_generates_stereo_samples() {
    let mut apu = Apu::new(48_000);
    let ram = alternating_wave_ram();
    apu.write8(0x80, 0x00);
    apu.write8(0x81, 0x07);
    apu.write8(0x88, 0xFF);
    apu.write8(0x90, 0x01);

    apu.step_cycles(4096, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
    assert_eq!(samples.len() % 2, 0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.001));
}

#[test]
fn disabled_sample_generation_keeps_buffer_empty() {
    let mut apu = Apu::new(48_000);
    let ram = alternating_wave_ram();
    apu.write8(0x80, 0x00);
    apu.write8(0x81, 0x07);
    apu.write8(0x88, 0xFF);
    apu.write8(0x90, 0x01);
    apu.set_sample_generation_enabled(false);

    apu.step_cycles(4096, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(samples.is_empty());
    assert_ne!(apu.debug_snapshot().sample_pos[0], 0);
}

#[test]
fn disabled_sample_fast_path_matches_sampled_generator_state() {
    let ram = alternating_wave_ram();
    let mut sampled = Apu::new(48_000);
    sampled.write8(0x80, 0x00);
    sampled.write8(0x81, 0x07);
    sampled.write8(0x88, 0xFF);
    sampled.write8(0x90, 0x01);
    let mut disabled = sampled.clone();
    disabled.set_sample_generation_enabled(false);

    sampled.step_cycles(4096, &ram);
    disabled.step_cycles(4096, &ram);

    assert_eq!(disabled.save_state(), sampled.save_state());
}

#[test]
fn disabled_channels_leave_generator_state_unchanged() {
    let ram = alternating_wave_ram();
    for sample_generation_enabled in [false, true] {
        let mut apu = Apu::new(48_000);
        apu.write8(CONTROL_PORT, CHANNEL_3_SWEEP | CHANNEL_4_NOISE);
        apu.set_sample_generation_enabled(sample_generation_enabled);
        let mut expected = apu.save_state();

        apu.step_cycles(4096, &ram);

        expected.sample_cycle_accumulator = apu.sample_cycle_accumulator;
        assert_eq!(apu.save_state(), expected);
        if sample_generation_enabled {
            let mut samples = Vec::new();
            apu.drain_audio_samples_into(&mut samples);
            assert!(!samples.is_empty());
            assert!(samples.iter().all(|sample| sample.abs() <= f32::EPSILON));
        }
    }
}

#[test]
fn channel_mute_suppresses_output() {
    let mut apu = Apu::new(48_000);
    let ram = alternating_wave_ram();
    apu.write8(0x80, 0x00);
    apu.write8(0x81, 0x07);
    apu.write8(0x88, 0xFF);
    apu.write8(0x90, 0x01);
    apu.set_channel_mutes([true, false, false, false]);

    apu.step_cycles(4096, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|sample| sample.abs() <= f32::EPSILON));
}

#[test]
fn debug_samples_survive_audio_drain() {
    let mut apu = Apu::new(48_000);
    let ram = alternating_wave_ram();
    apu.write8(0x80, 0x00);
    apu.write8(0x81, 0x07);
    apu.write8(0x88, 0xFF);
    apu.write8(0x90, 0x01);

    apu.step_cycles(4096, &ram);
    let before_drain = apu.master_debug_samples_ordered();
    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);

    assert!(!samples.is_empty());
    assert!(!before_drain.is_empty());
    assert_eq!(apu.debug_snapshot().buffered_samples, 0);
    assert_eq!(apu.master_debug_samples_ordered(), before_drain);
    assert!(
        apu.channel_debug_samples_ordered(0)
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
}

#[test]
fn debug_samples_follow_channel_mutes() {
    let mut apu = Apu::new(48_000);
    let ram = alternating_wave_ram();
    apu.write8(0x80, 0x00);
    apu.write8(0x81, 0x07);
    apu.write8(0x88, 0xFF);
    apu.write8(0x90, 0x01);
    apu.set_channel_mutes([true, false, false, false]);

    apu.step_cycles(4096, &ram);

    assert!(
        apu.channel_debug_samples_ordered(0)
            .iter()
            .all(|sample| sample.abs() <= f32::EPSILON)
    );
    assert!(
        apu.master_debug_samples_ordered()
            .iter()
            .all(|sample| sample.abs() <= f32::EPSILON)
    );
}

#[test]
fn hyper_voice_sample_generates_selected_stereo_output() {
    let mut apu = Apu::new(48_000);
    let ram = vec![0; 0x10000];
    apu.write8(0x6A, 0x80);
    apu.write8(0x6B, 0x60);
    apu.write_hyper_voice_dma_sample(0x7F);

    apu.step_cycles(64, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(samples.len() >= 2);
    assert!(samples[0] > 0.1);
    assert!(samples[1] > 0.1);
}

#[test]
fn hyper_voice_signed_mode_can_generate_negative_output() {
    let mut apu = Apu::new(48_000);
    let ram = vec![0; 0x10000];
    apu.write8(0x6A, 0x88);
    apu.write8(0x6B, 0x40);
    apu.write_hyper_voice_dma_sample(0x80);

    apu.step_cycles(64, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(samples.len() >= 2);
    assert!(samples[0].abs() <= f32::EPSILON);
    assert!(samples[1] < -0.1);
}

#[test]
fn hyper_voice_direct_output_ports_feed_mixer() {
    let mut apu = Apu::new(48_000);
    let ram = vec![0; 0x10000];
    apu.write8(0x64, 0x00);
    apu.write8(0x65, 0x40);
    apu.write8(0x66, 0x00);
    apu.write8(0x67, 0xC0);
    apu.write8(0x6A, 0x80);

    apu.step_cycles(64, &ram);

    let mut samples = Vec::new();
    apu.drain_audio_samples_into(&mut samples);
    assert!(samples.len() >= 2);
    assert!(samples[0] > 0.1);
    assert!(samples[1] < -0.1);
}

#[test]
fn hyper_voice_manual_input_alternates_stereo_channels() {
    let mut apu = Apu::new(48_000);
    apu.write8(0x6A, 0x88);
    apu.write8(0x6B, 0x10);

    apu.write8(0x69, 0x80);
    apu.write8(0x69, 0x7F);

    let debug = apu.debug_snapshot();
    assert!(debug.hyper_voice_left_output < 0);
    assert!(debug.hyper_voice_right_output > 0);
    assert!(debug.hyper_voice_next_left);
}

#[test]
fn sound_port_masks_match_hardware_behavior() {
    let mut apu = Apu::new(48_000);

    apu.write8(0x81, 0xFF);
    apu.write8(0x8E, 0xFF);
    apu.write8(0x91, 0xFF);
    apu.write8(0x93, 0xFF);
    apu.write8(0x94, 0xFF);
    apu.write8(0x6B, 0xFF);

    assert_eq!(apu.read8(0x81), 0x07);
    assert_eq!(apu.read8(0x8E), 0x17);
    assert_eq!(apu.read8(0x91), 0x8F);
    assert_eq!(apu.read8(0x93), 0x7F);
    assert_eq!(apu.read8(0x94), 0x0F);
    assert_eq!(apu.read8(0x6B), 0x70);
}
