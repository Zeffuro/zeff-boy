use super::cpu::Cpu;
use super::{
    BaseBus, DETERMINISTIC_PSG_RESET_CLEARS_WAVE_RAM, DETERMINISTIC_PSG_RESET_VALUE, HuC6280Psg,
    MAX_PSG_SAMPLE_RATE, PSG_CHANNEL_COUNT, PSG_UNAVAILABLE_READ_VALUE, PSG_WAVEFORM_WORDS,
    PsgPort, PsgRevision,
};

fn port(offset: u8) -> PsgPort {
    PsgPort::from_offset(offset)
}

fn select(psg: &mut HuC6280Psg, channel: u8) {
    psg.write_port(port(0), channel);
}

#[test]
fn every_psg_read_is_unavailable_and_ports_above_nine_ignore_writes() {
    let mut psg = HuC6280Psg::new();
    for offset in 0..16 {
        assert_eq!(psg.read_port(port(offset)), PSG_UNAVAILABLE_READ_VALUE);
    }

    let channels = psg.channels().clone();
    let selected = psg.selected_channel_id();
    let main_amplitude = psg.main_amplitude();
    let lfo_frequency = psg.lfo_frequency();
    let lfo_control = psg.lfo_control();
    for offset in 10..16 {
        psg.write_port(port(offset), 0xFF);
    }
    assert_eq!(psg.channels(), &channels);
    assert_eq!(psg.selected_channel_id(), selected);
    assert_eq!(psg.main_amplitude(), main_amplitude);
    assert_eq!(psg.lfo_frequency(), lfo_frequency);
    assert_eq!(psg.lfo_control(), lfo_control);
}

#[test]
fn global_registers_ignore_invalid_channel_selection() {
    let mut psg = HuC6280Psg::new();
    select(&mut psg, 0xFF);
    assert_eq!(psg.selected_channel_id(), 7);
    assert_eq!(psg.selected_channel(), None);
    let channels = psg.channels().clone();
    for offset in 2..=7 {
        psg.write_port(port(offset), 0xFF);
    }
    assert_eq!(psg.channels(), &channels);

    psg.write_port(port(1), 0xA5);
    psg.write_port(port(8), 0x5A);
    psg.write_port(port(9), 0xFF);
    assert_eq!(psg.main_amplitude(), 0xA5);
    assert_eq!(psg.lfo_frequency(), 0x5A);
    assert_eq!(psg.lfo_control(), 0x83);
}

#[test]
fn per_channel_registers_are_independent_and_apply_hardware_masks() {
    let mut psg = HuC6280Psg::new();
    for channel in 0..PSG_CHANNEL_COUNT as u8 {
        select(&mut psg, channel);
        psg.write_port(port(2), 0x40 | channel);
        psg.write_port(port(3), 0xFF);
        psg.write_port(port(4), 0xFF);
        psg.write_port(port(5), 0x80 | channel);
        psg.write_port(port(7), 0xFF);
    }

    for (channel, state) in psg.channels().iter().enumerate() {
        assert_eq!(state.frequency(), 0x0F40 | channel as u16);
        assert_eq!(state.control(), 0xDF);
        assert!(state.key_on());
        assert!(state.dda_enabled());
        assert_eq!(state.amplitude(), 0x1F);
        assert_eq!(state.balance(), 0x80 | channel as u8);
        assert_eq!(state.noise_control(), if channel >= 4 { 0x9F } else { 0 });
    }
}

#[test]
fn waveform_and_dda_writes_follow_the_selected_channel_mode() {
    let mut psg = HuC6280Psg::new();
    for value in 0..PSG_WAVEFORM_WORDS as u8 {
        psg.write_port(port(6), 0xE0 | value);
    }
    assert_eq!(psg.channels()[0].wave_index(), 0);
    assert_eq!(
        psg.channels()[0].waveform(),
        &std::array::from_fn(|index| index as u8)
    );

    psg.write_port(port(6), 0x3F);
    assert_eq!(psg.channels()[0].waveform()[0], 0x1F);
    assert_eq!(psg.channels()[0].wave_index(), 1);
    psg.write_port(port(4), 0x40);
    assert_eq!(psg.channels()[0].wave_index(), 0);
    psg.write_port(port(6), 0x35);
    assert_eq!(psg.channels()[0].dda_hold(), 0x15);
    assert_eq!(psg.channels()[0].waveform()[0], 0x1F);

    psg.write_port(port(4), 0xC0);
    psg.write_port(port(6), 0x2A);
    assert_eq!(psg.channels()[0].dda_hold(), 0x0A);
    assert_eq!(psg.channels()[0].wave_index(), 0);
    psg.write_port(port(4), 0x80);
    psg.write_port(port(6), 0x00);
    assert_eq!(psg.channels()[0].waveform()[0], 0);
    assert_eq!(psg.channels()[0].wave_index(), 0);
    psg.write_port(port(4), 0x00);
    psg.write_port(port(6), 0x03);
    assert_eq!(psg.channels()[0].waveform()[0], 3);
    assert_eq!(psg.channels()[0].wave_index(), 1);
}

#[test]
fn lfo_trigger_resets_channel_two_index_and_active_state_is_derived() {
    let mut psg = HuC6280Psg::new();
    select(&mut psg, 1);
    for value in 1..=3 {
        psg.write_port(port(6), value);
    }
    assert_eq!(psg.channels()[1].wave_index(), 3);
    psg.write_port(port(4), 0x80);
    select(&mut psg, 0);
    psg.write_port(port(4), 0x80);

    psg.write_port(port(9), 2);
    assert!(psg.lfo_active());
    psg.write_port(port(9), 0x82);
    assert!(psg.lfo_halted());
    assert!(!psg.lfo_active());
    assert_eq!(psg.channels()[1].wave_index(), 0);
    psg.write_port(port(9), 2);
    assert!(psg.lfo_active());
    psg.write_port(port(9), 0);
    assert!(!psg.lfo_active());
    select(&mut psg, 1);
    psg.write_port(port(4), 0);
    psg.write_port(port(9), 1);
    assert!(!psg.lfo_active());
}

#[test]
fn reset_uses_the_named_zero_state_and_waveform_policy() {
    let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
    select(&mut psg, 5);
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(2), 0xFF);
    psg.write_port(port(4), 0);
    psg.write_port(port(6), 0x1F);
    psg.write_port(port(7), 0xFF);
    psg.write_port(port(8), 0xFF);
    psg.write_port(port(9), 3);

    psg.reset();

    assert_eq!(psg.revision(), PsgRevision::HuC6280A);
    assert_eq!(psg.selected_channel_id(), DETERMINISTIC_PSG_RESET_VALUE);
    assert_eq!(psg.main_amplitude(), DETERMINISTIC_PSG_RESET_VALUE);
    assert_eq!(psg.lfo_frequency(), DETERMINISTIC_PSG_RESET_VALUE);
    assert_eq!(psg.lfo_control(), DETERMINISTIC_PSG_RESET_VALUE);
    assert_eq!(
        psg.channels()
            .iter()
            .all(|channel| channel == &Default::default()),
        DETERMINISTIC_PSG_RESET_CLEARS_WAVE_RAM
    );
}

#[test]
fn original_and_a_revisions_apply_dc_removal_after_their_distinct_dac_levels() {
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut original = HuC6280Psg::with_revision(PsgRevision::HuC6280);
        original.set_sample_rate(sample_rate);
        original.write_port(port(1), 0xFF);
        original.write_port(port(5), 0xFF);
        original.write_port(port(4), 0xDF);
        original.write_port(port(6), 0);
        original.advance_master_ticks(3_000_000);
        let mut original_samples = Vec::new();
        original.drain_audio_samples_into(&mut original_samples);
        assert!(original_samples.iter().all(|sample| *sample == 0.0));

        let mut revised = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
        revised.set_sample_rate(sample_rate);
        revised.write_port(port(1), 0xFF);
        revised.write_port(port(5), 0xFF);
        revised.write_port(port(4), 0xDF);
        revised.write_port(port(6), 0);
        revised.advance_master_ticks(3_000_000);
        let mut revised_samples = Vec::new();
        revised.drain_audio_samples_into(&mut revised_samples);
        assert!(revised_samples.iter().any(|sample| *sample < -0.01));
        assert!(
            revised_samples
                .iter()
                .rev()
                .take(64)
                .all(|sample| sample.abs() < 0.001)
        );
    }
}

#[test]
fn shared_ac_removal_preserves_dac_transition_strength_across_host_rates() {
    let mut peaks = Vec::new();
    for sample_rate in [44_100, 48_000, 96_000, 192_000] {
        let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280);
        psg.set_sample_rate(sample_rate);
        psg.write_port(port(1), 0xFF);
        psg.write_port(port(5), 0xFF);
        psg.write_port(port(4), 0xDF);
        psg.write_port(port(6), 31);
        psg.advance_master_ticks(3_000_000);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);

        psg.write_port(port(6), 0);
        psg.advance_master_ticks(3_000_000);
        psg.drain_audio_samples_into(&mut samples);
        peaks.push(samples.iter().copied().map(f32::abs).fold(0.0, f32::max));
        assert!(samples.iter().any(|sample| *sample < -0.05));
        assert!(
            samples
                .iter()
                .rev()
                .take(64)
                .all(|sample| sample.abs() < 0.001)
        );
    }

    let minimum = peaks.iter().copied().fold(f32::INFINITY, f32::min);
    let maximum = peaks.iter().copied().fold(0.0, f32::max);
    assert!(maximum / minimum < 1.05);
}

#[test]
fn base_bus_routes_sampled_sixteen_byte_psg_mirrors() {
    let mut bus = BaseBus::new(Vec::new(), HuC6280Psg::new()).unwrap();
    bus.write(0x1F_EBF0, 4);
    bus.write(0x1F_EA32, 0x34);
    bus.write(0x1F_E943, 0xFF);
    bus.write(0x1F_EA74, 0x9F);
    bus.write(0x1F_EB85, 0xA5);
    bus.write(0x1F_E9B7, 0xFF);

    let channel = &bus.devices().channels()[4];
    assert_eq!(channel.frequency(), 0x0F34);
    assert_eq!(channel.control(), 0x9F);
    assert_eq!(channel.balance(), 0xA5);
    assert_eq!(channel.noise_control(), 0x9F);
    assert_eq!(bus.read(0x1F_E8F6), PSG_UNAVAILABLE_READ_VALUE);
}

#[test]
fn raw_cpu_writes_psg_registers_through_an_ff_mapping_register() {
    let rom = vec![
        0xA9, 0x04, 0x8D, 0x00, 0x28, 0xA9, 0x34, 0x8D, 0x02, 0x28, 0xA9, 0x0F, 0x8D, 0x03, 0x28,
    ];
    let mut bus = BaseBus::new(rom, HuC6280Psg::new()).unwrap();
    let mut cpu = Cpu::new();
    cpu.set_mapping_register(1, 0xFF);

    for _ in 0..6 {
        cpu.step(&mut bus).unwrap();
    }

    assert_eq!(bus.devices().selected_channel_id(), 4);
    assert_eq!(bus.devices().channels()[4].frequency(), 0x0F34);
}

#[test]
fn waveform_audio_is_scheduled_from_master_ticks() {
    let mut psg = HuC6280Psg::new();
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    for value in 0..PSG_WAVEFORM_WORDS as u8 {
        psg.write_port(port(6), value);
    }
    psg.write_port(port(2), 64);
    psg.write_port(port(4), 0x9F);
    psg.advance_master_ticks(1_365 * 262);

    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    assert_eq!(samples.len(), 734 * 2);
    assert!(samples.iter().any(|sample| sample.abs() > 0.001));
}

#[test]
fn dda_audio_holds_the_written_five_bit_value() {
    let mut psg = HuC6280Psg::new();
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    psg.write_port(port(4), 0xDF);
    psg.write_port(port(6), 0x1F);
    psg.advance_master_ticks(24_576);

    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    psg.advance_master_ticks(10_000);
    psg.drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
    assert!(samples.iter().any(|sample| *sample > 0.01));
    assert!(samples.chunks_exact(2).any(|pair| pair[0] > 0.01));
    assert!(samples.chunks_exact(2).any(|pair| pair[1] > 0.01));
}

#[test]
fn noise_audio_uses_only_the_two_noise_channels() {
    let mut psg = HuC6280Psg::new();
    psg.write_port(port(1), 0xFF);
    psg.write_port(port(5), 0xFF);
    psg.write_port(port(0), 4);
    psg.write_port(port(5), 0xFF);
    psg.write_port(port(4), 0x9F);
    psg.write_port(port(7), 0x9F);
    psg.advance_master_ticks(500_000);

    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    assert!(samples.windows(2).any(|pair| pair[0] != pair[1]));
}

fn configure_wave_channel(psg: &mut HuC6280Psg, channel: u8, frequency: u16, sample: u8) {
    select(psg, channel);
    psg.write_port(port(2), frequency as u8);
    psg.write_port(port(3), (frequency >> 8) as u8);
    psg.write_port(port(5), 0xFF);
    for _ in 0..PSG_WAVEFORM_WORDS {
        psg.write_port(port(6), sample);
    }
    psg.write_port(port(4), 0x9F);
}

#[test]
fn zero_frequency_uses_the_4096_period() {
    let mut psg = HuC6280Psg::new();
    configure_wave_channel(&mut psg, 0, 0, 31);

    psg.advance_master_ticks(4096 * 6 - 6);
    assert_eq!(psg.channels()[0].wave_index(), 0);
    psg.advance_master_ticks(6);
    assert_eq!(psg.channels()[0].wave_index(), 1);
}

#[test]
fn attenuation_slot_thirty_one_is_silent_globally_and_per_channel() {
    let mut global_muted = HuC6280Psg::new();
    global_muted.write_port(port(5), 0xFF);
    configure_wave_channel(&mut global_muted, 0, 1, 31);
    global_muted.advance_master_ticks(10_000);
    let mut samples = Vec::new();
    global_muted.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().all(|sample| *sample == 0.0));

    let mut channel_muted = HuC6280Psg::new();
    channel_muted.write_port(port(1), 0xFF);
    configure_wave_channel(&mut channel_muted, 0, 1, 31);
    select(&mut channel_muted, 0);
    channel_muted.write_port(port(4), 0x80);
    channel_muted.advance_master_ticks(10_000);
    channel_muted.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().all(|sample| *sample == 0.0));
}

fn configure_lfo_pair(psg: &mut HuC6280Psg, depth: u8) {
    psg.write_port(port(1), 0xFF);
    configure_wave_channel(psg, 0, 4, 16);
    configure_wave_channel(psg, 1, 1, 17);
    psg.write_port(port(8), 0);
    psg.write_port(port(9), depth);
}

#[test]
fn lfo_depth_changes_target_index_and_source_remains_audible() {
    let mut no_lfo = HuC6280Psg::new();
    no_lfo.write_port(port(1), 0xFF);
    configure_wave_channel(&mut no_lfo, 0, 4, 16);
    configure_wave_channel(&mut no_lfo, 1, 1, 17);
    no_lfo.advance_master_ticks(20 * 6);
    assert_eq!(no_lfo.channels()[0].wave_index(), 5);

    let mut depth_one = HuC6280Psg::new();
    configure_lfo_pair(&mut depth_one, 1);
    depth_one.advance_master_ticks(20 * 6);
    assert_eq!(depth_one.channels()[0].wave_index(), 4);
    assert_eq!(depth_one.channels()[1].wave_index(), 20);

    let mut depth_two = HuC6280Psg::new();
    configure_lfo_pair(&mut depth_two, 2);
    depth_two.advance_master_ticks(20 * 6);
    assert_eq!(depth_two.channels()[0].wave_index(), 3);

    let mut depth_three = HuC6280Psg::new();
    configure_lfo_pair(&mut depth_three, 3);
    depth_three.advance_master_ticks(20 * 6);
    assert_eq!(depth_three.channels()[0].wave_index(), 1);

    depth_one.advance_master_ticks(100_000);
    let mut samples = Vec::new();
    depth_one.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().any(|sample| *sample != 0.0));
}

#[test]
fn lfo_halt_holds_source_index_until_retrigger() {
    let mut psg = HuC6280Psg::new();
    configure_lfo_pair(&mut psg, 1);
    psg.write_port(port(9), 0x81);
    psg.advance_master_ticks(120);
    assert_eq!(psg.channels()[1].wave_index(), 0);
    psg.write_port(port(9), 1);
    psg.advance_master_ticks(6);
    assert_eq!(psg.channels()[1].wave_index(), 1);
}

#[test]
fn lfo_frequency_scales_source_index_cadence() {
    let mut psg = HuC6280Psg::new();
    configure_lfo_pair(&mut psg, 1);
    psg.write_port(port(8), 2);
    psg.advance_master_ticks(6);
    assert_eq!(psg.channels()[1].wave_index(), 0);
    psg.advance_master_ticks(6);
    assert_eq!(psg.channels()[1].wave_index(), 1);
}

fn configure_square_wave(psg: &mut HuC6280Psg, frequency: u16) {
    psg.write_port(port(1), 0xFF);
    select(psg, 0);
    psg.write_port(port(2), frequency as u8);
    psg.write_port(port(3), (frequency >> 8) as u8);
    psg.write_port(port(5), 0xFF);
    for _ in 0..16 {
        psg.write_port(port(6), 0);
    }
    for _ in 0..16 {
        psg.write_port(port(6), 31);
    }
    psg.write_port(port(4), 0x9F);
}

fn capture_square_wave(sample_rate: u32, frequency: u16, master_ticks: u64) -> Vec<f32> {
    let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
    psg.set_sample_rate(sample_rate);
    configure_square_wave(&mut psg, frequency);
    psg.advance_master_ticks(master_ticks);
    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    samples
}

fn mono_samples(samples: &[f32], skipped_frames: usize) -> impl Iterator<Item = f64> + '_ {
    samples
        .chunks_exact(2)
        .skip(skipped_frames)
        .map(|pair| f64::from(pair[0]))
}

fn rms(samples: &[f32], skipped_frames: usize) -> f64 {
    let samples = mono_samples(samples, skipped_frames).collect::<Vec<_>>();
    (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64).sqrt()
}

fn rising_zero_crossing_frequency(samples: &[f32], skipped_frames: usize, sample_rate: u32) -> f64 {
    let samples = mono_samples(samples, skipped_frames).collect::<Vec<_>>();
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    crossings as f64 * f64::from(sample_rate) / (samples.len() - 1) as f64
}

fn tone_amplitude(samples: &[f32], skipped_frames: usize, sample_rate: u32, hz: f64) -> f64 {
    let samples = mono_samples(samples, skipped_frames).collect::<Vec<_>>();
    let last = (samples.len() - 1) as f64;
    let (sin, cos, weight) =
        samples
            .iter()
            .enumerate()
            .fold((0.0, 0.0, 0.0), |(sin, cos, weight), (index, sample)| {
                let phase = std::f64::consts::TAU * hz * index as f64 / f64::from(sample_rate);
                let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / last).cos();
                (
                    sin + sample * phase.sin() * window,
                    cos + sample * phase.cos() * window,
                    weight + window,
                )
            });
    2.0 * sin.hypot(cos) / weight
}

#[test]
fn band_limited_waveform_has_consistent_pitch_spectrum_and_amplitude() {
    const MASTER_TICKS: u64 = 5_369_318;
    const FREQUENCY: u16 = 64;
    const EXPECTED_HZ: f64 = 315_000_000.0 / 88.0 / (FREQUENCY as f64 * 32.0);
    let mut amplitudes = Vec::new();

    for sample_rate in [44_100, 48_000, 96_000] {
        let samples = capture_square_wave(sample_rate, FREQUENCY, MASTER_TICKS);
        let frames = samples.len() / 2;
        let expected_frames = (MASTER_TICKS as f64 * 88.0 * f64::from(sample_rate)
            / (6.0 * 315_000_000.0))
            .floor() as usize;
        assert!(frames.abs_diff(expected_frames) <= 1, "rate={sample_rate}");

        let skipped = sample_rate as usize / 50;
        let measured = rising_zero_crossing_frequency(&samples, skipped, sample_rate);
        assert!(
            (measured / EXPECTED_HZ - 1.0).abs() < 0.01,
            "rate={sample_rate}"
        );

        let fundamental = tone_amplitude(&samples, skipped, sample_rate, EXPECTED_HZ);
        let off_frequency = tone_amplitude(&samples, skipped, sample_rate, EXPECTED_HZ * 1.4);
        assert!(fundamental > off_frequency * 20.0, "rate={sample_rate}");
        amplitudes.push(rms(&samples, skipped));
    }

    let minimum = amplitudes.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = amplitudes.iter().copied().fold(0.0, f64::max);
    assert!(maximum / minimum < 1.05);
}

#[test]
fn band_limited_waveform_rejects_ultrasonic_aliases() {
    for sample_rate in [44_100, 48_000, 96_000] {
        let samples = capture_square_wave(sample_rate, 1, 2_000_000);
        assert!(rms(&samples, sample_rate as usize / 100) < 0.005);
    }
}

#[test]
fn irregular_audio_drains_preserve_long_run_continuity() {
    const MASTER_TICKS: u64 = 6_000_000;
    for sample_rate in [44_100, 48_000, 96_000] {
        let whole = capture_square_wave(sample_rate, 64, MASTER_TICKS);

        let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
        psg.set_sample_rate(sample_rate);
        configure_square_wave(&mut psg, 64);
        let mut chunked = Vec::new();
        let mut elapsed = 0;
        for requested in [1, 7, 1_365, 65_537, 777_777, 2_000_003] {
            let ticks = requested.min(MASTER_TICKS - elapsed);
            psg.advance_master_ticks(ticks);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);
            chunked.extend(samples);
            elapsed += ticks;
        }
        psg.advance_master_ticks(MASTER_TICKS - elapsed);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        chunked.extend(samples);

        assert_eq!(chunked, whole, "rate={sample_rate}");
    }
}

#[test]
fn reset_preserves_the_configured_sample_rate() {
    let mut psg = HuC6280Psg::new();
    psg.set_sample_rate(96_000);
    psg.reset();
    configure_square_wave(&mut psg, 64);
    psg.advance_master_ticks(1_000_000);
    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    assert_eq!(samples.len() / 2, 4_469);
}

#[test]
fn sample_rate_clamps_to_the_named_capacity_limit() {
    let minimum = capture_square_wave(1, 64, 2_000_000);
    let maximum = capture_square_wave(MAX_PSG_SAMPLE_RATE, 64, 2_000_000);
    let clamped = capture_square_wave(u32::MAX, 64, 2_000_000);
    assert!(minimum.len() <= 2);
    assert_eq!(clamped, maximum);
}

#[test]
fn audio_controls_preserve_oscillator_and_reset_policies() {
    let mut psg = HuC6280Psg::new();
    configure_square_wave(&mut psg, 64);
    psg.advance_master_ticks(64 * 6 - 1);
    assert_eq!(psg.channels()[0].wave_index(), 0);
    psg.set_sample_rate(48_000);
    psg.advance_master_ticks(1);
    assert_eq!(psg.channels()[0].wave_index(), 1);

    psg.set_channel_mutes(&[true]);
    let mut samples = Vec::new();
    psg.set_sample_generation_enabled(false);
    let index = psg.channels()[0].wave_index();
    psg.advance_master_ticks(64 * 6);
    assert_eq!(psg.channels()[0].wave_index(), index.wrapping_add(1) & 0x1F);
    psg.drain_audio_samples_into(&mut samples);
    assert!(samples.is_empty());

    psg.reset();
    configure_square_wave(&mut psg, 64);
    psg.set_sample_generation_enabled(true);
    psg.advance_master_ticks(100_000);
    psg.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().all(|sample| *sample == 0.0));

    psg.set_channel_mutes(&[]);
    psg.advance_master_ticks(100_000);
    psg.drain_audio_samples_into(&mut samples);
    assert!(samples.iter().any(|sample| *sample != 0.0));
}
