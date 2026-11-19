use super::*;
use crate::constants::{SOUND_CPU_DIVISOR, VDP_FRAME_HEIGHT, VDP_LINE_TICKS, VDP_MAX_FRAME_HEIGHT};

mod bus_paths;

const COUNTER: usize = 0x700;

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
    assert_eq!(machine.sound_fault(), None);
}

fn machine(region: Region, tall: bool) -> Emulator {
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    rom[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    if tall {
        machine.queue_vdp_register(1, 0x0c).unwrap();
        advance(&mut machine, 20);
    }
    let program = [0x31, 0xfd, 0x1f, 0xed, 0x56, 0xfb, 0x76, 0x18, 0xfd];
    machine.bus.sound.ram[..program.len()].copy_from_slice(&program);
    let handler = [
        0x3a, 0x00, 0x07, 0x3c, 0x32, 0x00, 0x07, 0x06, 0x40, 0x10, 0xfe, 0xfb, 0xed, 0x4d,
    ];
    machine.bus.sound.ram[0x38..0x38 + handler.len()].copy_from_slice(&handler);
    machine.bus.sound.set_reset(false);
    machine
}

fn pulse(machine: &Emulator, tall: bool) -> u64 {
    let view = machine
        .vdp()
        .interrupts()
        .sound_interrupt(machine.vdp().registers(), machine.region());
    let line = if tall {
        VDP_MAX_FRAME_HEIGHT
    } else {
        VDP_FRAME_HEIGHT
    } as u64
        * VDP_LINE_TICKS;
    line + (0..VDP_LINE_TICKS)
        .find(|offset| view.asserted_at(line + offset).unwrap())
        .unwrap()
}

#[test]
fn automatic_sound_irq_is_sampled_per_cursor_and_not_per_interval_endpoint() {
    for (region, tall) in [
        (Region::NtscUs, false),
        (Region::Pal, false),
        (Region::Pal, true),
    ] {
        let mut source = machine(region, tall);
        let mut partitioned = source.clone();
        let ticks = 3 * region.frame_ticks();
        advance(&mut source, ticks);
        let mut remaining = ticks;
        while remaining != 0 {
            let part = remaining.min(1171);
            advance(&mut partitioned, part);
            remaining -= part;
        }
        assert_eq!(source.bus.sound.ram[COUNTER], 3);
        assert!(!source.bus.sound.irq_asserted);
        assert_eq!(source, partitioned);
    }
}

#[test]
fn masked_reset_and_requested_sound_cpus_miss_the_completed_pulse() {
    for hold in 0..3 {
        let mut source = machine(Region::NtscUs, false);
        match hold {
            0 => source.bus.sound.ram[5] = 0xf3,
            1 => source.bus.sound.set_reset(true),
            2 => source.bus.sound.request(true),
            _ => unreachable!(),
        }
        let ticks = source.region().frame_ticks();
        advance(&mut source, ticks);
        assert_eq!(source.bus.sound.ram[COUNTER], 0);
        source.bus.sound.ram[5] = 0xfb;
        source.bus.sound.set_reset(false);
        source.bus.sound.request(false);
        source.reset_sound_cpu();
        let before_next = ticks + pulse(&source, false) - source.bus.now.get() - 1;
        advance(&mut source, before_next);
        assert_eq!(source.bus.sound.ram[COUNTER], 0);
        advance(&mut source, 1000 * SOUND_CPU_DIVISOR);
        assert_eq!(source.bus.sound.ram[COUNTER], 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn manual_sound_irq_remains_an_independent_asserted_input() {
    warm_reset_public_entry();
    let mut source = machine(Region::NtscUs, false);
    source.set_sound_interrupt(true);
    advance(&mut source, 200 * SOUND_CPU_DIVISOR);
    assert_eq!(source.bus.sound.ram[COUNTER], 1);
    assert!(source.bus.sound.irq_asserted);
    source.set_sound_interrupt(false);
    let end = source.region().frame_ticks();
    let ticks = end - source.bus.now.get();
    advance(&mut source, ticks);
    assert_eq!(source.bus.sound.ram[COUNTER], 2);
    assert!(!source.bus.sound.irq_asserted);
}

fn warm_fixture(concurrent: bool, origin: u8, target: u16, phase: u8) -> Emulator {
    use crate::hardware::sound::schedule::PendingSound;
    use std::sync::Arc;
    let mut source = machine(Region::NtscUs, false);
    source.concurrent_sound = false;
    source.bus.set_sound_reset(true);
    source.sound_schedule.reset_cpu(
        &mut source.sound_cpu,
        &mut source.bus.sound,
        &mut crate::sound_trace::NoSoundCapture,
    );
    Arc::make_mut(&mut source.bus.cartridge.bytes)[..8]
        .copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0xff, 2, 0]);
    source.bus.ram[0x200..0x214].copy_from_slice(&[
        0x52, 0x79, 0, 0xff, 3, 0, 0x13, 0xfc, 0, 1, 0, 0xa1, 0x12, 0, 0x4e, 0x72, 0x27, 0, 0x4e,
        0x71,
    ]);
    source.cpu.registers.d[2] = 0x12345678;
    source.cpu.registers.a[3] = 0x11223344;
    source.cpu.registers.usp = 0xff4000;
    source.cpu.registers.set_status_register(0x2015);
    if origin == 1 {
        source.cpu = zeff_m68k::timing::PrefetchCpu::from_prefetched(
            source.registers(),
            [0x4e72, 0x2015],
            source.cpu_clock(),
        )
        .unwrap();
        source.step_instruction().unwrap();
        source.step_instruction().unwrap();
        assert!(source.cpu.stopped() && source.main_cpu_stopped());
    } else if origin == 2 {
        source.cpu.registers.set_status_register(0xa015);
        source.step_instruction().unwrap();
    }
    source.bus.sound.set_reset(false);
    let [high, low] = target.to_be_bytes();
    source.bus.sound.ram[..8].copy_from_slice(&[0x3e, 0x5a, 0x21, low, high, 0x77, 0x18, 0xfe]);
    source.main_stopped = true;
    source.step_sound_instruction().unwrap();
    source.step_sound_instruction().unwrap();
    source.main_stopped = source.cpu.stopped();
    source.bus.sound.ram[0x20] = 0x21;
    source.bus.sound.ram[0x21] = 0x99;
    let now = source.bus.now.get();
    for value in [0x84, 1, 0x90] {
        source.bus.audio.write_psg(now, value);
    }
    for (port, value) in [(0, 0x2a), (1, 0xd3), (0, 0x2b), (1, 0x80)] {
        source.bus.audio.write_opn2(now, port, value);
    }
    source
        .bus
        .input
        .set_controller_raw(crate::ControllerPort::One, 0xfe);
    if phase == 1 {
        let at = VDP_FRAME_HEIGHT as u64 * VDP_LINE_TICKS - 140;
        source
            .bus
            .advance_machine(at - source.bus.now.get())
            .unwrap();
    } else if phase == 2 {
        for (index, value) in [
            (1, 0x14),
            (15, 2),
            (19, 64),
            (20, 0),
            (21, 0),
            (22, 0),
            (23, 0),
        ] {
            source.bus.advance_machine(1).unwrap();
            let write = source
                .bus
                .vdp
                .queue_register(index, value, source.region())
                .unwrap();
            source
                .bus
                .advance_machine(write.publish_at.get() - source.bus.now.get())
                .unwrap();
        }
        source
            .bus
            .vdp
            .write_control(0x4100, source.region())
            .unwrap();
        source
            .bus
            .vdp
            .write_control(0x0080, source.region())
            .unwrap();
    }
    source.sound_schedule.pending = Some(
        PendingSound::prepare(
            &source.sound_cpu,
            &source.bus.sound,
            &source.bus.cartridge,
            source.bus.now.get(),
        )
        .unwrap(),
    );
    if origin == 2 {
        source.bus.irq_level = 7;
    }
    source.concurrent_sound = concurrent;
    source
}

fn warm_control(source: &mut Emulator) -> u64 {
    use zeff_emu_common::audio_trace::AudioTraceInvalidation;
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::{ClockedBus, CpuClocks};
    fn apply(bus: &mut impl ClockedBus<Error = BusError>, mut clock: CpuClocks) -> CpuClocks {
        bus.idle(clock, 14);
        clock.0 += 14;
        for (index, address) in [0, 2, 4, 6, 0xff0200, 0xff0202].into_iter().enumerate() {
            if index == 5 {
                bus.idle(clock, 2);
                clock.0 += 2;
            }
            let response = bus.read_cycle(
                BusAccess {
                    logical_address: address,
                    bus_address: address,
                    function_code: FunctionCode::SupervisorProgram,
                    kind: if index < 4 {
                        AccessKind::ResetVector
                    } else {
                        AccessKind::Prefetch
                    },
                    lanes: ByteLanes::Both,
                },
                clock,
            );
            assert_eq!(
                response.result,
                Ok([0xff, 0x8000, 0xff, 0x0200, 0x5279, 0xff][index])
            );
            clock.0 += 4 + u64::from(response.wait_clocks);
        }
        clock
    }
    let mut registers = source.registers();
    let before = source.cpu_clock();
    source.bus.input.retire_uart_reset();
    if !source.bus.sound.reset_asserted {
        source.bus.audio.reset_opn2(source.bus.now.get());
        source.bus.sound.set_reset(true);
        source.sound_schedule.reset_cpu(
            &mut source.sound_cpu,
            &mut source.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
    }
    source
        .bus
        .audio
        .invalidate_trace(AudioTraceInvalidation::ExternalMutation);
    let clock = if source.concurrent_sound {
        source.bus.audio.begin_batch();
        let clock = apply(
            &mut ScheduledBus::new(
                &mut source.bus,
                &mut source.sound_cpu,
                &mut source.sound_schedule,
            ),
            before,
        );
        source.bus.audio.finish_batch();
        clock
    } else {
        apply(&mut source.bus, before)
    };
    registers.pc = 0xff0200;
    registers.ssp = 0xff8000;
    registers.set_status_register(0x2700 | (registers.status_register() & 31));
    let mut writer = StateWriter::new();
    writer.write_u8(1);
    writer.write_section(|writer| {
        for value in registers.d.into_iter().chain(registers.a) {
            writer.write_u32(value);
        }
        for value in [registers.usp, registers.ssp, registers.pc] {
            writer.write_u32(value);
        }
        writer.write_u16(registers.status_register());
        writer.write_u64(clock.0);
        writer.write_bool(true);
        writer.write_u32(registers.pc);
        writer.write_u16(0x5279);
        writer.write_u16(0xff);
        writer.write_bool(false);
        writer.write_bool(false);
        writer.write_u8(source.bus.interrupt_level());
        writer.write_bool(false);
    });
    source
        .cpu
        .read_boundary_state(&mut StateReader::new(&writer.into_bytes()))
        .unwrap();
    source.main_stopped = false;
    source.bus.audio.trace_origin = None;
    clock.0 - before.0
}

fn warm_reset_public_entry() {
    for concurrent in [false, true] {
        for (origin, target, phase) in [
            (0, 0x20, 0),
            (1, 0x20, 0),
            (2, 0x20, 0),
            (0, 0x6000, 0),
            (0, 0x7f11, 0),
            (0, 0x4000, 0),
            (1, 0x20, 1),
            (0, 0x20, 2),
        ] {
            let mut source = warm_fixture(concurrent, origin, target, phase);
            assert_eq!(source.check_warm_reset(), Ok(()));
            let original = source.registers();
            let ram = source.work_ram().to_vec();
            let sound_ram = source.bus.sound.ram.clone();
            let tick = source.bus.now.get();
            let frame = source.frame_count();
            let mut control = source.clone();
            let clocks = warm_control(&mut control);
            assert_eq!(
                source.warm_reset(),
                Ok(WarmResetReport {
                    elapsed_cpu_clocks: clocks,
                    elapsed_master_ticks: clocks * 7,
                })
            );
            assert!(
                source == control,
                "public reset/control mismatch mode={concurrent} origin={origin} target={target:04x} phase={phase}; cpu={} bus={} sound_cpu={} schedule={}",
                source.cpu == control.cpu,
                source.bus == control.bus,
                source.sound_cpu == control.sound_cpu,
                source.sound_schedule == control.sound_schedule
            );
            assert_eq!(source.bus.now.get(), tick + clocks * 7);
            assert_eq!(source.registers().d, original.d);
            assert_eq!(source.registers().a, original.a);
            assert_eq!(source.registers().usp, original.usp);
            assert_eq!(source.work_ram(), ram);
            assert_eq!(source.bus.sound.ram, sound_ram);
            assert_eq!(&source.bus.sound.ram[0x20..0x22], &[0x21, 0x99]);
            assert_eq!(source.sound_cpu, zeff_z80::Cpu::new());
            assert!(source.sound_schedule.pending.is_none());
            assert!(source.sound_reset_asserted());
            assert!(!source.main_cpu_stopped());
            if phase == 1 {
                assert!(source.frame_count() > frame);
            }
            if phase == 2 {
                assert!(clocks > 40);
            } else {
                assert_eq!(clocks, 40);
            }
            if origin == 0 && target == 0x20 && phase == 0 {
                let mut spuriously_reset = source.bus.audio.clone();
                spuriously_reset.reset_opn2(source.bus.now.get());
                assert_ne!(source.bus.audio, spuriously_reset);
                let clocks = warm_control(&mut control);
                assert_eq!(source.warm_reset().unwrap().elapsed_cpu_clocks, clocks);
                assert!(
                    source == control,
                    "already-held reset changed preserved state"
                );
            }
            let state = source.encode_frontend_state().unwrap();
            let mut restored = source.clone();
            restored.decode_frontend_state(&state).unwrap();
            for machine in [&mut source, &mut control, &mut restored] {
                machine.bus.irq_level = 0;
                machine.step_instruction().unwrap();
                assert_eq!(&machine.work_ram()[0x300..0x302], &[0, 1]);
                machine.step_instruction().unwrap();
                machine.step_field().unwrap();
                assert!(!machine.sound_reset_asserted());
                if !concurrent {
                    machine.step_sound_instruction().unwrap();
                }
                assert!(machine.sound_cpu.cycles() > 0);
                assert!(
                    machine
                        .drain_audio_samples()
                        .iter()
                        .any(|sample| sample.abs() > 0.000001)
                );
            }
            assert!(
                source == control,
                "public reset/control mismatch mode={concurrent} origin={origin} target={target:04x} phase={phase}; cpu={} bus={} sound_cpu={} schedule={}",
                source.cpu == control.cpu,
                source.bus == control.bus,
                source.sound_cpu == control.sound_cpu,
                source.sound_schedule == control.sound_schedule
            );
            assert!(
                source == restored,
                "frontend reset future mismatch mode={concurrent} origin={origin} target={target:04x} phase={phase}"
            );
        }
    }
    warm_reset_rejection_and_failure();
}

fn warm_reset_rejection_and_failure() {
    use std::sync::Arc;
    for case in 0..16 {
        let mut source = warm_fixture(true, 0, 0x20, 0);
        match case {
            0 => source.bus.sound.bank = 1,
            1 => source.bus.sound.request(true),
            2 => source.bus.sound.granted = true,
            3 => source.bus.sound.irq_asserted = true,
            4 => source.sound_schedule.begin_epoch_field(),
            5 => {
                source.sound_schedule.rmw_checkpoint =
                    source.sound_schedule.pending.clone().map(Box::new)
            }
            6 => source.sound_schedule.fault = Some(crate::SoundFault::ClockOverflow),
            7 => {
                let pending = source.sound_schedule.pending.as_mut().unwrap();
                pending.deadline = source.bus.now.get();
                pending.start =
                    pending.deadline - u64::from(pending.instruction.cycles) * SOUND_CPU_DIVISOR;
            }
            8 => source.sound_schedule.pending.as_mut().unwrap().start = source.bus.now.get() + 1,
            9 => source.bus.audio.begin_batch(),
            10 => {
                source
                    .sound_schedule
                    .pending
                    .as_mut()
                    .unwrap()
                    .instruction
                    .cycles += 1
            }
            11 => source.sound_cpu.suspend(),
            12 => {
                let mut image = source.rom_bytes().to_vec();
                image[4..8].copy_from_slice(&0x100u32.to_be_bytes());
                image[0x1b0..0x1bc]
                    .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
                source.bus.cartridge = crate::cartridge::Cartridge::new(image).unwrap();
            }
            13 => source.bus.sound.set_reset(true),
            14 => {
                source.sound_schedule.pending.as_mut().unwrap().deadline = source.bus.now.get() - 15
            }
            15 => source.bus.clock_fault = true,
            _ => unreachable!(),
        }
        let before = source.clone();
        let reason = source.check_warm_reset().unwrap_err();
        assert!(
            source == before,
            "reset rejection/failure mutated preserved state"
        );
        assert_eq!(source.warm_reset(), Err(WarmResetError::Rejected(reason)));
        assert!(
            source == before,
            "reset rejection/failure mutated preserved state"
        );
    }
    for concurrent in [false, true] {
        let mut source = warm_fixture(concurrent, 1, 0x20, 0);
        Arc::make_mut(&mut source.bus.cartridge.bytes)[4..8]
            .copy_from_slice(&0x400000u32.to_be_bytes());
        let tick = source.bus.now.get();
        assert!(matches!(
            source.warm_reset(),
            Err(WarmResetError::Failed(_))
        ));
        assert_eq!(source.bus.now.get(), tick + 238);
        assert!(source.bus.clock_fault && source.sound_reset_asserted());
        assert_eq!(&source.bus.sound.ram[0x20..0x22], &[0x21, 0x99]);
        assert!(source.encode_state().is_err());
        let before = source.clone();
        assert!(matches!(
            source.warm_reset(),
            Err(WarmResetError::Rejected(_))
        ));
        assert!(
            source == before,
            "reset rejection/failure mutated preserved state"
        );
        assert!(source.step_instruction().is_err());
        assert_eq!(source.cpu_clock(), before.cpu_clock());
        assert_eq!(source.bus.now, before.bus.now);
        assert_eq!(source.work_ram(), before.work_ram());
    }
}

#[test]
fn state_inside_automatic_pulse_preserves_pending_execution_and_future_audio() {
    for (region, tall) in [(Region::NtscUs, false), (Region::Pal, true)] {
        let mut source = machine(region, tall);
        let at = pulse(&source, tall) + 2 * SOUND_CPU_DIVISOR;
        let ticks = at - source.bus.now.get();
        advance(&mut source, ticks);
        assert!(
            source
                .vdp()
                .interrupts()
                .sound_interrupt(source.vdp().registers(), source.region())
                .asserted_at(source.bus.now.get())
                .unwrap()
        );
        assert!(source.sound_schedule.pending.is_some());
        let saved = source.encode_state().unwrap();
        let mut target = source.clone();
        advance(&mut target, region.frame_ticks());
        target.decode_state(&saved).unwrap();
        source.set_sample_rate(source.sample_rate());
        for ticks in [
            1,
            VDP_LINE_TICKS,
            region.frame_ticks(),
            region.frame_ticks(),
        ] {
            advance(&mut source, ticks);
            advance(&mut target, ticks);
            assert_eq!(target, source);
        }
        assert_eq!(source.bus.sound.ram[COUNTER], 3);
        assert_eq!(target.drain_audio_samples(), source.drain_audio_samples());
    }
}
