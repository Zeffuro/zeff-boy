use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::schedule::SoundInterval;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn read(machine: &mut Emulator) -> u32 {
    let access = BusAccess {
        logical_address: 0x100,
        bus_address: 0x100,
        lanes: ByteLanes::Both,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
    };
    let clock = machine.bus.cpu_clock;
    let report = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .read_cycle(access, clock);
    assert!(report.result.is_ok());
    assert!(report.wait_clocks > 0);
    assert_eq!(machine.sound_fault(), None);
    report.wait_clocks
}

fn register(machine: &mut Emulator, index: u8, value: u8) {
    advance(machine, 1);
    let write = machine
        .bus
        .vdp
        .queue_register(index, value, machine.region())
        .unwrap();
    let ticks = write.publish_at.get() - machine.bus.now.get();
    advance(machine, ticks);
}

#[test]
fn external_dma_wait_preserves_sound_pulse_that_ends_before_the_wait() {
    let mut source = machine(Region::NtscUs, false);
    for (index, value) in [
        (1, 0x14),
        (15, 2),
        (19, 0),
        (20, 2),
        (21, 0),
        (22, 0xc0),
        (23, 0x7f),
    ] {
        register(&mut source, index, value);
    }
    let at = pulse(&source, false) - 1000;
    let ticks = at - source.bus.now.get();
    advance(&mut source, ticks);
    source
        .bus
        .vdp
        .write_control(0x4400, source.region())
        .unwrap();
    source.bus.vdp.write_control(0x80, source.region()).unwrap();
    let mut expected = source.clone();
    let begin = source.bus.now.get();
    read(&mut source);
    assert!(source.bus.now.get() > pulse(&source, false) + VDP_LINE_TICKS);
    let ticks = source.bus.now.get() - begin;
    advance(&mut expected, ticks);
    assert_eq!(source.bus.sound.ram[COUNTER], 1);
    assert_eq!(source.sound_cpu, expected.sound_cpu);
    assert_eq!(source.sound_schedule, expected.sound_schedule);
    assert_eq!(source.bus.sound, expected.bus.sound);
    assert_eq!(source.bus.audio, expected.bus.audio);
}

#[test]
fn bank_reservation_delay_samples_irq_before_following_main_transfer() {
    let mut source = machine(Region::NtscUs, false);
    source.bus.sound.ram[6..12].copy_from_slice(&[0x3a, 0x00, 0x80, 0xc3, 0x06, 0x00]);
    let on = pulse(&source, false);
    let ticks = on - 1000 - source.bus.now.get();
    advance(&mut source, ticks);
    for _ in 0..500 {
        if source
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|p| p.reservation().is_some())
        {
            break;
        }
        advance(&mut source, 1);
    }
    let pending = source.sound_schedule.pending.as_mut().unwrap();
    assert!(pending.reservation().is_some());
    let waits = (on + 30 - pending.deadline).div_ceil(SOUND_CPU_DIVISOR);
    pending.extend_bus_segment(waits).unwrap();
    let release = pending.deadline;
    let begin = source.bus.now.get();
    let irq = source
        .vdp()
        .interrupts()
        .sound_interrupt(*source.vdp().registers(), source.region());
    let mut expected = source.clone();
    read(&mut source);
    let end = source.bus.now.get();
    let mut interval = SoundInterval::new(begin..end, &mut expected.bus.audio);
    let reservation = [Reservation {
        requester: Requester::MAIN,
        ticks: release..end,
    }];
    interval.occupied = &reservation;
    interval.sound_irq = Some(irq);
    expected.sound_schedule.advance(
        &mut expected.sound_cpu,
        &mut expected.bus.sound,
        &expected.bus.cartridge,
        &mut expected.bus.ram,
        interval,
    );
    assert_eq!(source.sound_cpu, expected.sound_cpu);
    assert_eq!(source.sound_schedule, expected.sound_schedule);
    assert_eq!(source.bus.sound, expected.bus.sound);
    assert_eq!(source.bus.audio, expected.bus.audio);
}
