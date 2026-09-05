use super::*;

fn encoded_psg_state(psg: &HuC6280Psg) -> Vec<u8> {
    let mut writer = StateWriter::new();
    psg.write_state(&mut writer);
    writer.into_bytes()
}

fn assert_scalar_psg_matches(optimized: &HuC6280Psg, scalar: &HuC6280Psg) {
    assert_eq!(encoded_psg_state(optimized), encoded_psg_state(scalar));
    assert_eq!(
        optimized.debug_capture_phase(),
        scalar.debug_capture_phase()
    );
    assert_eq!(
        optimized.master_debug_samples_ordered(),
        scalar.master_debug_samples_ordered()
    );
    for channel in 0..PSG_CHANNEL_COUNT {
        assert_eq!(
            optimized.channel_debug_samples_ordered(channel),
            scalar.channel_debug_samples_ordered(channel)
        );
    }
}

fn configure_scalar_differential_psg(revision: PsgRevision) -> HuC6280Psg {
    let mut psg = HuC6280Psg::with_revision(revision);
    psg.set_sample_rate(192_000);
    psg.main_amplitude = 0xFF;
    for (index, channel) in psg.channels.iter_mut().enumerate() {
        channel.control = 0x9F;
        channel.balance = 0xFF;
        channel.frequency = 1 + index as u16;
        channel.wave_counter = 1;
        channel.waveform = std::array::from_fn(|sample| ((sample + index * 7) & 0x1F) as u8);
    }
    psg.channels[1].control = 0xDF;
    psg.channels[1].dda_hold = 19;
    psg.channels[4].noise_control = 0x9F;
    psg.channels[4].noise_counter = 1;
    psg.channels[5].control = 0xDF;
    psg.channels[5].dda_hold = 7;
    psg.lfo_frequency = 1;
    psg.lfo_control = 0x82;
    psg.lfo_counter = 1;
    psg.lfo_phase_valid = false;
    seed_effective_gains(&mut psg);
    psg.queue_gain_scan();
    psg.resampler.clocks = BLIP_FRAME_CLOCKS - 4;
    psg.set_debug_capture_enabled(true);
    psg
}

fn configure_source_transition_matrix(revision: PsgRevision) -> HuC6280Psg {
    let mut psg = HuC6280Psg::with_revision(revision);
    psg.set_sample_rate(192_000);
    psg.main_amplitude = 0xFF;
    for (index, channel) in psg.channels.iter_mut().enumerate() {
        channel.control = 0x9F;
        channel.balance = 0xFF;
        channel.frequency = 1;
        channel.wave_counter = 1;
        channel.wave_index = if index == 1 { 5 } else { 0 };
        channel.waveform = std::array::from_fn(|sample| ((sample + index * 3) & 0x1F) as u8);
    }
    psg.channels[2].control = 0xDF;
    psg.channels[2].dda_hold = 23;
    psg.channels[4].noise_control = 0x80;
    psg.channels[4].noise_counter = 1;
    psg.channels[4].noise_seed = 1;
    psg.channels[5].noise_control = 0x80;
    psg.channels[5].noise_counter = 1;
    psg.channels[5].noise_seed = 3;
    psg.channel_mutes[3] = true;
    psg.lfo_frequency = 1;
    psg.lfo_control = 1;
    psg.lfo_counter = 1;
    psg.lfo_phase_valid = false;
    seed_effective_gains(&mut psg);
    psg.gain_scan_active = true;
    psg.gain_scan_clock = 254;
    psg.attenuation_latch = 31;
    psg.set_debug_capture_enabled(true);
    psg
}

#[test]
fn batched_resampler_matches_scalar_timing_across_pcm_state_and_continuation() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut optimized = configure_scalar_differential_psg(revision);
        let mut scalar = configure_scalar_differential_psg(revision);
        for (step, master_ticks) in [
            0_u64, 1, 2, 3, 4, 5, 7, 11, 17, 29, 61, 127, 509, 1_021, 6_143, 12_287,
        ]
        .into_iter()
        .enumerate()
        {
            optimized.advance_master_ticks(master_ticks);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&optimized, &scalar);

            if step == 4 {
                optimized.set_channel_mutes(&[false, true, false, false, false, false]);
                scalar.set_channel_mutes(&[false, true, false, false, false, false]);
            } else if step == 6 {
                optimized.set_channel_mutes(&[]);
                scalar.set_channel_mutes(&[]);
                for psg in [&mut optimized, &mut scalar] {
                    psg.selected_channel = 5;
                    psg.write_port(PsgPort::from_offset(6), 29);
                    psg.selected_channel = 4;
                    psg.write_port(PsgPort::from_offset(7), 0x80);
                }
            } else if step == 8 {
                for psg in [&mut optimized, &mut scalar] {
                    psg.selected_channel = 1;
                    psg.write_port(PsgPort::from_offset(4), 0x9F);
                    psg.write_port(PsgPort::from_offset(9), 0x83);
                }
            }
            assert_scalar_psg_matches(&optimized, &scalar);

            if step % 3 == 2 {
                let mut optimized_pcm = Vec::new();
                let mut scalar_pcm = Vec::new();
                optimized.drain_audio_samples_into(&mut optimized_pcm);
                scalar.drain_audio_samples_into(&mut scalar_pcm);
                assert_eq!(optimized_pcm, scalar_pcm);
                assert_scalar_psg_matches(&optimized, &scalar);
            }
        }

        optimized.advance_master_ticks(65_537 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR + 5);
        scalar.advance_master_ticks_scalar(65_537 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR + 5);
        assert_scalar_psg_matches(&optimized, &scalar);
        let mut optimized_pcm = Vec::new();
        let mut scalar_pcm = Vec::new();
        optimized.drain_audio_samples_into(&mut optimized_pcm);
        scalar.drain_audio_samples_into(&mut scalar_pcm);
        assert_eq!(optimized_pcm, scalar_pcm);
        assert_scalar_psg_matches(&optimized, &scalar);
    }
}

#[test]
fn batched_resampler_matches_scalar_across_disabled_generation_continuation() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut optimized = configure_scalar_differential_psg(revision);
        let mut scalar = configure_scalar_differential_psg(revision);
        optimized.set_sample_generation_enabled(false);
        scalar.set_sample_generation_enabled(false);
        for master_ticks in [0_u64, 1, 2, 3, 4, 5, 7, 1_021, 6_143] {
            optimized.advance_master_ticks(master_ticks);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&optimized, &scalar);
        }

        optimized.set_sample_generation_enabled(true);
        scalar.set_sample_generation_enabled(true);
        for master_ticks in [5_u64, 6, 17, 1_023, 12_289] {
            optimized.advance_master_ticks(master_ticks);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&optimized, &scalar);
            let mut optimized_pcm = Vec::new();
            let mut scalar_pcm = Vec::new();
            optimized.drain_audio_samples_into(&mut optimized_pcm);
            scalar.drain_audio_samples_into(&mut scalar_pcm);
            assert_eq!(optimized_pcm, scalar_pcm);
            assert_scalar_psg_matches(&optimized, &scalar);
        }
    }
}

#[test]
fn direct_source_detection_matches_scalar_for_lfo_noise_dda_mute_and_gain() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut optimized = configure_source_transition_matrix(revision);
        let mut scalar = configure_source_transition_matrix(revision);
        for master_ticks in [1_u64, 2, 3, 6, 12, 31, 257, 1_019, 4_099] {
            optimized.advance_master_ticks(master_ticks);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&optimized, &scalar);

            let mut optimized_pcm = Vec::new();
            let mut scalar_pcm = Vec::new();
            optimized.drain_audio_samples_into(&mut optimized_pcm);
            scalar.drain_audio_samples_into(&mut scalar_pcm);
            assert_eq!(optimized_pcm, scalar_pcm);
            assert_scalar_psg_matches(&optimized, &scalar);
        }
    }
}

#[test]
fn batched_resampler_skips_unchanged_adjacent_waveform_levels() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut optimized = HuC6280Psg::with_revision(revision);
        optimized.set_sample_rate(192_000);
        optimized.main_amplitude = 0xFF;
        optimized.channels[0].control = 0x9F;
        optimized.channels[0].balance = 0xFF;
        optimized.channels[0].frequency = 1;
        optimized.channels[0].wave_counter = 1;
        optimized.channels[0].waveform = [17; 32];
        seed_effective_gains(&mut optimized);
        let mut scalar = HuC6280Psg::with_revision(revision);
        scalar.set_sample_rate(192_000);
        scalar.main_amplitude = 0xFF;
        scalar.channels[0].control = 0x9F;
        scalar.channels[0].balance = 0xFF;
        scalar.channels[0].frequency = 1;
        scalar.channels[0].wave_counter = 1;
        scalar.channels[0].waveform = [17; 32];
        seed_effective_gains(&mut scalar);

        for master_ticks in [6_u64, 6, 12, 30, 1_023] {
            let previous_index = optimized.channels[0].wave_index;
            optimized.advance_master_ticks(master_ticks);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&optimized, &scalar);
            assert_ne!(optimized.channels[0].wave_index, previous_index);
        }
    }
}

#[cfg(feature = "profiling")]
#[test]
fn profiled_batched_resampler_matches_scalar_timing() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut profiled = configure_scalar_differential_psg(revision);
        let mut scalar = configure_scalar_differential_psg(revision);
        let mut profiling = crate::hardware::profiling::PceProfiling::default();
        for master_ticks in [0_u64, 1, 2, 3, 4, 5, 7, 17, 509, 1_021, 6_143, 65_537] {
            profiled.advance_master_ticks_profiled(master_ticks, &mut profiling);
            scalar.advance_master_ticks_scalar(master_ticks);
            assert_scalar_psg_matches(&profiled, &scalar);
        }
        assert_eq!(profiling.snapshot.psg_advance_calls, 12);
        assert!(
            profiling.snapshot.psg_mixer_source_transitions
                <= profiling.snapshot.psg_mixer_source_examinations
        );
        assert!(
            profiling.snapshot.psg_mixer_source_examinations
                <= profiling.snapshot.psg_oscillator_clocks * PSG_CHANNEL_COUNT as u64
        );
    }
}

#[cfg(feature = "profiling")]
#[test]
fn profiled_source_detection_counts_only_relevant_final_sources() {
    let mut profiled = configure_source_transition_matrix(PsgRevision::HuC6280);
    let mut scalar = configure_source_transition_matrix(PsgRevision::HuC6280);
    let mut profiling = crate::hardware::profiling::PceProfiling::default();

    profiled.advance_master_ticks_profiled(6, &mut profiling);
    scalar.advance_master_ticks_scalar(6);

    assert_scalar_psg_matches(&profiled, &scalar);
    assert_eq!(profiling.snapshot.psg_internal_clocks, 2);
    assert_eq!(profiling.snapshot.psg_oscillator_clocks, 1);
    assert_eq!(profiling.snapshot.psg_mixer_source_examinations, 4);
    assert_eq!(profiling.snapshot.psg_mixer_source_transitions, 3);
    assert_eq!(profiling.snapshot.psg_mix_scans, 1);
}

#[cfg(feature = "profiling")]
#[test]
fn zero_gain_unmuted_source_remains_transition_relevant() {
    let configured = || {
        let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280);
        psg.set_sample_rate(192_000);
        psg.channels[0].control = 0x9F;
        psg.channels[0].frequency = 1;
        psg.channels[0].wave_counter = 1;
        psg.channels[0].waveform[0] = 3;
        psg.channels[0].waveform[1] = 27;
        psg.channels[0].effective_left_attenuation = 30;
        psg.channels[0].effective_right_attenuation = 31;
        psg.resampler = psg.resampler_at_current_level();
        psg
    };
    let mut profiled = configured();
    let mut scalar = configured();
    let mut profiling = crate::hardware::profiling::PceProfiling::default();

    profiled.advance_master_ticks_profiled(6, &mut profiling);
    scalar.advance_master_ticks_scalar(6);

    assert_scalar_psg_matches(&profiled, &scalar);
    assert_eq!(profiling.snapshot.psg_mixer_source_examinations, 1);
    assert_eq!(profiling.snapshot.psg_mixer_source_transitions, 1);
    assert_eq!(profiling.snapshot.psg_mix_scans, 1);
}
