use super::Apu;
use crate::hardware::types::constants::*;
use crate::save_state::{StateReader, StateWriter};
use std::f64::consts::PI;

fn pulse(rate: u32, frequency: u16) -> Apu {
    let mut apu = Apu::new();
    apu.set_sample_rate(rate);
    apu.write(NR52, 0x80);
    apu.write(NR50, 0x77);
    apu.write(NR51, 0x11);
    apu.write(NR11, 0);
    apu.write(NR12, 0x80);
    apu.write(NR13, frequency as u8);
    apu.write(NR14, 0x80 | (frequency >> 8) as u8);
    apu
}

fn run(apu: &mut Apu, clocks: u64, chunks: &[u64], drain: bool) -> Vec<f32> {
    let mut remaining = clocks;
    let mut output = Vec::new();
    for chunk in chunks.iter().cycle() {
        if remaining == 0 {
            break;
        }
        let step = remaining.min(*chunk);
        apu.step(step);
        remaining -= step;
        if drain {
            output.extend(apu.drain_samples());
        }
    }
    output.extend(apu.drain_samples());
    output
}

fn amplitude(samples: &[f32], rate: u32, frequency: f64) -> f64 {
    let mut real = 0.0;
    let mut imaginary = 0.0;
    let mut total_weight = 0.0;
    for (index, &sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * PI * index as f64 / samples.len() as f64).cos();
        let angle = 2.0 * PI * frequency * index as f64 / f64::from(rate);
        real += f64::from(sample) * window * angle.cos();
        imaginary += f64::from(sample) * window * angle.sin();
        total_weight += window;
    }
    2.0 * real.hypot(imaginary) / total_weight
}

fn hardware_state(apu: &Apu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    apu.write_state(&mut writer);
    writer.into_bytes()
}

#[test]
fn high_pulse_tones_preserve_the_fundamental_without_folded_harmonics() {
    for rate in [44_100, 48_000, 96_000] {
        for frequency in [2025, 2037] {
            let mut apu = pulse(rate, frequency);
            let output = run(&mut apu, 1_048_576, &[4], false);
            let mono: Vec<_> = output.chunks_exact(2).map(|pair| pair[0]).collect();
            let settled = &mono[rate as usize / 20..];
            let fundamental = 131_072.0 / f64::from(2048 - frequency);
            let measured = amplitude(settled, rate, fundamental);
            let expected = (8.0 / 15.0) * (PI / 8.0).sin() / PI;
            assert!(
                (0.65..1.03).contains(&(measured / expected)),
                "rate={rate} frequency={frequency} amplitude={measured} expected={expected}"
            );
            for harmonic in [3, 5, 6, 7, 9] {
                let frequency = fundamental * f64::from(harmonic);
                if frequency < f64::from(rate) * 0.6 {
                    continue;
                }
                let folded = (frequency + f64::from(rate) / 2.0).rem_euclid(f64::from(rate))
                    - f64::from(rate) / 2.0;
                let alias = amplitude(settled, rate, folded.abs());
                assert!(
                    alias / measured < 0.008,
                    "rate={rate} harmonic={harmonic} alias={alias} fundamental={measured}"
                );
            }
        }
    }
}

#[test]
fn low_pulse_tone_gain_matches_its_analytic_fundamental() {
    for rate in [44_100, 48_000, 96_000] {
        let mut apu = pulse(rate, 1792);
        let output = run(&mut apu, 1_048_576, &[4], false);
        let mono: Vec<_> = output.chunks_exact(2).map(|pair| pair[0]).collect();
        let measured = amplitude(&mono[rate as usize / 20..], rate, 512.0);
        let expected = (8.0 / 15.0) * (PI / 8.0).sin() / PI;
        assert!((measured / expected - 1.0).abs() < 0.025);
    }
}

#[test]
fn ultrasonic_pulse_and_wave_do_not_fold_into_audible_output() {
    for wave in [false, true] {
        let mut apu = pulse(48_000, 2047);
        if wave {
            apu.write(NR51, 0x44);
            for address in WAVE_RAM_START..=WAVE_RAM_END {
                apu.write(address, 0xF0);
            }
            apu.write(NR30, 0x80);
            apu.write(NR32, 0x20);
            apu.write(NR33, 0xFF);
            apu.write(NR34, 0x87);
        }
        let samples = run(&mut apu, 1_048_576, &[4], false);
        let settled = &samples[9_600..];
        let power =
            settled.iter().map(|sample| sample * sample).sum::<f32>() / settled.len() as f32;
        assert!(power.sqrt() < 0.0005, "wave={wave} rms={}", power.sqrt());
    }
}

fn all_channels(cgb: bool, double_speed: bool, render: bool) -> Apu {
    let mut apu = pulse(48_000, 2025);
    apu.set_cgb_hardware(cgb);
    apu.set_cgb_double_speed(double_speed);
    apu.write(NR52, 0);
    apu.write(NR52, 0x80);
    apu.write(NR50, 0x73);
    apu.write(NR51, 0xFF);
    for (address, value) in [
        (NR11, 0x80),
        (NR12, 0xA3),
        (NR13, 0xFC),
        (NR14, 0x87),
        (NR21, 0x40),
        (NR22, 0x72),
        (NR23, 0xFA),
        (NR24, 0x87),
        (NR30, 0x80),
        (NR32, 0x20),
        (NR33, 0xFF),
        (NR34, 0x87),
        (NR42, 0xB3),
        (NR43, 0x08),
        (NR44, 0x80),
    ] {
        apu.write(address, value);
    }
    for offset in 0..16 {
        apu.wave_ram[offset] = (offset as u8).wrapping_mul(0x31);
    }
    apu.sample_generation_enabled = render;
    apu
}

#[test]
fn rendering_preserves_hardware_state_and_pcm_for_every_clock_phase() {
    for (cgb, double_speed) in [(false, false), (true, false), (true, true)] {
        let mut rendered = all_channels(cgb, double_speed, true);
        let mut silent = all_channels(cgb, double_speed, false);
        for index in 0..1024 {
            if index % 17 == 0 {
                rendered.clock_div_apu();
                silent.clock_div_apu();
            }
            if index % 61 == 0 {
                for apu in [&mut rendered, &mut silent] {
                    apu.write(NR43, (index & 0x7F) as u8);
                    apu.write(NR32, ((index & 3) << 5) as u8);
                    apu.write(NR14, 0x87);
                    apu.write(NR34, 0x87);
                    apu.write(NR10, 0x13);
                }
            }
            let clocks = [1, 2, 4, 3, 17, 511, 7, 65_538][index % 8];
            rendered.step(clocks);
            silent.step(clocks);
            assert_eq!(rendered.pcm12(), silent.pcm12());
            assert_eq!(rendered.pcm34(), silent.pcm34());
            assert_eq!(hardware_state(&rendered), hardware_state(&silent));
            rendered.drain_samples();
        }
    }
}

#[test]
fn output_is_identical_across_clock_chunks_and_drain_boundaries() {
    for (cgb, double_speed) in [(false, false), (true, false), (true, true)] {
        let expected = run(
            &mut all_channels(cgb, double_speed, true),
            100_003,
            &[1],
            false,
        );
        for (chunks, drain) in [(&[4][..], true), (&[65_538, 3, 7][..], false)] {
            let actual = run(
                &mut all_channels(cgb, double_speed, true),
                100_003,
                chunks,
                drain,
            );
            assert_eq!(actual, expected, "cgb={cgb} double_speed={double_speed}");
        }
    }
}

#[test]
fn mute_and_routing_changes_have_the_same_output_transition() {
    let mut muted = pulse(48_000, 2025);
    let mut routed = pulse(48_000, 2025);
    assert_eq!(
        run(&mut muted, 1003, &[4], false),
        run(&mut routed, 1003, &[4], false)
    );
    muted.set_channel_mutes([true; 4]);
    routed.write(NR51, 0);
    assert_eq!(
        run(&mut muted, 10_003, &[7], true),
        run(&mut routed, 10_003, &[7], true)
    );
    muted.set_channel_mutes([false; 4]);
    routed.write(NR51, 0x11);
    assert_eq!(
        run(&mut muted, 20_007, &[1, 4], false),
        run(&mut routed, 20_007, &[1, 4], false)
    );
}

#[test]
fn rate_power_and_state_restore_clear_only_host_output_history() {
    let mut apu = pulse(44_100, 2025);
    apu.step(50_000);
    let state = hardware_state(&apu);
    apu.set_sample_rate(96_000);
    assert!(apu.drain_samples().is_empty());
    assert_eq!(hardware_state(&apu), state);
    let mut restored = Apu::read_state(
        &mut StateReader::new(&state),
        crate::save_state::SAVE_STATE_FORMAT_VERSION,
    )
    .unwrap();
    restored.set_sample_rate(96_000);
    assert_eq!(
        run(&mut apu, 15_003, &[4], false),
        run(&mut restored, 15_003, &[4], false)
    );
    apu.step(50_000);
    apu.write(NR52, 0);
    assert!(apu.drain_samples().is_empty());
    apu.step(50_000);
    assert!(apu.drain_samples().is_empty());
    apu.write(NR52, 0x80);
    assert!(
        run(&mut apu, 50_000, &[4], true)
            .iter()
            .all(|&sample| sample == 0.0)
    );
}

#[test]
fn output_sample_count_and_stereo_routing_survive_short_drains() {
    for rate in [8000, 44_100, 48_000, 96_000, 192_000] {
        let mut apu = pulse(rate, 2025);
        apu.write(NR51, 0x10);
        let samples = run(&mut apu, 4_194_304, &[4093, 1, 17], true);
        assert_eq!(samples.len(), rate as usize * 2);
        assert!(samples.chunks_exact(2).all(|pair| pair[1] == 0.0));
        assert!(samples.chunks_exact(2).any(|pair| pair[0] != 0.0));
    }
}

#[test]
fn disabling_output_discards_queued_audio_without_changing_hardware() {
    let mut apu = pulse(48_000, 2025);
    apu.step(10_000);
    let state = hardware_state(&apu);
    apu.set_sample_generation_enabled(false);
    assert!(apu.drain_samples().is_empty());
    apu.set_sample_generation_enabled(true);
    assert_eq!(hardware_state(&apu), state);
    apu.step(10_000);
    let state = hardware_state(&apu);
    apu.set_enabled(false);
    assert!(apu.drain_samples().is_empty());
    apu.set_enabled(true);
    assert_eq!(hardware_state(&apu), state);
}

#[test]
fn resampler_impulse_latency_does_not_extend_requested_duration() {
    let mut renderer = super::output::StereoOutput::new(48_000);
    renderer.set_mixer(0x77, 0x11, [false; 4]);
    renderer.record_channel(0, 0, 1.0);
    renderer.record_channel(0, 32, 0.0);
    let mut output = Vec::new();
    renderer.finish_step(4096, &mut output);
    renderer.flush(&mut output);
    assert_eq!(output.len(), 46 * 2);
    let peak = output
        .chunks_exact(2)
        .enumerate()
        .max_by(|(_, left), (_, right)| left[0].abs().total_cmp(&right[0].abs()))
        .unwrap()
        .0;
    assert!((8..=9).contains(&peak), "impulse peak at sample {peak}");
    let length = output.len();
    renderer.flush(&mut output);
    assert_eq!(output.len(), length);
}

#[test]
fn transition_capture_matches_observing_every_channel_after_every_clock() {
    for rate in [8_001, 44_100, 48_000, 48_001, 96_000, 192_001] {
        for (cgb, double_speed) in [(false, false), (true, false), (true, true)] {
            let mut captured = all_channels(cgb, double_speed, true);
            let mut observed = all_channels(cgb, double_speed, true);
            for apu in [&mut captured, &mut observed] {
                apu.set_sample_rate(rate);
            }
            for index in 0..256 {
                for apu in [&mut captured, &mut observed] {
                    if index % 5 == 0 {
                        apu.clock_div_apu();
                    }
                    if index % 7 == 0 {
                        apu.write(NR50, index as u8);
                        apu.write(NR51, (index as u8).rotate_left(3));
                        apu.set_channel_mutes(std::array::from_fn(|c| index & (1 << c) != 0));
                    }
                    if index % 11 == 0 {
                        apu.write(NR10, 0x13);
                        apu.write(NR11, index as u8);
                        apu.write(NR21, (index as u8).rotate_left(2));
                        apu.write(NR12, 0xA1);
                        apu.write(NR22, 0x79);
                        apu.write(NR42, 0xB1);
                        apu.write(NR13, index as u8);
                        apu.write(NR23, index as u8);
                        apu.write(NR14, 0xC7);
                        apu.write(NR24, 0xC7);
                        apu.write(NR43, index as u8);
                        apu.write(NR44, 0xC0);
                    }
                    if index % 13 == 0 {
                        apu.write(NR30, (index as u8).rotate_left(5));
                        apu.write(NR32, ((index & 3) << 5) as u8);
                        apu.write(NR33, index as u8);
                        apu.write(NR34, 0xC7);
                        apu.write(WAVE_RAM_START + (index & 15), index as u8);
                    }
                    if index % 29 == 0 {
                        apu.write(NR12, 0);
                        apu.write(NR22, 0);
                        apu.write(NR42, 0);
                    }
                    if index % 31 == 0 {
                        apu.set_sample_rate(if index & 1 == 0 { rate } else { 48_001 });
                    }
                    if index % 37 == 0 {
                        let state = hardware_state(apu);
                        *apu = Apu::read_state(
                            &mut StateReader::new(&state),
                            crate::save_state::SAVE_STATE_FORMAT_VERSION,
                        )
                        .unwrap();
                        apu.set_cgb_hardware(cgb);
                        apu.set_cgb_double_speed(double_speed);
                        apu.set_sample_rate(rate);
                    }
                }
                for _ in 0..[1, 2, 3, 4, 7, 31, 257, 1025][index as usize % 8] {
                    captured.step(1);
                    observed.step(1);
                    for channel in 0..4 {
                        observed.capture_output_channel(channel, 0);
                    }
                    observed.generate_samples(0);
                }
                let actual = captured.drain_samples();
                let expected = observed.drain_samples();
                assert_eq!(
                    actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    "rate={rate} cgb={cgb} double_speed={double_speed} index={index}"
                );
                assert_eq!(hardware_state(&captured), hardware_state(&observed));
            }
        }
    }
}
