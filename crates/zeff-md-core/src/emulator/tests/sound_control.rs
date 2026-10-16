use super::*;
use crate::constants::{SOUND_BUS_REQUEST, SOUND_RESET};
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address + u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn pending_store() -> Emulator {
    let mut machine = machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x155;
    machine.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 1, 0x76]);
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
    machine.sound_schedule.pending = Some(pending);
    machine
}

fn write(machine: &mut Emulator, access: BusAccess, value: u16) -> Result<(), BusError> {
    let clock = machine.bus.cpu_clock;
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .write_cycle(access, value, clock);
    assert_eq!(result.wait_clocks, 0);
    result.result
}

fn read(machine: &mut Emulator, access: BusAccess) -> Result<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    let result = ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .read_cycle(access, clock);
    assert_eq!(result.wait_clocks, 0);
    result.result
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

#[test]
fn control_alias_deadline_order_and_pending_action() {
    for offset in (0..=0xfe).step_by(2) {
        for base in [SOUND_BUS_REQUEST, SOUND_RESET] {
            for tied in [false, true] {
                let mut machine = pending_store();
                machine.bus.now = MasterTicks::new(300 - 28 - u64::from(!tied));
                let cpu = machine.sound_cpu.clone();
                let value = if base == SOUND_BUS_REQUEST { 0x100 } else { 0 };
                write(&mut machine, access(base + offset, ByteLanes::Both), value).unwrap();
                assert_eq!(machine.sound_bank(), 0x155);
                assert_eq!(machine.bus.now.get(), 300 - u64::from(!tied));
                assert_eq!(machine.peek(0xa00100), Some(if tied { 0x5a } else { 0 }));
                if base == SOUND_RESET {
                    assert!(machine.sound_reset_asserted());
                    assert_eq!(machine.sound_cpu.cycles(), 0);
                    assert!(machine.sound_schedule.pending.is_none());
                    advance(&mut machine, 1);
                    assert_eq!(machine.peek(0xa00100), Some(if tied { 0x5a } else { 0 }));
                } else {
                    assert!(machine.sound_bus_requested());
                    assert_eq!(machine.sound_bus_granted(), tied);
                    if !tied {
                        assert_eq!(machine.sound_cpu, cpu);
                        assert!(machine.sound_deadline().is_some());
                        advance(&mut machine, 1);
                    }
                    assert!(machine.sound_bus_granted());
                    assert_eq!(machine.peek(0xa00100), Some(0x5a));
                    assert_eq!(machine.sound_cpu.cycles(), 20);
                }
            }
        }
    }
}

#[test]
fn control_alias_lower_lane_and_read_guards() {
    for offset in (0..=0xfe).step_by(2) {
        for base in [SOUND_BUS_REQUEST, SOUND_RESET] {
            let mut machine = pending_store();
            let cpu = machine.sound_cpu.clone();
            let memory = machine.bus.sound.clone();
            assert_eq!(
                write(&mut machine, access(base + offset, ByteLanes::Lower), 1),
                Err(BusError::UnsupportedAccess {
                    address: base + offset,
                    direction: Direction::Write
                })
            );
            assert_eq!(machine.sound_cpu, cpu);
            assert_eq!(machine.bus.sound, memory);
            assert!(machine.sound_deadline().is_some());
            assert_eq!(
                read(&mut machine, access(base + offset, ByteLanes::Lower)),
                Err(BusError::UnsupportedAccess {
                    address: base + offset,
                    direction: Direction::Read
                })
            );
            if base == SOUND_RESET {
                for lanes in [ByteLanes::Upper, ByteLanes::Both] {
                    assert_eq!(
                        read(&mut machine, access(base + offset, lanes)),
                        Err(BusError::UnsupportedAccess {
                            address: base + offset,
                            direction: Direction::Read
                        })
                    );
                }
            }
            assert_eq!(machine.sound_cpu, cpu);
            assert_eq!(machine.bus.sound, memory);
        }
    }
    for address in [
        0xa12100, 0xa121fe, 0xa13100, 0xa131fe, 0xb11100, 0xb111fe, 0xb11200, 0xb112fe,
    ] {
        let mut machine = pending_store();
        assert_eq!(
            write(&mut machine, access(address, ByteLanes::Both), 0x100),
            Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Write
            })
        );
        assert_eq!(
            read(&mut machine, access(address, ByteLanes::Both)),
            Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Read
            })
        );
        assert!(!machine.sound_bus_requested() && !machine.sound_reset_asserted());
    }
}

#[test]
fn control_word_data_uses_only_upper_bit_zero() {
    for byte in 0..=255u8 {
        for base in [SOUND_BUS_REQUEST, SOUND_RESET] {
            let mut machine = pending_store();
            let value = u16::from_be_bytes([byte, !byte]);
            write(&mut machine, access(base + 0xfe, ByteLanes::Both), value).unwrap();
            if base == SOUND_BUS_REQUEST {
                assert_eq!(machine.sound_bus_requested(), byte & 1 != 0);
                assert!(!machine.sound_reset_asserted());
            } else {
                assert_eq!(machine.sound_reset_asserted(), byte & 1 == 0);
                assert!(!machine.sound_bus_requested());
            }
            assert_eq!(machine.sound_bank(), 0x155);
            assert_eq!(machine.peek(0xa00100), Some(0));
        }
    }
}

#[test]
fn control_alias_overflow_has_no_control_effect() {
    for offset in (0..=0xfe).step_by(2) {
        for base in [SOUND_BUS_REQUEST, SOUND_RESET] {
            for lanes in [ByteLanes::Upper, ByteLanes::Both] {
                let mut machine = pending_store();
                machine.bus.now = MasterTicks::new(u64::MAX - 1);
                let saved = machine.clone();
                assert_eq!(
                    write(&mut machine, access(base + offset, lanes), 0x100),
                    Err(BusError::ClockOverflow)
                );
                assert_eq!(machine, saved);
            }
        }
    }
}
