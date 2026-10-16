use super::*;
use crate::constants::{SOUND_CPU_DIVISOR, VDP_FRAME_HEIGHT, VDP_LINE_TICKS, VDP_MAX_FRAME_HEIGHT};

mod bus_paths;

const COUNTER: usize = 0x700;

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
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
        .sound_interrupt(*machine.vdp().registers(), machine.region());
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

#[test]
fn manual_sound_irq_remains_an_independent_asserted_input() {
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
                .sound_interrupt(*source.vdp().registers(), source.region())
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
