use super::*;
use crate::hardware::{PadButtons, PsgDebugSnapshot, save_state};

mod lifecycle;
mod sources;
mod timing;

fn rom(program: &[u8]) -> Vec<u8> {
    let mut image = vec![0xEA; 0x2000];
    image[..program.len()].copy_from_slice(program);
    image[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    image
}

fn machine(program: &[u8]) -> PceMachine {
    let mut machine =
        PceMachine::with_controller(rom(program), ControllerPort::two_button()).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
    machine
}

fn run(machine: &mut PceMachine, count: usize) {
    for _ in 0..count {
        machine.step_boundary().unwrap();
    }
}

fn finish(machine: &mut PceMachine) -> Huc6280AudioTrace {
    let trace = machine.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    trace
}

fn assert_reset_matches(reset: &Huc6280ResetState, snapshot: PsgDebugSnapshot) {
    assert_eq!(reset.selected_channel, snapshot.selected_channel);
    assert_eq!(reset.main_amplitude, snapshot.main_amplitude);
    assert_eq!(reset.lfo_frequency, snapshot.lfo_frequency);
    assert_eq!(reset.lfo_control, snapshot.lfo_control);
    assert_eq!(reset.lfo_counter, snapshot.lfo_counter);
    assert_eq!(reset.lfo_phase_valid, snapshot.lfo_phase_valid);
    assert_eq!(reset.gain_scan_clock, snapshot.gain_scan_clock);
    assert_eq!(reset.gain_scan_active, snapshot.gain_scan_active);
    assert_eq!(reset.gain_scan_queued, snapshot.gain_scan_queued);
    assert_eq!(reset.attenuation_latch, snapshot.attenuation_latch);
    assert_eq!(reset.master_tick_remainder, snapshot.master_tick_remainder);
    for (channel, snapshot) in reset.channels.iter().zip(snapshot.channels) {
        assert_eq!(channel.frequency, snapshot.frequency);
        assert_eq!(channel.control, snapshot.control);
        assert_eq!(channel.balance, snapshot.balance);
        assert_eq!(channel.waveform, snapshot.waveform);
        assert_eq!(channel.wave_index, snapshot.wave_index);
        assert_eq!(channel.dda_hold, snapshot.dda_hold);
        assert_eq!(channel.noise_control, snapshot.noise_control);
        assert_eq!(channel.wave_counter, snapshot.wave_counter);
        assert_eq!(channel.noise_counter, snapshot.noise_counter);
        assert_eq!(channel.noise_seed, snapshot.noise_seed);
        assert_eq!(
            channel.effective_left_attenuation,
            snapshot.effective_left_attenuation
        );
        assert_eq!(
            channel.effective_right_attenuation,
            snapshot.effective_right_attenuation
        );
    }
}

#[test]
fn descriptor_matches_the_actual_reset_of_both_psg_revisions() {
    for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
        let mut machine = PceMachine::with_psg_revision(rom(&[0xEA]), revision).unwrap();
        machine
            .devices_mut()
            .psg_mut()
            .write_port(crate::hardware::PsgPort::from_offset(1), 255);
        machine.reset_and_begin_audio_trace(1).unwrap();
        let snapshot = machine.devices().psg().debug_snapshot();
        let trace = finish(&mut machine);
        assert_eq!(trace.cycle_hz, 236_250_000);
        assert_eq!(trace.cycle_hz_denominator, 11);
        assert_eq!(trace.timing, AudioTraceTiming::MemoryWriteCompletion);
        assert_eq!(trace.chip.clock_hz_numerator, 315_000_000);
        assert_eq!(trace.chip.clock_hz_denominator, 88);
        assert_eq!(trace.chip.master_clock_divisor, 6);
        assert_eq!(trace.chip.internal_master_clock_divisor, 3);
        assert_eq!(trace.chip.revision, reset_chip(revision).revision);
        assert_eq!(trace.end_cycle, 0);
        assert!(trace.events.is_empty());
        assert_reset_matches(&trace.chip.reset, snapshot);
    }
}

fn audible_program() -> Vec<u8> {
    let mut program = vec![
        0x03, 0x0C, 0x13, 0, 0x23, 0, 0x03, 0x0D, 0x13, 1, 0x23, 1, 0x03, 0x0E, 0x13, 1, 0x23, 0,
        0xA9, 0, 0x8D, 2, 4, 0x8D, 3, 4, 0xA9, 0x38, 0x8D, 4, 4, 0xA9, 0, 0x8D, 5, 4,
    ];
    let mut write = |register, value| program.extend_from_slice(&[0xA9, value, 0x8D, register, 8]);
    write(1, 255);
    for channel in [0, 1] {
        write(0, channel);
        write(2, 64);
        write(3, 0);
        write(5, 255);
        for sample in 0..32 {
            write(6, if sample < 16 { 0 } else { 31 });
        }
        write(4, 0x9F);
    }
    write(8, 4);
    write(9, 1);
    write(0, 4);
    write(5, 255);
    write(7, 0x9E);
    write(4, 0x9F);
    write(0, 5);
    write(5, 255);
    write(4, 0xDF);
    write(6, 31);
    program.extend_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    program
}

fn pcm(machine: &mut PceMachine) -> Vec<f32> {
    let mut samples = Vec::new();
    machine.drain_audio_samples_into(&mut samples);
    samples
}

#[test]
fn capture_preserves_canonical_state_pcm_video_and_input() {
    let program = audible_program();
    let mut disabled = machine(&program);
    let mut enabled = machine(&program);
    disabled.set_sample_rate(48_000);
    enabled.set_sample_rate(48_000);
    disabled.reset();
    enabled.reset_and_begin_audio_trace(256).unwrap();
    let mut audible = false;
    for buttons in [PadButtons::RUN, PadButtons::I, PadButtons::empty()] {
        for machine in [&mut disabled, &mut enabled] {
            machine
                .controller_input_mut()
                .two_button_pad_mut()
                .unwrap()
                .set_buttons(buttons);
        }
        assert_eq!(
            disabled.run_until_frame().unwrap(),
            enabled.run_until_frame().unwrap()
        );
        assert_eq!(
            save_state::encode_state(&disabled).unwrap(),
            save_state::encode_state(&enabled).unwrap()
        );
        assert_eq!(disabled.framebuffer(), enabled.framebuffer());
        assert_eq!(disabled.debug_snapshot(), enabled.debug_snapshot());
        let expected = pcm(&mut disabled);
        audible |= expected.iter().any(|sample| sample.abs() > 0.01);
        assert_eq!(expected, pcm(&mut enabled));
    }
    assert!(audible);
    assert!(
        enabled
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] != 0)
    );
    let trace = finish(&mut enabled);
    assert_eq!(trace.end_cycle, disabled.master_ticks());
    assert_eq!(trace.events.len(), 85);
    assert!(disabled.finish_audio_trace().is_none());
}

#[test]
fn capture_matches_the_plain_memory_lane_and_scalar_device_schedule() {
    let program = audible_program();
    let mut reference = machine(&program);
    let mut plain = machine(&program);
    let mut scalar = machine(&program);
    reference.set_plain_memory_lane_for_test(false);
    scalar.set_device_advancement_coalescing_for_test(false);
    for machine in [&mut reference, &mut plain, &mut scalar] {
        machine.reset_and_begin_audio_trace(256).unwrap();
        machine.run_until_frame().unwrap();
    }
    let expected_state = save_state::encode_state(&reference).unwrap();
    assert_eq!(expected_state, save_state::encode_state(&plain).unwrap());
    assert_eq!(expected_state, save_state::encode_state(&scalar).unwrap());
    let expected_pcm = pcm(&mut reference);
    assert_eq!(expected_pcm, pcm(&mut plain));
    assert_eq!(expected_pcm, pcm(&mut scalar));
    let expected_trace = finish(&mut reference);
    assert_eq!(expected_trace, finish(&mut plain));
    assert_eq!(expected_trace, finish(&mut scalar));
}
