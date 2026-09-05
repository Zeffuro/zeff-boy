use super::{
    ATTENUATION_GAIN, BLIP_FRAME_CLOCKS, DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT, HuC6280Psg,
    MAX_PSG_STATE_AUDIO_SAMPLES, MAX_PSG_STATE_SECTION_BYTES, MIX_SCALE,
    PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS, PROVISIONAL_PSG_NOISE_ZERO_PERIOD,
    PSG_CHANNEL_COUNT, PSG_CLOCK_DENOMINATOR, PSG_INTERNAL_CLOCK_NUMERATOR,
    PSG_INTERNAL_MASTER_CLOCK_DIVISOR, PsgRevision, StereoBlipResampler, attenuation_slot,
    blip_level, effective_period, lfo_target_period,
};
use crate::hardware::PsgPort;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[path = "tests/batching.rs"]
mod batching;

fn phase_state(psg: &HuC6280Psg) -> [(u8, i32, u16, u32); 6] {
    std::array::from_fn(|index| {
        let channel = &psg.channels[index];
        (
            channel.wave_index,
            channel.wave_counter,
            channel.noise_counter,
            channel.noise_seed,
        )
    })
}

fn seed_effective_gains(psg: &mut HuC6280Psg) {
    for channel in &mut psg.channels {
        channel.effective_left_attenuation = attenuation_slot(
            psg.main_amplitude >> 4,
            channel.amplitude(),
            channel.balance >> 4,
        );
        channel.effective_right_attenuation =
            attenuation_slot(psg.main_amplitude, channel.amplitude(), channel.balance);
    }
    psg.gain_scan_clock = 0;
    psg.gain_scan_active = false;
    psg.gain_scan_queued = false;
    psg.resampler = psg.resampler_at_current_level();
}

#[test]
fn lfo_signed_source_and_target_clamps_are_bounded() {
    assert_eq!(lfo_target_period(100, 0x10, 2), 100);
    assert_eq!(lfo_target_period(100, 0x0F, 2), 96);
    assert_eq!(lfo_target_period(100, 0x00, 2), 36);
    assert_eq!(lfo_target_period(1, 0x00, 3), 1);
    assert_eq!(lfo_target_period(0x1FFF, 0x1F, 3), 0x1FFF);
}

#[test]
fn attenuation_components_follow_exact_slots_and_cutoff() {
    assert_eq!(ATTENUATION_GAIN[30], 0);
    assert_eq!(ATTENUATION_GAIN[31], 0);
    for (slot, gain) in ATTENUATION_GAIN.iter().enumerate().take(30) {
        let expected = (1_000_000.0 * 10_f64.powf(-1.5 * slot as f64 / 20.0)).round();
        assert_eq!(f64::from(*gain), expected);
    }

    for main in 0..16 {
        for channel in 0..32 {
            for balance in 0..16 {
                let expected = (2 * (15 - main) + (31 - channel) + 2 * (15 - balance)).min(31);
                let slot = attenuation_slot(main, channel, balance);
                assert_eq!(slot, expected);
                assert_eq!(
                    ATTENUATION_GAIN[usize::from(slot)] == 0,
                    main == 0 || channel == 0 || balance == 0 || expected >= 30
                );
            }
        }
    }

    assert_eq!(attenuation_slot(0xC, 0x1C, 0xF), 9);
    assert_eq!(attenuation_slot(0x8, 0x1C, 0x8), 31);
    assert_eq!(attenuation_slot(0xC, 0x1D, 0xF), 8);
    assert_eq!(attenuation_slot(0x8, 0x1D, 0x8), 30);
}

#[test]
fn provisional_gain_scan_applies_right_then_left_across_six_live_channels() {
    let mut psg = HuC6280Psg::new();
    psg.main_amplitude = 0xFF;
    for channel in &mut psg.channels {
        channel.control = 0x1F;
        channel.balance = 0xFF;
    }
    psg.queue_gain_scan();

    for component in 0..PSG_CHANNEL_COUNT * 2 {
        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        let channel = component / 2;
        if component & 1 == 0 {
            assert_eq!(
                psg.channels[channel].effective_right_attenuation,
                DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT
            );
        } else {
            assert_eq!(
                psg.channels[channel].effective_left_attenuation,
                DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT
            );
        }
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        if component & 1 == 0 {
            assert_eq!(psg.channels[channel].effective_right_attenuation, 0);
        } else {
            assert_eq!(psg.channels[channel].effective_left_attenuation, 0);
        }
    }

    assert_eq!(psg.gain_scan_clock, 3_072);
    assert!(psg.gain_scan_active);
    psg.advance_master_ticks(1_024 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.gain_scan_clock, 0);
    assert!(!psg.gain_scan_active);
}

#[test]
fn gain_scan_latch_is_stable_and_midpass_writes_coalesce_one_followup_pass() {
    let mut psg = HuC6280Psg::new();
    psg.main_amplitude = 0xFF;
    psg.channels[0].control = 0x1F;
    psg.channels[0].balance = 0xFF;
    psg.queue_gain_scan();
    psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.attenuation_latch, 0);

    psg.write_port(PsgPort::from_offset(1), 0xF0);
    psg.write_port(PsgPort::from_offset(1), 0xF0);
    psg.write_port(PsgPort::from_offset(5), 0xFF);
    assert!(psg.gain_scan_queued);
    psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.channels[0].effective_right_attenuation, 0);

    psg.advance_master_ticks(3_840 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert!(psg.gain_scan_active);
    assert!(!psg.gain_scan_queued);
    assert_eq!(psg.gain_scan_clock, 0);
    psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.attenuation_latch, 30);
    psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.channels[0].effective_right_attenuation, 30);
    psg.advance_master_ticks(3_840 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert!(!psg.gain_scan_active);

    psg.write_port(PsgPort::from_offset(1), 0xF0);
    assert!(psg.gain_scan_active);
    assert_eq!(psg.gain_scan_clock, 0);
}

#[test]
fn gain_apply_after_a_drain_uses_the_end_of_the_next_internal_clock() {
    let mut psg = HuC6280Psg::new();
    psg.set_sample_rate(192_000);
    psg.main_amplitude = 0xFF;
    psg.channels[0].control = 0xDF;
    psg.channels[0].balance = 0xFF;
    psg.channels[0].dda_hold = 31;
    psg.queue_gain_scan();

    psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().all(|sample| *sample == 0.0));
    assert_eq!(psg.resampler_clock(), 0);
    assert_eq!(psg.channels[0].effective_right_attenuation, 31);

    psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
    assert_eq!(psg.channels[0].effective_right_attenuation, 0);
    assert_eq!(psg.channels[0].effective_left_attenuation, 31);
    assert_eq!(psg.resampler_clock(), 1);
    assert_eq!(psg.resampler.last_delta_clock, Some(1));
}

#[test]
fn level_edge_at_blip_frame_boundary_starts_the_next_frame() {
    let mut resampler = StereoBlipResampler::new(192_000);
    let mut samples = Vec::new();
    resampler.clocks = BLIP_FRAME_CLOCKS - 1;

    resampler.push_level(MIX_SCALE, -MIX_SCALE, &mut samples);

    assert_eq!(resampler.clocks, 0);
    assert_eq!(resampler.last_delta_clock, Some(0));
    assert_ne!(resampler.left_level, 0);
    assert_ne!(resampler.right_level, 0);
    assert!(!samples.is_empty());

    resampler.push_level(MIX_SCALE, -MIX_SCALE, &mut samples);
    assert_eq!(resampler.clocks, 1);
    assert_eq!(resampler.last_delta_clock, Some(0));
    resampler.flush(&mut samples);
}

#[path = "tests/state.rs"]
mod state;

#[test]
fn rate_change_preserves_a_sampled_gain_latch_midpass() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::new();
        psg.main_amplitude = 0xFF;
        psg.channels[0].control = 0x1F;
        psg.channels[0].balance = 0xFF;
        psg.queue_gain_scan();
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.attenuation_latch, 0);

        psg.set_sample_rate(sample_rate);
        assert_eq!(psg.gain_scan_clock, 1);
        assert_eq!(psg.attenuation_latch, 0);
        psg.write_port(PsgPort::from_offset(1), 0xF0);
        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_right_attenuation, 0);
    }
}

#[test]
fn key_on_with_new_level_uses_old_gain_until_each_scan_apply() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.main_amplitude = 0xFF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].dda_hold = 31;
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        assert_eq!(psg.resampler_levels(), (0, 0));

        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.resampler_levels(), (0, 0));
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_right_attenuation, 0);
        assert_eq!(psg.channels[0].effective_left_attenuation, 31);
        assert_eq!(psg.resampler.left_level, 0);
        assert_ne!(psg.resampler.right_level, 0);

        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.resampler.left_level, 0);
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_left_attenuation, 0);
        assert_ne!(psg.resampler.left_level, 0);
    }
}

#[test]
fn master_divide_three_clock_preserves_divide_six_oscillator_pitch() {
    let mut psg = HuC6280Psg::new();
    psg.channels[0].frequency = 1;
    psg.channels[0].control = 0x9F;
    psg.channels[0].wave_counter = 1;

    psg.advance_master_ticks(3);
    assert_eq!(psg.resampler_clock(), 1);
    assert_eq!(psg.channels[0].wave_index, 0);
    assert_eq!(psg.master_tick_remainder(), 3);
    psg.advance_master_ticks(3);
    assert_eq!(psg.resampler_clock(), 2);
    assert_eq!(psg.channels[0].wave_index, 1);
    assert_eq!(psg.master_tick_remainder(), 0);
}

#[test]
fn disabled_generation_advances_gain_scan_and_chunking_is_equivalent() {
    fn configured(sample_rate: u32) -> HuC6280Psg {
        let mut psg = HuC6280Psg::new();
        psg.set_sample_rate(sample_rate);
        psg.channels[0].control = 0xDF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].dda_hold = 31;
        psg.set_sample_generation_enabled(false);
        psg.write_port(PsgPort::from_offset(1), 0xFF);
        psg
    }

    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = configured(sample_rate);
        psg.advance_master_ticks(
            u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS)
                * PSG_INTERNAL_MASTER_CLOCK_DIVISOR,
        );
        assert_eq!(psg.channels[0].effective_left_attenuation, 0);
        assert_eq!(psg.channels[0].effective_right_attenuation, 0);
        psg.set_sample_generation_enabled(true);
        assert_ne!(psg.resampler_levels(), (0, 0));
        assert_eq!(psg.resampler_clock(), 0);
    }

    let total = u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS)
        * PSG_INTERNAL_MASTER_CLOCK_DIVISOR
        + 2;
    let mut bulk = configured(48_000);
    let mut chunked = configured(48_000);
    bulk.advance_master_ticks(total);
    let mut remaining = total;
    for chunk in [1_u64, 5, 17, 64, 511].into_iter().cycle() {
        if remaining == 0 {
            break;
        }
        let chunk = chunk.min(remaining);
        chunked.advance_master_ticks(chunk);
        remaining -= chunk;
    }
    assert_eq!(bulk.channels, chunked.channels);
    assert_eq!(bulk.gain_scan_clock, chunked.gain_scan_clock);
    assert_eq!(bulk.gain_scan_active, chunked.gain_scan_active);
    assert_eq!(bulk.gain_scan_queued, chunked.gain_scan_queued);
    assert_eq!(bulk.attenuation_latch, chunked.attenuation_latch);
    assert_eq!(bulk.master_tick_remainder, chunked.master_tick_remainder);
}

#[test]
fn defined_noise_frequencies_advance_at_exact_periods() {
    for register in 0..31 {
        let noise_factor = register ^ 0x1F;
        let mut psg = HuC6280Psg::new();
        psg.sample_generation_enabled = false;
        psg.write_port(PsgPort::from_offset(0), 4);
        psg.write_port(PsgPort::from_offset(4), 0x80);
        psg.write_port(PsgPort::from_offset(7), 0x80 | register);

        psg.advance_psg_tick();
        let seed = psg.channels[4].noise_seed;
        for _ in 1..u16::from(noise_factor) * 64 {
            psg.advance_psg_tick();
            assert_eq!(
                psg.channels[4].noise_seed, seed,
                "R7={register:02x}, NF={noise_factor}"
            );
        }
        psg.advance_psg_tick();
        assert_ne!(
            psg.channels[4].noise_seed, seed,
            "R7={register:02x}, NF={noise_factor}"
        );
    }
}

#[test]
fn provisional_zero_noise_frequency_advances_every_tick() {
    assert_eq!(PROVISIONAL_PSG_NOISE_ZERO_PERIOD, 1);
    let mut psg = HuC6280Psg::new();
    psg.sample_generation_enabled = false;
    psg.write_port(PsgPort::from_offset(0), 4);
    psg.write_port(PsgPort::from_offset(4), 0x80);
    psg.write_port(PsgPort::from_offset(7), 0x9F);

    let mut seed = psg.channels[4].noise_seed;
    for _ in 0..8 {
        psg.advance_psg_tick();
        assert_ne!(psg.channels[4].noise_seed, seed);
        seed = psg.channels[4].noise_seed;
    }
}

#[test]
fn noise_phase_free_runs_and_register_writes_change_only_the_next_reload() {
    let mut psg = HuC6280Psg::new();
    psg.sample_generation_enabled = false;
    let channel = &psg.channels[4];
    assert_eq!(channel.noise_counter, 0);
    assert_eq!(channel.noise_seed, 1);

    psg.advance_psg_tick();
    let seed = psg.channels[4].noise_seed;
    assert_ne!(seed, 1);
    assert_eq!(psg.channels[4].noise_counter, 31 * 64);

    psg.selected_channel = 4;
    for _ in 0..10 {
        psg.advance_psg_tick();
    }
    let counter = psg.channels[4].noise_counter;
    psg.write_port(PsgPort::from_offset(7), 0x9F);
    assert_eq!(psg.channels[4].noise_counter, counter);
    assert_eq!(psg.channels[4].noise_seed, seed);

    for _ in 1..counter {
        psg.advance_psg_tick();
        assert_eq!(psg.channels[4].noise_seed, seed);
    }
    psg.advance_psg_tick();
    assert_ne!(psg.channels[4].noise_seed, seed);
    assert_eq!(psg.channels[4].noise_counter, 1);
}

#[test]
fn noise_key_and_enable_bits_gate_only_the_mixer() {
    let mut reference = HuC6280Psg::new();
    let mut toggled = HuC6280Psg::new();
    reference.sample_generation_enabled = false;
    toggled.sample_generation_enabled = false;
    toggled.selected_channel = 4;

    for step in 0..20_000 {
        if step % 97 == 0 {
            toggled.write_port(PsgPort::from_offset(4), 0x9F);
            toggled.write_port(PsgPort::from_offset(7), 0x80 | (step as u8 & 0x1F));
        } else if step % 53 == 0 {
            toggled.write_port(PsgPort::from_offset(4), 0);
            toggled.write_port(PsgPort::from_offset(7), step as u8 & 0x1F);
        }
        reference.channels[4].noise_control = toggled.channels[4].noise_control;
        reference.advance_psg_tick();
        toggled.advance_psg_tick();
        assert_eq!(
            toggled.channels[4].noise_counter,
            reference.channels[4].noise_counter
        );
        assert_eq!(
            toggled.channels[4].noise_seed,
            reference.channels[4].noise_seed
        );
    }
}

#[test]
fn noise_lfsr_stays_within_its_nonzero_eighteen_bit_state() {
    let mut psg = HuC6280Psg::new();
    psg.sample_generation_enabled = false;
    psg.selected_channel = 4;
    psg.write_port(PsgPort::from_offset(7), 0x1F);
    for _ in 0..=0x3_FFFF {
        psg.advance_psg_tick();
        assert_ne!(psg.channels[4].noise_seed, 0);
        assert_eq!(psg.channels[4].noise_seed & !0x3_FFFF, 0);
    }
}

#[test]
fn active_dda_rate_change_and_generation_resume_seed_the_live_level() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::new();
        psg.write_port(PsgPort::from_offset(0), 0);
        psg.write_port(PsgPort::from_offset(1), 0xFF);
        psg.write_port(PsgPort::from_offset(5), 0xFF);
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        psg.write_port(PsgPort::from_offset(6), 31);
        psg.set_sample_generation_enabled(false);
        psg.advance_master_ticks(u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS) * 6);
        psg.set_sample_generation_enabled(true);

        psg.set_sample_rate(sample_rate);
        let (left, right) = psg.mix_output();
        assert_eq!(psg.resampler.left_level, blip_level(left));
        assert_eq!(psg.resampler.right_level, blip_level(right));
        psg.advance_master_ticks(100_000);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));

        psg.set_sample_generation_enabled(false);
        psg.advance_master_ticks(100_000);
        psg.set_sample_generation_enabled(true);
        assert_eq!(psg.resampler.left_level, blip_level(left));
        assert_eq!(psg.resampler.right_level, blip_level(right));
        psg.advance_master_ticks(100_000);
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn output_register_writes_refresh_at_the_current_clock_without_advancing_phase() {
    for (revision, sample_rate) in [PsgRevision::HuC6280, PsgRevision::HuC6280A]
        .into_iter()
        .flat_map(|revision| {
            [44_100, 48_000, 96_000, 192_000]
                .into_iter()
                .map(move |sample_rate| (revision, sample_rate))
        })
    {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.set_sample_rate(sample_rate);
        psg.selected_channel = 0;
        psg.main_amplitude = 0xFF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].control = 0xDF;
        psg.channels[0].dda_hold = 8;
        seed_effective_gains(&mut psg);
        psg.advance_master_ticks(30);

        for (register, value) in [(1, 0xEF), (5, 0xEF), (4, 0xDD)] {
            let phase = phase_state(&psg);
            let clock = psg.resampler.clocks;
            let levels = psg.resampler_levels();
            psg.write_port(PsgPort::from_offset(register), value);
            assert_eq!(psg.resampler_levels(), levels);
            assert_eq!(psg.resampler.clocks, clock);
            assert_eq!(phase_state(&psg), phase);
        }

        let phase = phase_state(&psg);
        let clock = psg.resampler.clocks;
        let levels = psg.resampler_levels();
        psg.write_port(PsgPort::from_offset(6), 24);
        assert_ne!(psg.resampler_levels(), levels);
        assert_eq!(psg.resampler.clocks, clock);
        assert_eq!(phase_state(&psg), phase);

        psg.write_port(PsgPort::from_offset(4), 0x5F);
        let silent_level = (psg.resampler.left_level, psg.resampler.right_level);
        let phase = phase_state(&psg);
        psg.write_port(PsgPort::from_offset(6), 31);
        assert_eq!(psg.channels[0].dda_hold, 31);
        assert_eq!(psg.resampler.left_level, silent_level.0);
        assert_eq!(psg.resampler.right_level, silent_level.1);
        assert_eq!(phase_state(&psg), phase);

        let clock = psg.resampler.clocks;
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        assert_ne!(psg.resampler.left_level, silent_level.0);
        assert_ne!(psg.resampler.right_level, silent_level.1);
        assert_eq!(psg.resampler.clocks, clock);
        psg.advance_master_ticks(6_000);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    }
}

#[test]
fn mute_changes_refresh_immediately_without_advancing_channel_state() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.main_amplitude = 0xFF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].control = 0xDF;
        psg.channels[0].dda_hold = 31;
        seed_effective_gains(&mut psg);
        psg.advance_master_ticks(33);
        let audible = psg.resampler_levels();
        let phase = phase_state(&psg);
        let clock = psg.resampler_clock();

        psg.set_channel_mutes(&[true]);
        assert_eq!(psg.resampler_levels(), (0, 0));
        assert_eq!(psg.resampler_clock(), clock);
        assert_eq!(phase_state(&psg), phase);

        psg.set_channel_mutes(&[]);
        assert_eq!(psg.resampler_levels(), audible);
        assert_eq!(psg.resampler_clock(), clock);
        assert_eq!(phase_state(&psg), phase);

        psg.set_sample_generation_enabled(false);
        let phase = phase_state(&psg);
        psg.set_channel_mutes(&[true]);
        assert_eq!(psg.resampler_levels(), (0, 0));
        assert_eq!(psg.resampler_clock(), 0);
        assert_eq!(phase_state(&psg), phase);
        psg.set_sample_generation_enabled(true);
        assert_eq!(psg.resampler_levels(), (0, 0));
        psg.set_channel_mutes(&[]);
        assert_eq!(psg.resampler_levels(), audible);
        assert_eq!(psg.resampler_clock(), 0);
        assert_eq!(phase_state(&psg), phase);
    }
}

#[test]
fn control_writes_switch_dda_wave_and_noise_sources_at_the_write_boundary() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.main_amplitude = 0xFF;
        psg.selected_channel = 0;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].control = 0xDF;
        psg.channels[0].dda_hold = 31;
        psg.channels[0].waveform[0] = 4;
        seed_effective_gains(&mut psg);
        let dda_levels = psg.resampler_levels();
        let clock = psg.resampler_clock();

        psg.write_port(PsgPort::from_offset(4), 0x9F);
        let wave_levels = psg.resampler_levels();
        assert_ne!(wave_levels, dda_levels);
        assert_eq!(psg.resampler_clock(), clock);
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        assert_eq!(psg.resampler_levels(), dda_levels);
        assert_eq!(psg.resampler_clock(), clock);

        psg.selected_channel = 4;
        psg.channels[4].control = 0xDF;
        psg.channels[4].balance = 0xFF;
        psg.channels[4].dda_hold = 3;
        psg.channels[4].noise_control = 0x80;
        psg.resampler = psg.resampler_at_current_level();
        let noise_levels = psg.resampler_levels();
        let phase = phase_state(&psg);
        psg.write_port(PsgPort::from_offset(6), 29);
        assert_eq!(psg.channels[4].dda_hold, 29);
        assert_eq!(psg.resampler_levels(), noise_levels);
        assert_eq!(phase_state(&psg), phase);
    }
}

#[test]
fn same_value_writes_preserve_mix_but_apply_defined_register_side_effects() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::new();
        psg.set_sample_rate(sample_rate);
        psg.main_amplitude = 0xFF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].control = 0xDF;
        psg.channels[0].dda_hold = 31;
        psg.channels[0].wave_index = 7;
        psg.selected_channel = 0;
        psg.resampler = psg.resampler_at_current_level();
        let levels = psg.resampler_levels();
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        assert_eq!(psg.channels[0].wave_index, 0);
        assert_eq!(psg.resampler_levels(), levels);

        psg.selected_channel = 4;
        psg.channels[4].noise_control = 0x80;
        psg.channels[4].noise_counter = 100;
        let noise_state = (psg.channels[4].noise_counter, psg.channels[4].noise_seed);
        psg.write_port(PsgPort::from_offset(7), 0x80);
        assert_eq!(
            (psg.channels[4].noise_counter, psg.channels[4].noise_seed,),
            noise_state
        );
        assert_eq!(psg.resampler_levels(), levels);

        psg.channels[1].control = 0x9F;
        psg.channels[1].balance = 0xFF;
        psg.channels[1].wave_index = 5;
        psg.channels[1].waveform[5] = 31;
        psg.channels[1].waveform[0] = 31;
        psg.lfo_control = 0x80;
        psg.resampler = psg.resampler_at_current_level();
        let levels = psg.resampler_levels();
        psg.write_port(PsgPort::from_offset(9), 0x80);
        assert_eq!(psg.channels[1].wave_index, 0);
        assert_eq!(psg.resampler_levels(), levels);

        psg.advance_master_ticks(60);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn low_lfo_trigger_activation_resets_the_audible_source_index() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.main_amplitude = 0xFF;
        for channel in 0..=1 {
            psg.channels[channel].control = 0x9F;
            psg.channels[channel].balance = 0xFF;
        }
        psg.channels[1].wave_index = 6;
        psg.channels[1].waveform[6] = 31;
        psg.channels[1].waveform[0] = 0;
        seed_effective_gains(&mut psg);
        let before = psg.resampler_levels();
        let clock = psg.resampler_clock();

        psg.write_port(PsgPort::from_offset(9), 1);
        assert!(psg.lfo_active());
        assert_eq!(psg.channels[1].wave_index, 0);
        assert_ne!(psg.resampler_levels(), before);
        assert_eq!(psg.resampler_clock(), clock);
    }
}

#[test]
fn noise_and_lfo_source_writes_refresh_without_advancing_unrelated_state() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::new();
        psg.set_sample_rate(sample_rate);
        psg.main_amplitude = 0xFF;
        psg.selected_channel = 4;
        psg.channels[4].control = 0x9F;
        psg.channels[4].balance = 0xFF;
        seed_effective_gains(&mut psg);

        let phase = phase_state(&psg);
        let clock = psg.resampler.clocks;
        psg.write_port(PsgPort::from_offset(7), 0x80);
        assert_eq!(phase_state(&psg), phase);
        assert_eq!(psg.resampler.clocks, clock);
        let noise_levels = (psg.resampler.left_level, psg.resampler.right_level);
        assert_ne!(noise_levels, (0, 0));

        psg.write_port(PsgPort::from_offset(7), 0x81);
        assert_eq!(phase_state(&psg), phase);
        assert_eq!(
            (psg.resampler.left_level, psg.resampler.right_level),
            noise_levels
        );
        psg.write_port(PsgPort::from_offset(7), 0x01);
        assert_eq!(phase_state(&psg), phase);
        assert_eq!(
            (psg.resampler.left_level, psg.resampler.right_level),
            (0, 0)
        );

        psg.selected_channel = 1;
        psg.channels[1].control = 0x9F;
        psg.channels[1].balance = 0xFF;
        psg.channels[1].wave_index = 5;
        psg.channels[1].waveform[5] = 31;
        psg.channels[1].waveform[0] = 0;
        seed_effective_gains(&mut psg);
        let noise_phase = [
            (psg.channels[4].noise_counter, psg.channels[4].noise_seed),
            (psg.channels[5].noise_counter, psg.channels[5].noise_seed),
        ];
        let clock = psg.resampler.clocks;
        psg.write_port(PsgPort::from_offset(9), 0x80);
        assert_eq!(psg.channels[1].wave_index, 0);
        assert_eq!(psg.resampler.clocks, clock);
        assert_eq!(
            [
                (psg.channels[4].noise_counter, psg.channels[4].noise_seed),
                (psg.channels[5].noise_counter, psg.channels[5].noise_seed),
            ],
            noise_phase
        );
        assert_eq!(
            (psg.resampler.left_level, psg.resampler.right_level),
            (0, 0)
        );
    }
}

#[test]
fn waveform_programming_and_keyed_non_dda_writes_follow_distinct_paths() {
    for (revision, sample_rate) in [PsgRevision::HuC6280, PsgRevision::HuC6280A]
        .into_iter()
        .flat_map(|revision| {
            [44_100, 48_000, 96_000, 192_000]
                .into_iter()
                .map(move |sample_rate| (revision, sample_rate))
        })
    {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.set_sample_rate(sample_rate);
        psg.write_port(PsgPort::from_offset(6), 31);
        assert_eq!(
            (psg.resampler.left_level, psg.resampler.right_level),
            (0, 0)
        );
        assert_eq!(psg.resampler.clocks, 0);

        psg.main_amplitude = 0xFF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].control = 0x9F;
        psg.channels[0].waveform[psg.channels[0].wave_index as usize] = 31;
        seed_effective_gains(&mut psg);
        let index = psg.channels[0].wave_index;
        let counter = psg.channels[0].wave_counter;
        let levels = (psg.resampler.left_level, psg.resampler.right_level);
        psg.write_port(PsgPort::from_offset(6), 0);
        assert_eq!(psg.channels[0].waveform[usize::from(index)], 0);
        assert_eq!(psg.channels[0].wave_index, index);
        assert_eq!(psg.channels[0].wave_counter, counter);
        assert_ne!(psg.resampler_levels(), levels);
        assert_eq!(psg.resampler.clocks, 0);

        psg.selected_channel = 4;
        psg.channels[4].control = 0x9F;
        psg.channels[4].balance = 0xFF;
        psg.channels[4].noise_control = 0x80;
        psg.channels[4].wave_index = 7;
        psg.channels[4].wave_counter = 123;
        seed_effective_gains(&mut psg);
        let levels = psg.resampler_levels();
        psg.write_port(PsgPort::from_offset(6), 29);
        assert_eq!(psg.channels[4].waveform[7], 29);
        assert_eq!(psg.channels[4].wave_index, 7);
        assert_eq!(psg.channels[4].wave_counter, 123);
        assert_eq!(psg.resampler_levels(), levels);
    }
}

#[test]
fn keyed_wave_write_holds_until_the_exact_next_oscillator_clock() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut psg = HuC6280Psg::with_revision(revision);
        psg.channels[0].control = 0x9F;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].effective_left_attenuation = 0;
        psg.channels[0].effective_right_attenuation = 0;
        psg.channels[0].wave_index = 4;
        psg.channels[0].wave_counter = 1;
        psg.channels[0].waveform[4] = 3;
        psg.channels[0].waveform[5] = 11;
        psg.resampler = psg.resampler_at_current_level();

        psg.write_port(PsgPort::from_offset(6), 29);
        assert_eq!(psg.channels[0].waveform[4], 29);
        assert_eq!(psg.channels[0].wave_index, 4);
        assert_eq!(psg.channels[0].wave_counter, 1);
        assert_eq!(psg.resampler.last_delta_clock, Some(0));

        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].wave_index, 4);
        assert_eq!(psg.resampler.last_delta_clock, Some(0));
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].wave_index, 5);
        assert_eq!(psg.channels[0].waveform[5], 11);
        assert_eq!(psg.resampler.last_delta_clock, Some(2));
    }
}

#[test]
fn waveform_rate_change_and_generation_resume_emit_only_real_transitions() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
        psg.write_port(PsgPort::from_offset(0), 0);
        psg.write_port(PsgPort::from_offset(1), 0xFF);
        psg.write_port(PsgPort::from_offset(2), 0);
        psg.write_port(PsgPort::from_offset(3), 8);
        psg.write_port(PsgPort::from_offset(5), 0xFF);
        psg.write_port(PsgPort::from_offset(6), 0);
        psg.write_port(PsgPort::from_offset(6), 31);
        for _ in 2..32 {
            psg.write_port(PsgPort::from_offset(6), 0);
        }
        psg.write_port(PsgPort::from_offset(4), 0x9F);
        psg.set_sample_generation_enabled(false);
        psg.advance_master_ticks(u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS) * 6);
        psg.channels[0].wave_index = 0;
        psg.channels[0].wave_counter = effective_period(psg.channels[0].frequency);
        psg.set_sample_generation_enabled(true);

        psg.set_sample_rate(sample_rate);
        let (left, right) = psg.mix_output();
        assert_eq!(psg.resampler.left_level, blip_level(left));
        assert_eq!(psg.resampler.right_level, blip_level(right));
        psg.advance_master_ticks(2_047 * 6);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
        psg.advance_master_ticks(1_500 * 6);
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.001));

        psg.set_sample_generation_enabled(false);
        psg.advance_master_ticks(500 * 6);
        psg.set_sample_generation_enabled(true);
        let (left, right) = psg.mix_output();
        assert_eq!(psg.resampler.left_level, blip_level(left));
        assert_eq!(psg.resampler.right_level, blip_level(right));
        psg.advance_master_ticks(47 * 6);
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
        psg.advance_master_ticks(1_000 * 6);
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    }
}
