use super::*;
use crate::emulator::Sega8LoadConfig;
use crate::hardware::cartridge::{Sega8MapperKind, SystemHint};
use crate::hardware::timing::Sega8VideoStandard;
use zeff_emu_common::audio_trace::{
    AudioTraceInvalidation, AudioTraceSource, AudioTraceWrite, MAX_AUDIO_TRACE_EVENTS,
};

const PROGRAM: &[u8] = &[
    0x3E, 0x84, 0xD3, 0x7F, 0x3E, 0x01, 0xD3, 0x40, 0x3E, 0x90, 0xD3, 0x7E, 0x3E, 0x11, 0xD3, 0x06,
    0x3E, 0xE7, 0xD3, 0x7F, 0x3E, 0xF2, 0xD3, 0x7F, 0x76,
];

fn rom(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; 0x10000];
    rom[..program.len()].copy_from_slice(program);
    rom
}

fn emulator(program: &[u8], hint: SystemHint, video: Sega8VideoStandard) -> Emulator {
    Emulator::new_with_hint_and_video_standard(&rom(program), 48_000, hint, video).unwrap()
}

fn assert_machine_equal(left: &mut Emulator, right: &mut Emulator) {
    assert_eq!(left.cpu().regs(), right.cpu().regs());
    assert_eq!(left.cpu_cycles(), right.cpu_cycles());
    assert_eq!(left.framebuffer(), right.framebuffer());
    assert_eq!(left.encode_state().unwrap(), right.encode_state().unwrap());
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
fn capture_preserves_pcm_state_and_instruction_boundary_writes_for_every_sega_system() {
    for hint in [
        SystemHint::MasterSystem,
        SystemHint::GameGear,
        SystemHint::Sg1000,
    ] {
        for video in [Sega8VideoStandard::Ntsc, Sega8VideoStandard::Pal] {
            let mut baseline = emulator(PROGRAM, hint, video);
            let mut captured = emulator(PROGRAM, hint, video);
            captured.reset_and_begin_audio_trace(32).unwrap();
            baseline.step_frame();
            captured.step_frame();
            assert_machine_equal(&mut baseline, &mut captured);
            assert!(baseline.finish_audio_trace().is_none());
            let trace = captured.finish_audio_trace().unwrap();
            trace.validate_complete().unwrap();
            assert_eq!(trace.end_cycle, captured.cpu_cycles());
            assert_eq!(trace.cycle_hz, video.clock_hz_approx());
            assert_eq!(trace.chip.clock_hz, video.clock_hz_approx());
            assert_eq!(trace.timing, AudioTraceTiming::InstructionBoundary);
            assert_eq!(trace.chip.stereo, hint == SystemHint::GameGear);
            assert_eq!(trace.chip.feedback_mask, 9);
            assert_eq!(trace.chip.reset.noise_lfsr, 0x8000);
            let mut expected = vec![
                (7, 2, 0x7F, 0x84),
                (25, 6, 0x40, 0x01),
                (43, 10, 0x7E, 0x90),
            ];
            if hint == SystemHint::GameGear {
                expected.push((61, 14, 0x06, 0x11));
            }
            expected.extend([(79, 18, 0x7F, 0xE7), (97, 22, 0x7F, 0xF2)]);
            assert_eq!(trace.events.len(), expected.len());
            for (event, (cycle, pc, port, value)) in trace.events.iter().zip(expected) {
                assert_eq!((event.cycle, event.pc), (cycle, pc));
                assert_eq!(
                    event.instruction_source,
                    AudioTraceSource::CartridgeRom {
                        offset: u64::from(pc),
                        bit_reversed: false
                    }
                );
                let write = if port == 6 {
                    AudioTraceWrite::GameGearStereo { port, value }
                } else {
                    AudioTraceWrite::Sn76489 { port, value }
                };
                assert_eq!(event.write, write);
            }
        }
    }
}

#[test]
fn capture_coexists_with_instruction_and_bus_tracing() {
    let mut baseline = emulator(PROGRAM, SystemHint::GameGear, Sega8VideoStandard::Ntsc);
    let mut captured = baseline.clone();
    baseline.set_instruction_trace_enabled(true);
    captured.set_instruction_trace_enabled(true);
    captured.reset_and_begin_audio_trace(32).unwrap();
    for _ in 0..100 {
        let (left, left_bus) = baseline.step_instruction_with_bus_trace();
        let (right, right_bus) = captured.step_instruction_with_bus_trace();
        assert_eq!(left, right);
        assert_eq!(left_bus, right_bus);
    }
    assert_machine_equal(&mut baseline, &mut captured);
    captured
        .finish_audio_trace()
        .unwrap()
        .validate_complete()
        .unwrap();
}

#[test]
fn disabled_io_and_unimplemented_fm_ports_are_not_chip_writes() {
    let program = [
        0x3E, 4, 0xD3, 0x3E, 0xD3, 0x7F, 0x3E, 0, 0xD3, 0x3E, 0xD3, 0xF0, 0xD3, 0xF1, 0xD3, 0xF2,
        0xD3, 0x7F, 0x76,
    ];
    let mut emu = emulator(&program, SystemHint::MasterSystem, Sega8VideoStandard::Ntsc);
    emu.reset_and_begin_audio_trace(4).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, 16);
    assert_eq!(
        trace.events[0].write,
        AudioTraceWrite::Sn76489 {
            port: 0x7F,
            value: 0
        }
    );
}

#[test]
fn writer_provenance_preserves_bank_header_and_transformed_rom_offsets() {
    for header_size in [0, 512] {
        for reversed in [false, true] {
            let program = if reversed {
                vec![0x3E, 0x46, 0x32, 0x00, 0x40, 0xC3, 0x00, 0x40]
            } else {
                vec![0x3E, 3, 0x32, 0xFF, 0xFF, 0xC3, 0x00, 0x80]
            };
            let mut payload = rom(&program);
            for (offset, value) in [0x3Eu8, 0x9F, 0xD3, 0x7F, 0x76].into_iter().enumerate() {
                payload[0xC000 + offset] = if reversed {
                    value.reverse_bits()
                } else {
                    value
                };
            }
            let mut input = vec![0; header_size];
            input.extend(payload);
            let mut config =
                Sega8LoadConfig::new(48_000).with_system_hint(SystemHint::MasterSystem);
            config.mapper_kind = Some(if reversed {
                Sega8MapperKind::Janggun
            } else {
                Sega8MapperKind::Sega
            });
            let mut emu = Emulator::new_with_config(&input, config).unwrap();
            emu.reset_and_begin_audio_trace(4).unwrap();
            emu.step_frame();
            let trace = emu.finish_audio_trace().unwrap();
            assert_eq!(trace.events.len(), 1);
            assert_eq!(trace.events[0].pc, if reversed { 0x4002 } else { 0x8002 });
            assert_eq!(
                trace.events[0].instruction_source,
                AudioTraceSource::CartridgeRom {
                    offset: (0xC002 + header_size) as u64,
                    bit_reversed: reversed
                }
            );
        }
    }
}

#[test]
fn ram_writer_provenance_uses_actual_backing_offset() {
    for (hint, destination, bank_control, expected) in [
        (
            SystemHint::MasterSystem,
            0xE400u16,
            0u8,
            AudioTraceSource::WorkRam { offset: 0x402 },
        ),
        (
            SystemHint::Sg1000,
            0xE400u16,
            0u8,
            AudioTraceSource::WorkRam { offset: 2 },
        ),
        (
            SystemHint::MasterSystem,
            0x8000u16,
            0x0Cu8,
            AudioTraceSource::CartridgeRam { offset: 0x4002 },
        ),
    ] {
        let [low, high] = destination.to_le_bytes();
        let mut program = vec![
            0x3E,
            bank_control,
            0x32,
            0xFC,
            0xFF,
            0x21,
            0x20,
            0,
            0x11,
            low,
            high,
            0x01,
            5,
            0,
            0xED,
            0xB0,
            0xC3,
            low,
            high,
        ];
        program.resize(0x20, 0);
        program.extend([0x3E, 0x9F, 0xD3, 0x7F, 0x76]);
        let mut emu = emulator(&program, hint, Sega8VideoStandard::Ntsc);
        emu.reset_and_begin_audio_trace(4).unwrap();
        emu.step_frame();
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.events[0].instruction_source, expected);
    }
}

#[test]
fn boot_writer_is_not_reported_as_cartridge_code() {
    let mut boot = vec![0; 0x2000];
    boot[..PROGRAM.len()].copy_from_slice(PROGRAM);
    let config = Sega8LoadConfig::new(48_000).with_system_hint(SystemHint::MasterSystem);
    let mut emu = Emulator::new_with_config_and_boot_rom(&rom(&[0x76]), config, &boot).unwrap();
    emu.reset_and_begin_audio_trace(16).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::BootRom { offset: 2 }
    );
}

#[test]
fn overflow_and_discontinuities_cannot_be_exported_as_complete_captures() {
    let mut emu = emulator(PROGRAM, SystemHint::GameGear, Sega8VideoStandard::Ntsc);
    emu.reset_and_begin_audio_trace(1).unwrap();
    emu.step_frame();
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.dropped_events, 5);
    assert!(trace.validate_complete().is_err());
    emu.reset_and_begin_audio_trace(1).unwrap();
    let empty = emu.finish_audio_trace().unwrap();
    assert_eq!(empty.generation, trace.generation + 1);
    assert!(empty.events.is_empty());
    assert_eq!(empty.end_cycle, 0);

    for reason in [
        AudioTraceInvalidation::Reset,
        AudioTraceInvalidation::StateRestore,
        AudioTraceInvalidation::ExternalMutation,
        AudioTraceInvalidation::ClockChanged,
    ] {
        emu.reset_and_begin_audio_trace(16).unwrap();
        emu.step_instruction();
        emu.step_instruction();
        match reason {
            AudioTraceInvalidation::Reset => emu.reset(),
            AudioTraceInvalidation::StateRestore => {
                let state = emu.encode_state().unwrap();
                emu.load_state(&state).unwrap();
            }
            AudioTraceInvalidation::ExternalMutation => {
                emu.bus_mut();
            }
            AudioTraceInvalidation::ClockChanged => emu.set_video_standard(Sega8VideoStandard::Pal),
            _ => unreachable!(),
        }
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(trace.invalidated, Some(reason));
        assert_eq!(trace.events.len(), 1);
        assert!(trace.validate_complete().is_err());
    }
}

#[test]
fn failed_start_and_state_load_leave_the_current_capture_unchanged() {
    let mut emu = emulator(PROGRAM, SystemHint::GameGear, Sega8VideoStandard::Ntsc);
    emu.reset_and_begin_audio_trace(16).unwrap();
    emu.step_instruction();
    let state = emu.encode_state().unwrap();
    for capacity in [0, MAX_AUDIO_TRACE_EVENTS + 1, usize::MAX] {
        assert!(emu.reset_and_begin_audio_trace(capacity).is_err());
        assert_eq!(state, emu.encode_state().unwrap());
    }
    assert!(emu.load_state(&[0]).is_err());
    emu.step_instruction();
    let trace = emu.finish_audio_trace().unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.events[0].cycle, 7);
}
