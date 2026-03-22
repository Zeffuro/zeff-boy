use super::*;
use crate::hardware::types::constants::*;

#[test]
fn frame_sequencer_advances_every_8192_t_cycles() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    assert_eq!(apu.frame_seq_step, 0);

    apu.step(8191);
    assert_eq!(apu.frame_seq_step, 0);

    apu.step(1);
    assert_eq!(apu.frame_seq_step, 1);

    apu.step(8192 * 3);
    assert_eq!(apu.frame_seq_step, 4);
}

#[test]
fn step_does_not_advance_when_powered_off() {
    let mut apu = Apu::new();
    apu.step(8192 * 4);
    assert_eq!(apu.frame_seq_step, 0);
    assert_eq!(apu.nr52_raw() & 0x80, 0);
}

#[test]
fn power_off_resets_frame_sequencer_state() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.step(8192 * 2 + 17);
    assert_eq!(apu.frame_seq_step, 2);
    assert_eq!(apu.frame_seq_cycle_accum, 17);

    apu.write(NR52, 0x00);
    assert_eq!(apu.frame_seq_step, 0);
    assert_eq!(apu.frame_seq_cycle_accum, 0);
}

#[test]
fn trigger_reloads_zero_length_counter() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR14, 0x80);
    assert_eq!(apu.channels[0].length_counter, 64);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
}

#[test]
fn length_tick_requires_length_enable() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x3F);
    apu.write(NR14, 0x80);

    apu.step(8192);

    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
    assert_eq!(apu.channels[0].length_counter, 1);
}

#[test]
fn length_tick_disables_channel_when_enabled_and_counter_expires() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x3F);
    apu.write(NR14, 0xC0);

    apu.step(8192); // step 0 clocks length

    assert_eq!(apu.channels[0].length_counter, 0);
    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn envelope_ticks_on_step_7_for_channel_1() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0x19);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 7;
    apu.frame_sequencer_step();

    assert_eq!(apu.channels[0].envelope_volume, 2);
}

#[test]
fn envelope_decrease_clamps_at_zero() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0x01);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 7;
    apu.frame_sequencer_step();

    assert_eq!(apu.channels[0].envelope_volume, 0);
}

#[test]
fn sweep_tick_updates_ch1_frequency() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x11);
    apu.write(NR13, 100);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();

    assert_eq!(apu.ch1_frequency(), 150);
}

#[test]
fn sweep_overflow_disables_channel_1() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x11);
    apu.write(NR13, 0xF8);
    apu.write(NR14, 0x87);

    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();

    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn ch1_trigger_with_dac_off_does_not_enable_channel() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0x00);
    apu.write(NR14, 0x80);

    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn ch1_dac_off_write_disables_active_channel() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR14, 0x80);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.write(NR12, 0x00);
    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn ch3_trigger_requires_dac_enable() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);

    apu.write(NR30, 0x00);
    apu.write(NR34, 0x80);
    assert_eq!(apu.nr52_raw() & 0x04, 0x00);

    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);
    assert_eq!(apu.nr52_raw() & 0x04, 0x04);
}

#[test]
fn ch3_dac_off_write_disables_active_channel() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);
    assert_eq!(apu.nr52_raw() & 0x04, 0x04);

    apu.write(NR30, 0x00);
    assert_eq!(apu.nr52_raw() & 0x04, 0x00);
}

#[test]
fn sweep_period_zero_still_ticks_with_period_8() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x01);
    apu.write(NR13, 100);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 2;
    for _ in 0..7 {
        apu.frame_sequencer_step();
    }
    assert_eq!(apu.ch1_frequency(), 100);

    apu.frame_sequencer_step();
    assert_eq!(apu.ch1_frequency(), 150);
}

#[test]
fn clearing_sweep_negate_after_subtraction_disables_ch1() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x19);
    apu.write(NR13, 100);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.write(NR10, 0x11);
    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn length_enable_rising_edge_on_odd_step_immediately_clocks_length() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x3F);
    apu.write(NR14, 0x80);
    assert_eq!(apu.channels[0].length_counter, 1);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.frame_seq_step = 1;
    apu.write(NR14, 0x40);

    assert_eq!(apu.channels[0].length_counter, 0);
    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn length_enable_rising_edge_on_even_step_does_not_clock_immediately() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x3F);
    apu.write(NR14, 0x80);

    apu.frame_seq_step = 0;
    apu.write(NR14, 0x40);

    assert_eq!(apu.channels[0].length_counter, 1);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
}

#[test]
fn trigger_with_zero_length_and_length_enable_on_odd_step_loads_max_minus_one() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.frame_seq_step = 1;

    apu.write(NR14, 0xC0);

    assert_eq!(apu.channels[0].length_counter, 63);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
}

#[test]
fn debug_waveform_capture_advances_for_enabled_channel() {
    let mut apu = Apu::new();
    apu.debug_capture_enabled = true;
    apu.write(NR52, 0x80);
    apu.write(NR50, 0x77);
    apu.write(NR51, 0x11);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x80);
    apu.write(NR14, 0x80);

    apu.step(64 * 16);

    let ordered = apu.channel_debug_samples_ordered(0);
    assert!(ordered.iter().any(|sample| sample.abs() > 0.0001));
}

#[test]
fn channel_mute_only_affects_audio_mix_output() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR50, 0x77);
    apu.write(NR51, 0x11);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x80);
    apu.write(NR14, 0x80);

    let (left_on, right_on) = apu.mix_sample();
    assert!(left_on.abs() > 0.0 || right_on.abs() > 0.0);

    apu.set_channel_mutes([true, false, false, false]);
    let (left_muted, right_muted) = apu.mix_sample();
    assert_eq!(left_muted, 0.0);
    assert_eq!(right_muted, 0.0);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
}

#[test]
fn drain_samples_keeps_sample_buffer_capacity() {
    let mut apu = Apu::new();
    let initial_capacity = apu.sample_buffer.capacity();
    assert!(initial_capacity >= APU_INITIAL_SAMPLE_CAPACITY);

    apu.sample_buffer.extend_from_slice(&[0.1, -0.2, 0.3, -0.4]);
    let drained = apu.drain_samples();

    assert_eq!(drained, vec![0.1, -0.2, 0.3, -0.4]);
    assert!(apu.sample_buffer.is_empty());
    assert_eq!(apu.sample_buffer.capacity(), initial_capacity);
}
