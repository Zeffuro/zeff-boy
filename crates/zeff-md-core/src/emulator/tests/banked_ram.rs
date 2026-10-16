use super::*;
use crate::constants::{SOUND_CPU_DIVISOR, WORK_RAM_SIZE};
use crate::hardware::sound::CandidateBus;
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Z80Bus;

fn ready(sound: &[u8]) -> Emulator {
    let mut machine = machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..sound.len()].copy_from_slice(sound);
    machine.bus.ram.fill(0xde);
    machine
}

fn pending_write() -> Emulator {
    let mut machine = ready(&[0x3e, 0x55, 0x32, 4, 0x80, 0x76]);
    let first = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        0,
    )
    .unwrap();
    machine.bus.now = MasterTicks::new(first.deadline);
    first.publish(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.ram,
        &mut machine.bus.audio,
    );
    let pending = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        machine.bus.now.get(),
    )
    .unwrap();
    assert_eq!(pending.deadline, 300);
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    machine.sound_schedule.pending = Some(pending);
    machine
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(ticks)
    .unwrap();
}

fn access() -> BusAccess {
    BusAccess {
        logical_address: 0xff8004,
        bus_address: 0xff8004,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Upper,
    }
}

#[test]
fn banked_capacity_is_checked_and_reads_never_forward() {
    let machine = ready(&[]);
    let mut snapshot = machine.bus.sound.clone();
    let mut candidate = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    candidate.cpu_write(0x8004, 0x55);
    assert_eq!(candidate.cpu_read(0x8004), 0xff);
    candidate.cpu_write(0x8004, 0xa6);
    assert_eq!(candidate.cpu_read(0x8004), 0xff);
    assert_eq!(candidate.fault.get(), None);
    candidate.memory.bank = 0x1c0;
    candidate.cpu_write(0x8005, 0x77);
    assert_eq!(
        candidate.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            address: 0x8005,
            direction: zeff_m68k::bus::Direction::Write
        })
    );
    let mut ram = vec![0xde; WORK_RAM_SIZE];
    candidate.writes.publish(&mut ram);
    assert_eq!(ram[0x8004], 0xa6);
    assert_eq!(ram[0x8005], 0xde);
    assert_eq!(ram[4], 0xde);
    assert_eq!(machine.peek(0xff8004), Some(0xde));
}

#[test]
fn banked_pending_clone_reset_and_disjoint_write() {
    let mut machine = pending_write();
    let deadline = machine.sound_deadline().unwrap().get();
    machine.bus.now = MasterTicks::new(deadline - 29);
    let clock = machine.bus.cpu_clock;
    let mut neighbor = access();
    neighbor.lanes = ByteLanes::Lower;
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .write_cycle(neighbor, 0x00a6, clock);
    assert_eq!(result.result, Ok(()));
    assert_eq!(result.wait_clocks, 0);
    assert_eq!(machine.bus.now.get(), deadline - 1);
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    assert_eq!(machine.peek(0xff8005), Some(0xa6));
    let mut copied = machine.clone();
    let mut reset = machine.clone();
    reset.reset_sound_cpu();
    advance(&mut reset, 1);
    assert_eq!(reset.peek(0xff8004), Some(0xde));
    assert_eq!(reset.peek(0xff8005), Some(0xa6));
    advance(&mut machine, 1);
    advance(&mut copied, 1);
    assert_eq!(machine, copied);
    assert_eq!(machine.peek(0xff8004), Some(0x55));
    assert_eq!(machine.peek(0xff8005), Some(0xa6));
    machine.bus.ram[0x8004] = 0x33;
    assert_eq!(copied.peek(0xff8004), Some(0x55));
}

#[test]
fn banked_sound_publication_precedes_tied_main_transfer() {
    for tied in [false, true] {
        let mut machine = pending_write();
        machine.bus.now = MasterTicks::new(300 - 28 - u64::from(!tied));
        let clock = machine.bus.cpu_clock;
        let result = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access(), 0xa600, clock);
        assert_eq!(result.result, Ok(()));
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(machine.peek(0xff8004), Some(0xa6));
        if !tied {
            assert!(machine.sound_deadline().is_some());
            advance(&mut machine, 1);
            assert_eq!(machine.peek(0xff8004), Some(0x55));
        } else {
            assert!(machine.sound_deadline().is_none());
        }
    }
    let mut machine = pending_write();
    machine.bus.now = MasterTicks::new(300 - 28);
    let clock = machine.bus.cpu_clock;
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .read_cycle(access(), clock);
    assert_eq!(result.result, Ok(0x55de));
    assert_eq!(result.wait_clocks, 0);
}

#[test]
fn banked_overflow_discards_only_unpublished_writes() {
    let machine = pending_write();
    assert_eq!(
        PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            u64::MAX - 14
        ),
        Err(SoundFault::ClockOverflow)
    );
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    let mut machine = ready(&[0x21, 0x34, 0x12, 0x22, 0, 0x80]);
    let deadline = (machine.bus.now.get() / SOUND_CPU_DIVISOR + 23) * SOUND_CPU_DIVISOR;
    let remaining = deadline - machine.bus.now.get();
    advance(&mut machine, remaining);
    assert_eq!(machine.sound_cpu.cycles(), 23);
    assert!(machine.sound_schedule.pending.is_none());
    assert!(machine.sound_instruction_pending());
    assert_eq!(machine.peek(0xff8000), Some(0x34));
    assert_eq!(machine.peek(0xff8001), Some(0xde));
    let cpu = machine.sound_cpu.clone();
    machine.bus.now = MasterTicks::new(u64::MAX - 28);
    advance(&mut machine, 7);
    assert_eq!(machine.sound_fault(), Some(SoundFault::ClockOverflow));
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.peek(0xff8000), Some(0x34));
    assert_eq!(machine.peek(0xff8001), Some(0xde));
}
