use super::*;
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, MAX_AUDIO_TRACE_EVENTS};

fn writes() -> Vec<u16> {
    let mut program = Vec::new();
    for value in [0x90, 0x9f, 0x91] {
        move_immediate(&mut program, false, value, 0xc00011);
    }
    program.extend([0x60fe, 0x4e71]);
    program
}

fn invalidated(machine: &mut Emulator, reason: AudioTraceInvalidation) {
    assert!(machine.bus.audio.trace_origin.is_none());
    let trace = machine.finish_audio_trace().unwrap().trace;
    assert_eq!(trace.invalidated, Some(reason));
    assert!(trace.validate_complete().is_err());
    assert!(machine.finish_audio_trace().is_none());
}

#[test]
fn rejected_start_and_finish_preserve_live_machine_and_owned_capture() {
    let mut machine = fixture(Region::NtscUs, true, &writes());
    machine.reset_and_begin_audio_trace(2).unwrap();
    step(&mut machine, 1);
    let before = machine.clone();
    for capacity in [0, MAX_AUDIO_TRACE_EVENTS + 1, usize::MAX] {
        assert!(machine.reset_and_begin_audio_trace(capacity).is_err());
        assert_eq!(machine, before);
    }
    let mut other = machine.clone();
    let saved = machine.encode_state().unwrap();
    let capture = machine.finish_audio_trace().unwrap();
    assert_eq!(capture.trace.events.len(), 1);
    assert_eq!(machine.encode_state().unwrap(), saved);
    assert_eq!(capture, other.finish_audio_trace().unwrap());
    assert!(machine.finish_audio_trace().is_none());
    step(&mut machine, 2);
    step(&mut other, 2);
    same_hardware(&mut machine, &mut other);
    assert_eq!(capture.trace.events.len(), 1);

    machine.reset_and_begin_audio_trace(2).unwrap();
    let restarted = machine.finish_audio_trace().unwrap();
    assert_eq!(restarted.trace.generation, capture.trace.generation + 1);
    assert_eq!(restarted.trace.end_cycle, 56);
    assert!(restarted.trace.events.is_empty());
    restarted.trace.validate_complete().unwrap();
}

#[test]
fn cloned_recorders_overflow_and_finish_independently() {
    for capacity in [1, 2] {
        let mut source = fixture(Region::Pal, true, &writes());
        source.reset_and_begin_audio_trace(capacity).unwrap();
        step(&mut source, 1);
        let mut other = source.clone();
        let prefix = source.finish_audio_trace().unwrap();
        step(&mut source, 2);
        step(&mut other, 2);
        let overflow = other.finish_audio_trace().unwrap();
        assert_eq!(prefix.trace.events.len(), 1);
        prefix.trace.validate_complete().unwrap();
        assert_eq!(overflow.trace.events.len(), capacity);
        assert_eq!(overflow.trace.dropped_events, (3 - capacity) as u64);
        assert_eq!(overflow.trace.events[0], prefix.trace.events[0]);
        assert!(overflow.trace.validate_complete().is_err());
        same_hardware(&mut source, &mut other);
        step(&mut source, 512);
        step(&mut other, 512);
        same_hardware(&mut source, &mut other);
    }
}

#[test]
fn reset_precheck_preserves_capture_and_success_invalidates() {
    let mut machine = fixture(Region::NtscJapan, true, &writes());
    machine.reset_and_begin_audio_trace(8).unwrap();
    step(&mut machine, 1);
    let now = machine.bus.now;
    machine.bus.now = MasterTicks::new(u64::MAX - 1);
    let before = machine.clone();
    assert_eq!(machine.reset_cpu(), Err(TimingFault::ClockOverflow));
    assert_eq!(machine, before);
    assert!(machine.bus.audio.trace.is_enabled());
    machine.bus.now = now;
    machine.reset_cpu().unwrap();
    invalidated(&mut machine, AudioTraceInvalidation::ExternalMutation);

    machine.reset_and_begin_audio_trace(8).unwrap();
    step(&mut machine, 2);
    machine.reset();
    assert_eq!(machine.bus.now.get(), 56);
    assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
    invalidated(&mut machine, AudioTraceInvalidation::Reset);
}

fn pending_psg() -> Emulator {
    let mut machine = fixture(Region::NtscUs, true, &[0x60fe, 0x4e71]);
    machine.reset_and_begin_audio_trace(8).unwrap();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..7].copy_from_slice(&[0x3e, 0x90, 0x32, 0x11, 0x7f, 0x18, 0xfe]);
    for _ in 0..1000 {
        let now = machine.bus.now.get();
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.psg_writes != Default::default()
                    && pending.deadline > now
                    && pending.deadline <= now + 28
            })
        {
            return machine;
        }
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .advance_machine(1)
        .unwrap();
    }
    panic!("actual Z80 PSG write did not reach its unpublished deadline");
}

#[test]
fn failed_reset_prime_rolls_back_a_sound_write_without_leaking_an_event() {
    let mut failed = pending_psg();
    assert_eq!(failed.psg_snapshot().volumes, [15; 4]);
    assert!(
        failed
            .clone()
            .finish_audio_trace()
            .unwrap()
            .trace
            .events
            .is_empty()
    );
    let mut success = failed.clone();
    success.reset_cpu().unwrap();
    assert_eq!(success.psg_snapshot().volumes, [0, 15, 15, 15]);
    invalidated(&mut success, AudioTraceInvalidation::ExternalMutation);

    // The first priming transfer publishes the pending write, then faults on decode.
    failed.bus.cartridge.initial.pc = 0x800000;
    let before = failed.clone();
    let saved = failed.encode_state().unwrap();
    assert!(matches!(
        failed.reset_cpu(),
        Err(TimingFault::Bus {
            source: BusError::UnsupportedAccess {
                address: 0x800000,
                ..
            },
            ..
        })
    ));
    assert_eq!(failed.encode_state().unwrap(), saved);
    assert_eq!(failed.psg_snapshot(), before.psg_snapshot());
    assert_eq!(failed.sound_schedule, before.sound_schedule);
    assert_eq!(failed.sound_cpu, before.sound_cpu);
    assert_eq!(failed.bus.now, before.bus.now);
    assert_eq!(failed.framebuffer(), before.framebuffer());
    let mut before = before;
    assert_eq!(failed.drain_audio_samples(), before.drain_audio_samples());
    let trace = failed.finish_audio_trace().unwrap().trace;
    assert_eq!(
        trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
    assert!(trace.events.is_empty());
    assert!(trace.validate_complete().is_err());
    failed.bus.cartridge.initial.pc = 0x100;
    before.bus.cartridge.initial.pc = 0x100;
    before.finish_audio_trace();
    step(&mut failed, 512);
    step(&mut before, 512);
    same_hardware(&mut failed, &mut before);
}

#[test]
fn rejected_restore_is_atomic_and_current_and_legacy_restore_invalidate() {
    for concurrent in [false, true] {
        let mut machine = fixture(Region::NtscUs, concurrent, &writes());
        machine.reset_and_begin_audio_trace(8).unwrap();
        step(&mut machine, 1);
        let saved = machine.encode_state().unwrap();
        step(&mut machine, 1);
        let before = machine.clone();
        let mut wrong_rom = saved.clone();
        wrong_rom[12] ^= 1;
        let mut trailing = saved.clone();
        trailing.push(0);
        for rejected in [
            &saved[..12],
            &saved[..saved.len() - 1],
            &wrong_rom,
            &trailing,
        ] {
            assert!(machine.decode_state(rejected).is_err());
            assert_eq!(machine, before);
        }
        let mut current = machine.clone();
        current.decode_state(&saved).unwrap();
        assert_eq!(current.encode_state().unwrap(), saved);
        let trace = current.finish_audio_trace().unwrap().trace;
        assert_eq!(trace.events.len(), 2);
        assert_eq!(
            trace.invalidated,
            Some(AudioTraceInvalidation::StateRestore)
        );
        assert!(trace.validate_complete().is_err());

        let mut legacy = saved[..saved.len() - 5].to_vec();
        legacy[8..12].copy_from_slice(&14_u32.to_le_bytes());
        machine.decode_state(&legacy).unwrap();
        invalidated(&mut machine, AudioTraceInvalidation::StateRestore);
        step(&mut current, 512);
        step(&mut machine, 512);
        same_hardware(&mut current, &mut machine);
    }
}

#[test]
fn actual_main_and_sound_fault_paths_invalidate_and_clear_origin() {
    let mut main = fixture(Region::NtscUs, false, &[0x1039, 0x0080, 0x0000, 0x60fe]);
    main.reset_and_begin_audio_trace(8).unwrap();
    assert!(matches!(
        main.step_instruction(),
        Err(MachineFault::Main(_))
    ));
    invalidated(&mut main, AudioTraceInvalidation::ExecutionFault);

    let mut overflow = fixture(Region::NtscUs, false, &[0x3080, 0x4e71]);
    overflow.reset_and_begin_audio_trace(8).unwrap();
    overflow.cpu.registers.a[0] = 0xff0000;
    overflow.bus.now = MasterTicks::new(u64::MAX - 1);
    assert!(matches!(
        overflow.step_instruction(),
        Err(MachineFault::Main(TimingFault::Bus {
            source: BusError::ClockOverflow,
            ..
        }))
    ));
    invalidated(&mut overflow, AudioTraceInvalidation::ClockOverflow);

    let mut sound = fixture(Region::NtscUs, false, &[0x4e72, 0x2700, 0x4e71, 0x4e71]);
    sound.reset_and_begin_audio_trace(8).unwrap();
    step(&mut sound, 2);
    sound.bus.sound.set_reset(false);
    sound.bus.sound.ram[..3].copy_from_slice(&[0x3a, 0, 0x50]);
    assert!(matches!(
        sound.step_sound_instruction(),
        Err(SoundFault::UnsupportedAccess {
            address: 0x5000,
            ..
        })
    ));
    let trace = sound.finish_audio_trace().unwrap().trace;
    assert_eq!(
        trace.invalidated,
        Some(AudioTraceInvalidation::ExecutionFault)
    );
    assert!(trace.events.is_empty());
}

#[test]
fn external_mutations_preserve_first_reason_and_rejected_mutations_preserve_capture() {
    let mut machine = fixture(Region::NtscUs, true, &writes());
    machine.reset_and_begin_audio_trace(8).unwrap();
    let before = machine.clone();
    assert!(!machine.set_interrupt_level(8));
    assert!(machine.queue_vdp_register(24, 0).is_err());
    assert!(machine.load_save_ram(&[]).is_err());
    assert_eq!(machine, before);
    assert!(machine.set_interrupt_level(1));
    machine.reset();
    invalidated(&mut machine, AudioTraceInvalidation::ExternalMutation);
    for mutate in [
        |machine: &mut Emulator| {
            machine.set_sound_interrupt(true);
        },
        |machine: &mut Emulator| {
            machine.reset_sound_cpu();
        },
        |machine: &mut Emulator| {
            machine.queue_vdp_register(0, 0).unwrap();
        },
    ] {
        machine.reset_and_begin_audio_trace(8).unwrap();
        mutate(&mut machine);
        invalidated(&mut machine, AudioTraceInvalidation::ExternalMutation);
    }
}

#[test]
fn save_ram_load_rejection_is_atomic_and_success_invalidates() {
    let base = fixture(Region::NtscUs, true, &writes());
    let mut rom = base.rom_bytes().to_vec();
    rom[0x1b0..0x1bc].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.reset_and_begin_audio_trace(8).unwrap();
    step(&mut machine, 1);
    let before = machine.clone();
    for size in [0, 511, 513] {
        assert!(machine.load_save_ram(&vec![0; size]).is_err());
        assert_eq!(machine, before);
    }
    machine.load_save_ram(&[0x55; 512]).unwrap();
    assert_eq!(machine.dump_save_ram().unwrap(), [0x55; 512]);
    invalidated(&mut machine, AudioTraceInvalidation::ExternalMutation);
}

#[test]
fn renderer_controls_and_controller_inputs_preserve_chip_write_observation() {
    let mut on = fixture(Region::Pal, true, &writes());
    let mut off = on.clone();
    on.reset_and_begin_audio_trace(8).unwrap();
    for machine in [&mut on, &mut off] {
        machine.set_sample_rate(44_100);
        machine.set_apu_sample_generation_enabled(false);
        machine.set_apu_channel_mutes([true; 4]);
        machine.set_fm_channel_mutes([true; 6]);
        machine
            .input_mut()
            .set_controller_kind(crate::ControllerPort::One, crate::ControllerKind::SixButton);
        machine
            .input_mut()
            .set_controller_raw(crate::ControllerPort::One, 0x7f);
        machine
            .input_mut()
            .set_controller_extra_raw(crate::ControllerPort::One, 0x0b);
        step(machine, 3);
    }
    let trace = on.finish_audio_trace().unwrap().trace;
    trace.validate_complete().unwrap();
    assert_eq!(trace.events.len(), 3);
    for (event, value) in trace.events.iter().zip([0x90, 0x9f, 0x91]) {
        assert!(matches!(event.write, MdTraceWrite::Psg { value: actual, .. } if actual == value));
    }
    same_hardware(&mut on, &mut off);
}
