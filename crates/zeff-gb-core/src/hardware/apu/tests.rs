use super::*;
use crate::hardware::types::constants::*;

#[test]
fn frame_sequencer_advances_every_8192_t_cycles() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    assert_eq!(apu.frame_seq_step, 0);

    apu.clock_div_apu();
    assert_eq!(apu.frame_seq_step, 1);

    for _ in 0..3 {
        apu.clock_div_apu();
    }
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
    apu.clock_div_apu();
    apu.clock_div_apu();
    apu.frame_seq_cycle_accum = 17;
    assert_eq!(apu.frame_seq_step, 2);
    assert_eq!(apu.frame_seq_cycle_accum, 17);

    apu.write(NR52, 0x00);
    assert_eq!(apu.frame_seq_step, 0);
    assert_eq!(apu.frame_seq_cycle_accum, 0);
}

#[test]
fn div_apu_power_on_skip_defers_first_frame_sequencer_clock() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.skip_next_div_apu_event_if(true);
    assert_eq!(apu.frame_seq_step, 1);

    apu.clock_div_apu();
    assert_eq!(apu.frame_seq_step, 1);

    apu.clock_div_apu();
    assert_eq!(apu.frame_seq_step, 1);

    apu.clock_div_apu();
    assert_eq!(apu.frame_seq_step, 2);
}

#[test]
fn channel_timer_clock_phase_resets_on_apu_power_on() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.step(2);
    assert_eq!(apu.pulse_noise_cycle_accum, 2);
    assert_eq!(apu.wave_cycle_accum, 0);

    apu.write(NR52, 0x00);
    apu.write(NR52, 0x80);

    assert_eq!(apu.pulse_noise_cycle_accum, 0);
    apu.write(NR12, 0xF0);
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x87);
    assert_eq!(apu.ch1_duty_pos, 0);
    assert_eq!(apu.ch1_output_delay, 12);

    apu.step(1);
    assert_eq!(apu.ch1_output_delay, 12);
    assert_eq!(apu.ch1_duty_pos, 0);
    apu.step(3);
    assert_eq!(apu.ch1_output_delay, 8);
    assert_eq!(apu.ch1_duty_pos, 0);

    apu.step(8);
    assert_eq!(apu.ch1_output_delay, 0);
    assert_eq!(apu.ch1_duty_pos, 1);

    apu.step(4);
    assert_eq!(apu.ch1_duty_pos, 2);
}

#[test]
fn square_trigger_delay_uses_double_speed_write_phase() {
    let mut apu = Apu::new();
    apu.set_cgb_double_speed(true);
    apu.write(NR52, 0x80);
    assert_eq!(apu.pulse_noise_cycle_accum, 1);
    assert_eq!(apu.square_trigger_phase_delay(), 1);
    assert_eq!(apu.square_trigger_start_delay(false), 4);

    apu.step(2);
    assert_eq!(apu.pulse_noise_cycle_accum, 3);
    assert_eq!(apu.square_trigger_phase_delay(), 3);
    assert_eq!(apu.square_trigger_start_delay(false), 8);

    apu.step(2);
    assert_eq!(apu.pulse_noise_cycle_accum, 1);
    assert_eq!(apu.square_trigger_phase_delay(), 1);
    assert_eq!(apu.square_trigger_start_delay(false), 4);
}

#[test]
fn square_frequency_write_during_trigger_delay_updates_next_period_only() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR13, 0xFC);
    apu.write(NR14, 0x87);

    let old_output_delay = apu.ch1_output_delay;
    assert_eq!(apu.ch1_timer, 16);

    apu.write(NR13, 0xFE);

    assert_eq!(apu.ch1_output_delay, old_output_delay);
    assert_eq!(apu.ch1_timer, 8);
}

#[test]
fn double_speed_nr14_high_write_on_reload_boundary_updates_period() {
    let mut apu = Apu::new();
    apu.set_cgb_double_speed(true);
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR13, 0xFC);
    apu.write(NR14, 0x87);

    apu.channels[0].enabled = true;
    apu.ch1_output_delay = 0;
    apu.ch1_just_reloaded = false;
    apu.ch1_timer = apu.square_period_t_cycles(0x07FC) - 1;

    apu.write(NR14, 0x00);

    assert_eq!(apu.ch1_timer, apu.square_period_t_cycles(0x00FC));
}

#[test]
fn noise_frequency_write_clamps_countdown_when_new_divisor_is_shorter() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.regs[(NR43 - NR10) as usize] = 0x09;
    apu.channels[3].enabled = true;
    apu.ch4_counter_countdown = 4;

    apu.write(NR43, 0x38);

    assert_eq!(apu.ch4_counter_countdown, 2);
}

#[test]
fn noise_frequency_write_reloads_countdown_when_new_divisor_is_slower_on_boundary() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.regs[(NR43 - NR10) as usize] = 0x18;
    apu.channels[3].enabled = true;
    apu.ch4_counter_countdown = 2;
    apu.ch4_alignment = 16;

    apu.write(NR43, 0x1A);

    assert_eq!(apu.ch4_counter_countdown, 10);
}

#[test]
fn noise_frequency_write_preserves_partial_countdown_when_slower_write_is_not_on_boundary() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.regs[(NR43 - NR10) as usize] = 0x09;
    apu.channels[3].enabled = true;
    apu.ch4_counter_countdown = 2;
    apu.ch4_alignment = 16;

    apu.write(NR43, 0x1A);

    assert_eq!(apu.ch4_counter_countdown, 2);
}

#[test]
fn envelope_period_one_enable_schedules_cgb_forced_tick() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0x08);
    apu.write(NR14, 0x80);

    apu.write(NR12, 0x09);

    assert_eq!(apu.channels[0].envelope_volume, 1);
    assert_eq!(apu.channels[0].envelope_forced_tick_delay, 1);

    apu.clock_div_apu();

    assert_eq!(apu.channels[0].envelope_volume, 2);
    assert_eq!(apu.channels[0].envelope_forced_tick_delay, 0);
}

#[test]
fn envelope_period_two_enable_does_not_schedule_cgb_forced_tick() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0x08);
    apu.write(NR14, 0x80);

    apu.write(NR12, 0x0A);

    assert_eq!(apu.channels[0].envelope_volume, 1);
    assert_eq!(apu.channels[0].envelope_forced_tick_delay, 0);

    apu.clock_div_apu();

    assert_eq!(apu.channels[0].envelope_volume, 1);
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

    apu.clock_div_apu();

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

    apu.clock_div_apu(); // step 0 clocks length

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

    apu.step(8);
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
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.step(8);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.step(4);

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
fn active_wave_ram_access_aliases_current_wave_byte() {
    let mut apu = Apu::new();
    apu.set_cgb_hardware(true);
    apu.write(NR52, 0x80);
    apu.write(WAVE_RAM_START, 0x12);
    apu.write(WAVE_RAM_START + 6, 0x34);
    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);

    apu.write(WAVE_RAM_START + 6, 0xAB);

    assert_eq!(apu.wave_ram[0], 0xAB);
    assert_eq!(apu.wave_ram[6], 0x34);
    assert_eq!(apu.read(WAVE_RAM_START + 6), 0xAB);
}

#[test]
fn wave_restart_keeps_current_sample_during_restart_delay() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(WAVE_RAM_START, 0xDF);
    apu.write(NR30, 0x80);
    apu.write(NR32, 0x20);
    apu.write(NR34, 0x87);
    apu.step(600);
    apu.ch3_wave_pos = 1;
    assert_eq!(apu.ch3_pcm_output(), 0x0F);

    apu.write(NR34, 0x87);

    assert!(apu.ch3_output_delay > 0);
    assert!(apu.ch3_restart_pending);
    assert_eq!(apu.ch3_pcm_output(), 0x0F);
}

#[test]
fn wave_restart_delay_cpu_access_restarts_from_first_byte() {
    let mut apu = Apu::new();
    apu.set_cgb_hardware(true);
    apu.write(NR52, 0x80);
    apu.write(WAVE_RAM_START, 0x00);
    apu.write(WAVE_RAM_START + 1, 0x11);
    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);
    apu.ch3_output_delay = 0;
    apu.ch3_wave_pos = 2;
    assert_eq!(apu.read(WAVE_RAM_START + 12), 0x11);

    apu.write(NR34, 0x80);

    assert!(apu.ch3_restart_pending);
    assert_ne!(apu.ch3_output_delay, 0);
    assert_eq!(apu.read(WAVE_RAM_START + 12), 0x00);
}

#[test]
fn wave_frequency_write_during_trigger_delay_updates_next_period() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR30, 0x80);
    apu.write(NR33, 0x00);
    apu.write(NR34, 0x87);
    assert_eq!(apu.ch3_output_delay, 518);
    assert_eq!(apu.ch3_timer, 512);

    apu.write(NR33, 0xF0);

    assert_eq!(apu.ch3_output_delay, 518);
    assert_eq!(apu.ch3_timer, 32);
}

#[test]
fn dmg_active_wave_ram_is_locked_outside_fetch_window() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(WAVE_RAM_START + 1, 0x11);
    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);
    apu.ch3_wave_pos = 2;
    apu.ch3_wave_access_index = 1;
    apu.ch3_wave_access_window = 0;

    assert_eq!(apu.read(WAVE_RAM_START), 0xFF);

    apu.ch3_wave_access_window = 8;
    assert_eq!(apu.read(WAVE_RAM_START), 0x11);
}

#[test]
fn dmg_wave_retrigger_copies_current_fetch_block_to_start() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    for i in 0..16 {
        apu.write(WAVE_RAM_START + i, i as u8);
    }
    apu.write(NR30, 0x80);
    apu.write(NR34, 0x80);
    apu.ch3_wave_pos = 11;
    apu.ch3_output_delay = 0;
    apu.ch3_timer = 2;

    apu.write(NR34, 0x80);

    assert_eq!(&apu.wave_ram[..4], &[4, 5, 6, 7]);
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
fn dmg_powered_off_nr41_updates_length_counter_without_register_write() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR41, 0x2A);

    apu.write(NR52, 0x00);
    assert_eq!(apu.regs[(NR41 - NR10) as usize], 0x00);
    assert_eq!(apu.channels[3].length_counter, 22);

    apu.write(NR41, 0x3E);
    assert_eq!(apu.regs[(NR41 - NR10) as usize], 0x00);
    assert_eq!(apu.channels[3].length_counter, 2);
}

#[test]
fn cgb_power_off_clears_nr41_and_ignores_powered_off_nr41_writes() {
    let mut apu = Apu::new();
    apu.set_cgb_hardware(true);
    apu.write(NR52, 0x80);
    apu.write(NR41, 0x2A);

    apu.write(NR52, 0x00);
    apu.write(NR41, 0x3E);

    assert_eq!(apu.regs[(NR41 - NR10) as usize], 0x00);
    assert_eq!(apu.channels[3].length_counter, 0);
}

#[test]
fn dmg_power_off_preserves_length_counters() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR11, 0x3E);
    apu.write(NR21, 0x3D);
    apu.write(NR31, 0xFC);
    apu.write(NR41, 0x3B);

    apu.write(NR52, 0x00);

    assert_eq!(apu.regs[(NR11 - NR10) as usize], 0x00);
    assert_eq!(apu.regs[(NR21 - NR10) as usize], 0x00);
    assert_eq!(apu.regs[(NR31 - NR10) as usize], 0x00);
    assert_eq!(apu.regs[(NR41 - NR10) as usize], 0x00);
    assert_eq!(apu.channels[0].length_counter, 2);
    assert_eq!(apu.channels[1].length_counter, 3);
    assert_eq!(apu.channels[2].length_counter, 4);
    assert_eq!(apu.channels[3].length_counter, 5);
}

#[test]
fn dmg_powered_off_length_writes_load_all_channel_counters() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR52, 0x00);

    apu.write(NR41, 0xCD);
    apu.write(NR31, 0xBC);
    apu.write(NR11, 0xEF);
    apu.write(NR21, 0xDE);

    assert_eq!(apu.channels[3].length_counter, 0x33);
    assert_eq!(apu.channels[2].length_counter, 0x44);
    assert_eq!(apu.channels[0].length_counter, 0x11);
    assert_eq!(apu.channels[1].length_counter, 0x22);
}

#[test]
fn cgb_power_off_clears_length_counters() {
    let mut apu = Apu::new();
    apu.set_cgb_hardware(true);
    apu.write(NR52, 0x80);
    apu.write(NR11, 0x3E);
    apu.write(NR21, 0x3D);
    apu.write(NR31, 0xFC);
    apu.write(NR41, 0x3B);

    apu.write(NR52, 0x00);

    assert_eq!(apu.channels[0].length_counter, 0);
    assert_eq!(apu.channels[1].length_counter, 0);
    assert_eq!(apu.channels[2].length_counter, 0);
    assert_eq!(apu.channels[3].length_counter, 0);
}

#[test]
fn sweep_period_zero_counts_as_8_but_suppresses_calculation_until_nonzero() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x01);
    apu.write(NR13, 100);
    apu.write(NR14, 0x80);

    apu.step(8);
    apu.frame_seq_step = 2;
    for _ in 0..7 {
        apu.frame_sequencer_step();
    }
    assert_eq!(apu.ch1_frequency(), 100);

    apu.frame_sequencer_step();
    assert_eq!(apu.ch1_frequency(), 100);

    apu.write(NR10, 0x11);
    for _ in 0..7 {
        apu.frame_sequencer_step();
    }
    assert_eq!(apu.ch1_frequency(), 100);

    apu.frame_sequencer_step();
    assert_eq!(apu.ch1_frequency(), 150);
}

#[test]
fn sweep_shift_zero_checks_overflow_on_timer_without_updating_frequency() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x10);
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x87);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.step(8);
    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();

    assert_eq!(apu.ch1_frequency(), 0x07FF);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);

    apu.step(8);

    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn ch1_sweep_clock_inside_trigger_visibility_delay_does_not_use_restart_frequency() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x10);
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x83);
    apu.step(8);

    apu.write(NR14, 0x87);
    apu.step(4);

    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();
    apu.step(16);

    assert_eq!(apu.ch1_frequency(), 0x07FF);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
}

#[test]
fn sweep_shift_zero_does_not_update_frequency_when_calculation_fits() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x10);
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x83);

    apu.step(8);
    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();

    assert_eq!(apu.ch1_frequency(), 0x03FF);
    assert_eq!(apu.nr52_raw() & 0x01, 0x01);
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
fn trigger_negate_calculation_marks_negate_used() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x09);
    apu.write(NR13, 0x00);
    apu.write(NR14, 0x80);

    apu.write(NR10, 0x10);

    assert_eq!(apu.nr52_raw() & 0x01, 0x00);
}

#[test]
fn periodic_shift_zero_negate_calculation_marks_negate_used() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR10, 0x18);
    apu.write(NR13, 0x00);
    apu.write(NR14, 0x80);

    apu.step(8);
    apu.frame_seq_step = 2;
    apu.frame_sequencer_step();
    apu.write(NR10, 0x10);

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
fn trigger_length_one_with_enable_on_odd_step_reloads_then_clocks() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);
    apu.write(NR12, 0xF0);
    apu.write(NR11, 0x3F);
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
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x87);

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
    apu.write(NR13, 0xFF);
    apu.write(NR14, 0x87);
    apu.step(12);

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

#[test]
fn cgb_pcm12_reads_current_square_channel_outputs() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);

    apu.channels[0].enabled = true;
    apu.channels[0].envelope_volume = 9;
    apu.regs[(NR11 - NR10) as usize] = 0x80;
    apu.ch1_current_duty = 0x02;
    apu.ch1_duty_pos = 0;

    apu.channels[1].enabled = true;
    apu.channels[1].envelope_volume = 5;
    apu.regs[(NR21 - NR10) as usize] = 0xC0;
    apu.ch2_current_duty = 0x03;
    apu.ch2_duty_pos = 0;

    assert_eq!(apu.read(CGB_PCM12), 0x09);

    apu.ch1_duty_pos = 4;
    apu.ch2_duty_pos = 6;
    assert_eq!(apu.read(CGB_PCM12), 0x50);
}

#[test]
fn cgb_pcm34_reads_current_wave_and_noise_outputs() {
    let mut apu = Apu::new();
    apu.write(NR52, 0x80);

    apu.channels[2].enabled = true;
    apu.regs[(NR30 - NR10) as usize] = 0x80;
    apu.regs[(NR32 - NR10) as usize] = 0x20;
    apu.wave_ram[0] = 0xA0;
    apu.ch3_wave_pos = 0;

    apu.channels[3].enabled = true;
    apu.channels[3].envelope_volume = 7;
    apu.ch4_lfsr = 0;

    assert_eq!(apu.read(CGB_PCM34), 0x7A);

    apu.regs[(NR32 - NR10) as usize] = 0x40;
    apu.ch4_lfsr = 1;
    assert_eq!(apu.read(CGB_PCM34), 0x05);
}
