use super::*;
use crate::constants::BIOS_SIZE;
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, MAX_AUDIO_TRACE_EVENTS};

const PROGRAM: &[u8] = &[
    0x3E, 0x84, 0xD3, 0xE0, 0x3E, 0x01, 0xD3, 0xFF, 0x3E, 0x90, 0xD3, 0xE1, 0x3E, 0xE7, 0xD3, 0xE0,
    0x3E, 0xF2, 0xD3, 0xFE, 0x76,
];

fn cartridge(program: &[u8]) -> Vec<u8> {
    let mut cartridge = vec![0; 0x8000];
    cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
    cartridge[2..2 + program.len()].copy_from_slice(program);
    cartridge
}

fn emulator(program: &[u8]) -> Emulator {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..program.len()].copy_from_slice(program);
    Emulator::new(&cartridge(&[]), &bios, 48_000).unwrap()
}

fn assert_machine_equal(left: &mut Emulator, right: &mut Emulator) {
    assert_eq!(left.cpu().regs(), right.cpu().regs());
    assert_eq!(left.cpu_cycles(), right.cpu_cycles());
    assert_eq!(left.effective_cycles(), right.effective_cycles());
    assert_eq!(left.framebuffer(), right.framebuffer());
    assert_eq!(left.save_state().unwrap(), right.save_state().unwrap());
    let left_samples: Vec<_> = left
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    let right_samples: Vec<_> = right
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    assert!(left_samples.iter().any(|sample| *sample != 0));
    assert_eq!(left_samples, right_samples);
}

#[test]
fn capture_preserves_pcm_state_and_ready_completion_timing() {
    let mut baseline = emulator(PROGRAM);
    let mut captured = emulator(PROGRAM);
    captured.reset_and_begin_audio_trace(16).unwrap();
    baseline.step_frame();
    captured.step_frame();
    assert_machine_equal(&mut baseline, &mut captured);
    assert!(baseline.finish_audio_trace().is_none());
    let trace = captured.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.end_cycle, captured.effective_cycles());
    assert_ne!(trace.end_cycle, captured.cpu_cycles());
    assert_eq!(trace.cycle_hz, 3_579_545);
    assert_eq!(trace.chip.clock_hz, 3_579_545);
    assert_eq!(trace.timing, AudioTraceTiming::IoWriteCompletion);
    assert_eq!(trace.chip.shift_register_width, 15);
    assert_eq!(trace.chip.feedback_mask, 3);
    assert_eq!(trace.chip.zero_period, Sn76489ZeroPeriod::Period1024);
    assert_eq!(trace.chip.reset.noise_lfsr, 0x4000);
    assert_eq!(trace.events.len(), 5);
    for (event, (cycle, pc, port, value)) in trace.events.iter().zip([
        (48, 2, 0xE0, 0x84),
        (96, 6, 0xFF, 1),
        (144, 10, 0xE1, 0x90),
        (192, 14, 0xE0, 0xE7),
        (240, 18, 0xFE, 0xF2),
    ]) {
        assert_eq!((event.cycle, event.pc), (cycle, pc));
        assert_eq!(
            event.instruction_source,
            AudioTraceSource::BootRom {
                offset: u64::from(pc)
            }
        );
        assert_eq!(event.write, AudioTraceWrite::Sn76489 { port, value });
    }
}

#[test]
fn prefix_waits_ed_output_and_block_output_tails_have_distinct_commit_cycles() {
    let mut program = vec![
        0x3E, 0x9F, 0xDD, 0xFD, 0xD3, 0xE0, 0x01, 0xE0, 0x90, 0xED, 0x41, 0x21, 0x20, 0, 0x01,
        0xE0, 2, 0xED, 0xB3, 0x76,
    ];
    program.resize(0x20, 0);
    program.extend([0x84, 0x01]);
    let mut baseline = emulator(&program);
    let mut captured = emulator(&program);
    captured.reset_and_begin_audio_trace(8).unwrap();
    baseline.set_instruction_trace_enabled(true);
    captured.set_instruction_trace_enabled(true);
    for _ in 0..100 {
        let (left, left_bus) = baseline.step_instruction_with_bus_trace();
        let (right, right_bus) = captured.step_instruction_with_bus_trace();
        assert_eq!(left, right);
        assert_eq!(left_bus, right_bus);
    }
    assert_machine_equal(&mut baseline, &mut captured);
    let trace = captured.finish_audio_trace().unwrap();
    assert_eq!(
        trace
            .events
            .iter()
            .map(|event| (event.cycle, event.pc))
            .collect::<Vec<_>>(),
        [(58, 2), (111, 9), (179, 17), (230, 17)]
    );
}

#[test]
fn cartridge_and_mirrored_ram_writers_have_distinct_provenance() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..3].copy_from_slice(&[0xC3, 0x02, 0x80]);
    let mut emu =
        Emulator::new(&cartridge(&[0x3E, 0x9F, 0xD3, 0xE0, 0x76]), &bios, 48_000).unwrap();
    emu.reset_and_begin_audio_trace(4).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, 0x8004);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 4,
            bit_reversed: false
        }
    );

    let mut program = vec![
        0x21, 0x20, 0, 0x11, 0, 0x64, 0x01, 5, 0, 0xED, 0xB0, 0xC3, 0, 0x64,
    ];
    program.resize(0x20, 0);
    program.extend([0x3E, 0x9F, 0xD3, 0xFF, 0x76]);
    let mut emu = emulator(&program);
    emu.reset_and_begin_audio_trace(4).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, 0x6402);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::WorkRam { offset: 2 }
    );
}

#[test]
fn unimplemented_expansion_ports_are_not_psg_events() {
    let mut emu = emulator(&[
        0xD3, 0x50, 0xD3, 0x51, 0xD3, 0x53, 0xD3, 0x7F, 0xD3, 0xDF, 0x76,
    ]);
    emu.reset_and_begin_audio_trace(4).unwrap();
    emu.step_frame();
    assert!(emu.finish_audio_trace().unwrap().events.is_empty());
}

#[test]
fn capacity_loss_reset_restore_and_external_writes_are_explicit() {
    let mut emu = emulator(PROGRAM);
    emu.reset_and_begin_audio_trace(1).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.dropped_events, 4);
    assert!(trace.validate_complete().is_err());
    emu.reset_and_begin_audio_trace(1).unwrap();
    let empty = emu.finish_audio_trace().unwrap();
    assert_eq!(empty.generation, trace.generation + 1);
    assert!(empty.events.is_empty());
    assert_eq!(empty.end_cycle, 0);

    for mutation in 0..6 {
        emu.reset_and_begin_audio_trace(16).unwrap();
        emu.step_instruction();
        emu.step_instruction();
        let expected = match mutation {
            0 => {
                emu.reset();
                AudioTraceInvalidation::Reset
            }
            1 => {
                let state = emu.save_state().unwrap();
                emu.load_state(&state).unwrap();
                AudioTraceInvalidation::StateRestore
            }
            2 => {
                let state = emu.encode_external_state().unwrap();
                emu.load_external_state(&state).unwrap();
                AudioTraceInvalidation::StateRestore
            }
            3 => {
                emu.bus_mut();
                AudioTraceInvalidation::ExternalMutation
            }
            4 => {
                emu.cpu_write8(0x6000, 0);
                AudioTraceInvalidation::ExternalMutation
            }
            _ => {
                zeff_emu_common::cheats::CheatByteTarget::cheat_write8(&mut emu, 0x6000, 0);
                AudioTraceInvalidation::ExternalMutation
            }
        };
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(trace.invalidated, Some(expected));
        assert_eq!(trace.events.len(), 1);
        assert!(trace.validate_complete().is_err());
    }
}

#[test]
fn invalid_requests_do_not_reset_or_discard_the_running_capture() {
    let mut emu = emulator(PROGRAM);
    emu.reset_and_begin_audio_trace(16).unwrap();
    emu.step_instruction();
    let state = emu.save_state().unwrap();
    for capacity in [0, MAX_AUDIO_TRACE_EVENTS + 1, usize::MAX] {
        assert!(emu.reset_and_begin_audio_trace(capacity).is_err());
        assert_eq!(state, emu.save_state().unwrap());
    }
    assert!(emu.load_state(&[0]).is_err());
    emu.step_instruction();
    let trace = emu.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.events[0].cycle, 48);
}
