use super::*;
mod epoch;
use zeff_emu_common::audio_trace::{
    AudioTraceInvalidation, MdTraceOrigin, MdTraceSource, MdTraceWrite, MdTraceWriter,
};

#[derive(Default)]
struct Boundaries {
    count: usize,
    stopped: bool,
    interrupt: bool,
    trace: bool,
}

fn reference_field(machine: &mut Emulator) -> (Result<(), MachineFault>, Boundaries) {
    let field = machine.pacing_field_count();
    machine.clear_frame_ready();
    let mut boundaries = Boundaries::default();
    while machine.pacing_field_count() == field {
        boundaries.count += 1;
        assert!(
            boundaries.count < 200_000,
            "synthetic field did not terminate"
        );
        match machine.step_instruction() {
            Err(fault) => return (Err(fault), boundaries),
            Ok(step) => {
                boundaries.stopped |= step.cpu == TimedOutcome::Stopped;
                boundaries.interrupt |= matches!(step.cpu, TimedOutcome::Interrupt { .. });
                boundaries.trace |= match step.cpu {
                    TimedOutcome::Trace { .. } => true,
                    TimedOutcome::Instruction(step) => step.exception == Some(9),
                    _ => false,
                };
            }
        }
    }
    (Ok(()), boundaries)
}

fn assert_observations(actual: &Emulator, expected: &Emulator) {
    assert!(actual == expected, "full machine differs at field return");
    assert_eq!(actual.timing_snapshot(), expected.timing_snapshot());
    assert_eq!(
        actual.framebuffer_dimensions(),
        expected.framebuffer_dimensions()
    );
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    assert_eq!(actual.frame_ready(), expected.frame_ready());
    assert_eq!(actual.frame_count(), expected.frame_count());
    assert_eq!(actual.video_error(), expected.video_error());
    assert_eq!(
        actual.encode_state().map_err(|error| error.to_string()),
        expected.encode_state().map_err(|error| error.to_string())
    );
    let mut a = actual.clone();
    let mut b = expected.clone();
    assert_eq!(
        a.drain_audio_samples()
            .into_iter()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
        b.drain_audio_samples()
            .into_iter()
            .map(f32::to_bits)
            .collect::<Vec<_>>()
    );
    assert_eq!(a.take_vdp_capture(), b.take_vdp_capture());
    assert_eq!(a.finish_audio_trace(), b.finish_audio_trace());
    assert!(a == b);
    assert!(actual.bus.audio.trace_origin.is_none());
}

fn compare_field(
    actual: &mut Emulator,
    expected: &mut Emulator,
) -> (Result<(), MachineFault>, Boundaries) {
    let (result, boundaries) = reference_field(expected);
    assert_eq!(actual.step_field(), result);
    assert_observations(actual, expected);
    (result, boundaries)
}

fn fixture(region: Region, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for vector in [9, 30] {
        rom[vector * 4..vector * 4 + 4].copy_from_slice(&[0, 0, 3, 0]);
    }
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom[0x300..0x304].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn audio_program() -> Vec<u16> {
    let mut program = Vec::new();
    for (address, value) in [
        (0xa11100, 1),
        (0xa11200, 0),
        (0xa11200, 1),
        (0xa04002, 0xb6),
        (0xa04003, 0xc0),
        (0xa04000, 0x2a),
        (0xa04001, 0xd3),
        (0xa04000, 0x2b),
        (0xa04001, 0x80),
        (0xc00011, 0x85),
        (0xc00011, 0x06),
        (0xc00011, 0x90),
    ] {
        write_byte(&mut program, address, value);
    }
    program.extend([0x1039, 0x00a0, 0x4000, 0x60fe, 0x4e71]);
    program
}

fn advance(machine: &mut Emulator, ticks: u64) {
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .advance_machine(ticks)
        .unwrap();
    } else {
        machine.bus.advance_machine(ticks).unwrap();
    }
}

fn configure(machine: &mut Emulator, h40: bool, interlace: bool) {
    for (index, value) in [
        (
            1,
            if machine.region() == Region::Pal {
                0x4c
            } else {
                0x44
            },
        ),
        (
            12,
            (if h40 { 0x81 } else { 0 }) | if interlace { 6 } else { 0 },
        ),
    ] {
        advance(machine, 1);
        let pending = machine.queue_vdp_register(index, value).unwrap();
        advance(machine, pending.publish_at.get() - machine.bus.now.get());
    }
}

fn near_field_end(machine: &mut Emulator, remaining: u64) {
    let end = machine
        .vdp()
        .interrupts()
        .next_field_end(machine.region(), machine.vdp().registers())
        .unwrap();
    advance(machine, end.get() - remaining - machine.bus.now.get());
}

#[test]
fn production_field_matches_old_loop_for_regions_widths_and_interlace_parities() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for h40 in [false, true] {
                for interlace in [false, true] {
                    let mut actual = fixture(region, concurrent, &audio_program());
                    configure(&mut actual, h40, interlace);
                    let mut expected = actual.clone();
                    let mut parities = [false; 2];
                    for _ in 0..3 {
                        let old = actual.pacing_field_count();
                        let (result, boundaries) = compare_field(&mut actual, &mut expected);
                        result.unwrap();
                        assert!(boundaries.count > 1);
                        assert_eq!(actual.pacing_field_count(), old + 1);
                        let position = actual.vdp().field_position(region).unwrap();
                        if interlace {
                            assert!(position.mode.interlaced());
                            parities[usize::from(position.active_odd)] = true;
                        }
                        if !interlace {
                            assert!(actual.frame_ready());
                        }
                        assert_eq!(
                            actual.framebuffer_dimensions().0,
                            if h40 { 320 } else { 256 }
                        );
                    }
                    if interlace {
                        assert_eq!(parities, [true; 2]);
                        assert_eq!(
                            actual.framebuffer_dimensions().1,
                            if region == Region::Pal { 480 } else { 448 }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cold_restored_midfield_and_initial_readiness_keep_exact_continuation() {
    for concurrent in [false, true] {
        let mut cold = fixture(Region::NtscUs, concurrent, &[0x60fe, 0x4e71]);
        let mut cold_reference = cold.clone();
        compare_field(&mut cold, &mut cold_reference).0.unwrap();
        for ready in [false, true] {
            let mut actual = fixture(Region::Pal, concurrent, &audio_program());
            configure(&mut actual, true, true);
            while !actual.frame_ready() {
                reference_field(&mut actual).0.unwrap();
            }
            if !ready {
                for _ in 0..113 {
                    actual.step_instruction().unwrap();
                }
                actual.clear_frame_ready();
            }
            assert_eq!(actual.frame_ready(), ready);
            let saved = actual.encode_state().unwrap();
            actual.decode_state(&saved).unwrap();
            assert_eq!(actual.encode_state().unwrap(), saved);
            let mut expected = actual.clone();
            let now = actual.bus.now;
            let (result, boundaries) = compare_field(&mut actual, &mut expected);
            result.unwrap();
            assert!(boundaries.count > 1);
            assert!(actual.bus.now > now);
            for machine in [&mut actual, &mut expected] {
                machine.set_sample_rate(44_100);
                machine.set_apu_channel_mutes([true, false, false, false]);
                machine.set_fm_channel_mutes([false, true, false, false, false, false]);
                machine
                    .input_mut()
                    .set_controller_raw(crate::ControllerPort::One, 0x7f);
            }
            compare_field(&mut actual, &mut expected).0.unwrap();
            for _ in 0..32 {
                assert_eq!(actual.step_instruction(), expected.step_instruction());
                assert_observations(&actual, &expected);
            }
        }
    }
}

#[test]
fn generation_rates_disabled_output_and_full_queue_preserve_pcm_bits() {
    for concurrent in [false, true] {
        for rate in [1, 44_100, 48_000, 192_000] {
            for enabled in [false, true] {
                let mut actual = fixture(Region::Pal, concurrent, &audio_program());
                actual.set_sample_rate(rate);
                actual.set_apu_sample_generation_enabled(enabled);
                let mut expected = actual.clone();
                for _ in 0..3 {
                    compare_field(&mut actual, &mut expected).0.unwrap();
                }
                let pcm = actual.clone().drain_audio_samples();
                if !enabled {
                    assert!(pcm.is_empty());
                } else if rate == 192_000 {
                    assert_eq!(pcm.len(), 4096 * 2);
                    assert!(pcm.iter().any(|sample| *sample != 0.0));
                }
                for machine in [&mut actual, &mut expected] {
                    machine.set_apu_sample_generation_enabled(!enabled);
                }
                compare_field(&mut actual, &mut expected).0.unwrap();
            }
        }
    }
}

#[test]
fn live_z80_status_replay_pending_publication_and_stopped_main_match() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for stopped in [false, true] {
            let mut actual = cpu_continuation_tests::machine(region, stopped);
            for _ in 0..2000 {
                if actual.sound_cpu.cycles() >= 400 && actual.sound_schedule.pending.is_some() {
                    break;
                }
                actual.step_instruction().unwrap();
            }
            assert!(actual.sound_cpu.cycles() >= 400);
            assert!(actual.sound_schedule.pending.is_some());
            let mut expected = actual.clone();
            for _ in 0..2 {
                let (result, boundaries) = compare_field(&mut actual, &mut expected);
                result.unwrap();
                assert_eq!(boundaries.stopped, stopped);
            }
            assert!(actual.opn2_snapshot().dac_enabled);
            assert!(
                actual
                    .clone()
                    .drain_audio_samples()
                    .iter()
                    .any(|sample| *sample != 0.0)
            );
        }
    }
}

#[test]
fn pending_banked_store_and_z80_trace_publish_through_the_real_field_path() {
    let mut actual = fixture(Region::NtscUs, true, &[0x2010, 0x60fc, 0x4e71]);
    actual.reset_and_begin_audio_trace(16).unwrap();
    actual.cpu.registers.a[0] = 0xff7ffe;
    actual.bus.ram.fill(0xde);
    actual.bus.sound.set_reset(false);
    actual.bus.sound.bank = 0x1ff;
    let sound = [
        0x21, 0x00, 0x80, 0x36, 0x55, 0x3e, 0x90, 0x32, 0x11, 0x7f, 0x3a, 0x00, 0x40, 0x76,
    ];
    actual.bus.sound.ram[..sound.len()].copy_from_slice(&sound);
    for _ in 0..1000 {
        if actual
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| pending.bank_access.is_some())
        {
            break;
        }
        advance(&mut actual, 1);
    }
    let pending = actual.sound_schedule.pending.as_ref().unwrap();
    assert!(pending.bank_access.is_some() && pending.reservation().is_some());
    assert!(pending.deadline > actual.bus.now.get());
    assert_eq!(actual.peek(0xff8000), Some(0xde));
    let mut publication = actual.clone();
    for _ in 0..1000 {
        if publication
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| pending.psg_writes != Default::default())
        {
            break;
        }
        advance(&mut publication, 1);
    }
    let pending = publication.sound_schedule.pending.as_ref().unwrap();
    let origin = MdTraceOrigin {
        writer: MdTraceWriter::Z80,
        boundary_pc: 10,
        source: MdTraceSource::SoundRam { offset: 10 },
    };
    assert_eq!(publication.sound_cpu.regs().pc, 10);
    assert!(publication.sound_schedule.state.pending());
    assert_ne!(pending.psg_writes, Default::default());
    assert_eq!(pending.trace_origin, Some(origin));
    assert!(pending.deadline > publication.bus.now.get());
    let mut expected = actual.clone();
    compare_field(&mut actual, &mut expected).0.unwrap();
    assert_eq!(actual.peek(0xff8000), Some(0x55));
    assert_eq!(actual.registers().d[0], 0xdede55de);
    let capture = actual.finish_audio_trace().unwrap();
    assert_eq!(Some(capture.clone()), expected.finish_audio_trace());
    capture.trace.validate_complete().unwrap();
    assert!(capture.trace.events.iter().any(|event| event.write
        == MdTraceWrite::Psg {
            origin,
            value: 0x90
        }));
    assert_observations(&actual, &expected);
}

#[test]
fn stop_polls_wake_on_vdp_irq_and_cpu_trace_exception_continues() {
    for concurrent in [false, true] {
        let mut actual = fixture(
            Region::NtscUs,
            concurrent,
            &[0x4e72, 0x2000, 0x60fe, 0x4e71],
        );
        let pending = actual.queue_vdp_register(1, 0x64).unwrap();
        let delta = pending.publish_at.get() - actual.bus.now.get();
        advance(&mut actual, delta);
        let mut expected = actual.clone();
        let (result, boundaries) = compare_field(&mut actual, &mut expected);
        result.unwrap();
        assert!(boundaries.stopped && boundaries.interrupt);
        assert!(!actual.cpu.stopped());
        assert_eq!(actual.registers().pc, 0x300);

        let mut actual = fixture(
            Region::NtscJapan,
            concurrent,
            &[0x46fc, 0xa000, 0x4e71, 0x60fe, 0x4e71],
        );
        let mut expected = actual.clone();
        let (result, boundaries) = compare_field(&mut actual, &mut expected);
        result.unwrap();
        assert!(boundaries.trace);
        assert_eq!(actual.registers().pc, 0x300);
    }
}

#[test]
fn main_trace_origins_full_recorders_and_future_instruction_returns_match() {
    for concurrent in [false, true] {
        for capacity in [1, 64] {
            let mut actual = fixture(Region::NtscUs, concurrent, &audio_program());
            actual.reset_and_begin_audio_trace(capacity).unwrap();
            let mut expected = actual.clone();
            compare_field(&mut actual, &mut expected).0.unwrap();
            let capture = actual.clone().finish_audio_trace().unwrap();
            assert!(!capture.trace.events.is_empty());
            assert_eq!(capture.trace.invalidated, None);
            let mut pcs = Vec::new();
            for event in &capture.trace.events {
                let origin = match event.write {
                    MdTraceWrite::Psg { origin, .. }
                    | MdTraceWrite::Opn2Port { origin, .. }
                    | MdTraceWrite::Opn2Reset { origin } => origin,
                };
                assert_eq!(origin.writer, MdTraceWriter::M68000);
                assert_eq!(origin.boundary_pc, event.pc);
                assert_eq!(
                    origin.source,
                    MdTraceSource::EffectiveRom { offset: event.pc }
                );
                pcs.push(event.pc);
            }
            if capacity == 64 {
                capture.trace.validate_complete().unwrap();
                pcs.dedup();
                assert!(pcs.len() > 3);
            } else {
                assert!(capture.trace.dropped_events > 0);
            }
            compare_field(&mut actual, &mut expected).0.unwrap();
            assert_eq!(actual.finish_audio_trace(), expected.finish_audio_trace());
            for _ in 0..16 {
                assert_eq!(actual.step_instruction(), expected.step_instruction());
            }
            assert_observations(&actual, &expected);
        }
    }
}

#[test]
fn pending_completed_captures_and_stored_raster_errors_do_not_end_a_field() {
    for concurrent in [false, true] {
        for completed in [false, true] {
            let mut actual = fixture(Region::NtscUs, concurrent, &[0x60fe, 0x4e71]);
            actual.step_field().unwrap();
            near_field_end(&mut actual, 500);
            let error = crate::VdpLineError::Render(crate::VdpRenderError::PendingLineInputs);
            actual
                .bus
                .video
                .publish_row(0, actual.pacing_field_count(), 224, Err(error));
            actual
                .queue_vdp_capture(MasterTicks::new(actual.bus.now.get() + 1))
                .unwrap();
            if completed {
                advance(&mut actual, 1);
            }
            let mut expected = actual.clone();
            let field = actual.pacing_field_count();
            compare_field(&mut actual, &mut expected).0.unwrap();
            assert_eq!(actual.pacing_field_count(), field + 1);
            assert_eq!(actual.video_error(), Some(error));
            assert!(actual.take_vdp_capture().is_some());
            assert!(expected.take_vdp_capture().is_some());
            assert_observations(&actual, &expected);
        }
    }
}

#[test]
fn faults_flush_only_admitted_prefix_and_win_over_a_field_crossing() {
    for concurrent in [false, true] {
        for crossing in [false, true] {
            let mut program = vec![0x7055, 0x4e71, 0x4e71];
            write_byte(&mut program, 0xc00011, 0x90);
            program.extend([0x23c0, 0x0080, 0, 0x4e71, 0x4e71]);
            let mut actual = fixture(Region::NtscUs, concurrent, &program);
            actual.reset_and_begin_audio_trace(16).unwrap();
            if crossing {
                near_field_end(&mut actual, 1000);
                let mut prefix = actual.clone();
                for _ in 0..4 {
                    prefix.step_instruction().unwrap();
                }
                let elapsed = prefix.bus.now.get() - actual.bus.now.get();
                near_field_end(&mut actual, elapsed + 1);
            }
            let field = actual.pacing_field_count();
            let mut expected = actual.clone();
            let (result, boundaries) = compare_field(&mut actual, &mut expected);
            assert!(matches!(
                result,
                Err(MachineFault::Main(TimingFault::Bus {
                    source: BusError::UnsupportedAccess {
                        address: 0x800000,
                        ..
                    },
                    ..
                }))
            ));
            assert!(boundaries.count > 1);
            assert_eq!(actual.registers().d[0], 0x55);
            assert_eq!(actual.pacing_field_count() != field, crossing);
            let trace = actual.clone().finish_audio_trace().unwrap().trace;
            assert_eq!(
                trace.invalidated,
                Some(AudioTraceInvalidation::ExecutionFault)
            );
            assert_eq!(trace.events.len(), 1);
            compare_field(&mut actual, &mut expected).0.unwrap_err();
        }
        for sound in [false, true] {
            let mut actual = fixture(Region::NtscUs, concurrent, &[0x4e72, 0x2700, 0x4e71]);
            actual.reset_and_begin_audio_trace(16).unwrap();
            actual.step_field().unwrap();
            assert!(actual.frame_ready());
            if sound {
                actual.sound_schedule.fault = Some(SoundFault::ClockOverflow);
            } else {
                actual.bus.now = MasterTicks::new(u64::MAX - 1);
            }
            let now = actual.bus.now;
            let mut expected = actual.clone();
            let (result, boundaries) = compare_field(&mut actual, &mut expected);
            assert!(result.is_err());
            assert_eq!(boundaries.count, 1);
            assert_eq!(actual.bus.now, now);
            assert!(!actual.frame_ready());
            assert_eq!(
                actual
                    .clone()
                    .finish_audio_trace()
                    .unwrap()
                    .trace
                    .invalidated,
                Some(AudioTraceInvalidation::ClockOverflow)
            );
            compare_field(&mut actual, &mut expected).0.unwrap_err();
        }
    }
}

mod error_prefixes;
