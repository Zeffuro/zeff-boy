use super::*;
use crate::constants::{TRANSFER_CLOCKS, VDP_DECODE_MASK, VDP_DECODE_VALUE};
use crate::hardware::vdp::{ControlCommand, VdpPortError};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

type Write = CycleResult<(), BusError>;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn write(machine: &mut Emulator, access: BusAccess, value: u16) -> Write {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access, value, clock)
    } else {
        machine.bus.write_cycle(access, value, clock)
    }
}

fn ready(concurrent: bool) -> Emulator {
    let mut machine = super::machine(&[0x4e71; 4]);
    machine.concurrent_sound = concurrent;
    machine
}

fn rewind(machine: &mut Emulator) {
    machine.bus.now = MasterTicks::new(PRIME_CLOCKS * MAIN_CPU_DIVISOR);
    machine.bus.audio = crate::hardware::audio::Audio::default();
    machine.bus.audio.advance_to(machine.bus.now.get());
    machine.bus.cpu_clock = CpuClocks(PRIME_CLOCKS);
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine
        .bus
        .vdp
        .advance_to(machine.bus.now, Region::NtscUs)
        .unwrap();
}

pub fn mirror_and_lane_writes_with(
    mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write,
) {
    let mut free = [0; 10];
    let mut count = 0;
    for bit in 5..24 {
        if VDP_DECODE_MASK & (1 << bit) == 0 {
            free[count] = bit;
            count += 1;
        }
    }
    assert_eq!(count, free.len());
    for concurrent in [false, true] {
        let mut machine = ready(concurrent);
        for mirror in 0..(1 << free.len()) {
            let mut base = VDP_DECODE_VALUE;
            for (index, bit) in free.iter().enumerate() {
                base |= ((mirror >> index) & 1) << bit;
            }
            for offset in [4, 6] {
                for (lanes, value, expected) in [
                    (ByteLanes::Both, 0x8f02, 2),
                    (ByteLanes::Upper, 0x8f01, 0x8f),
                    (ByteLanes::Lower, 0xae8f, 0x8f),
                ] {
                    rewind(&mut machine);
                    let response = transfer(&mut machine, access(base + offset, lanes), value);
                    assert_eq!(response.result, Ok(()));
                    assert_eq!(response.wait_clocks, 0);
                    assert_eq!(
                        machine.bus.now.get(),
                        (PRIME_CLOCKS + u64::from(TRANSFER_CLOCKS)) * MAIN_CPU_DIVISOR
                    );
                    let pending = machine.vdp().registers().pending().unwrap();
                    assert_eq!(
                        (pending.index, pending.value, pending.publish_at.get()),
                        (15, expected, 100)
                    );
                    assert_eq!(machine.vdp().registers().auto_increment(), Some(0));
                    assert_eq!(
                        machine.vdp().command(),
                        ControlCommand {
                            address: 0x0f00 | u32::from(expected),
                            code: 0x12
                        }
                    );
                    machine.bus.advance_machine(16).unwrap();
                    assert_eq!(machine.vdp().registers().auto_increment(), Some(expected));
                    assert_eq!(machine.peek(base + offset), None);
                }
            }
        }
    }
}

#[test]
fn mirror_and_lane_writes() {
    mirror_and_lane_writes_with(write);
}

pub fn bus_port_faults_with(mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write) {
    for concurrent in [false, true] {
        let mut machine = ready(concurrent);
        for address in [
            0xc00000, 0xc00002, 0xc00008, 0xc0001c, 0xc00020, 0xc00024, 0xc00080, 0xc00084,
            0xcf0004, 0xd70004,
        ] {
            rewind(&mut machine);
            let mut expected = machine.vdp().clone();
            expected
                .advance_to(MasterTicks::new(84), Region::NtscUs)
                .unwrap();
            let response = transfer(&mut machine, access(address, ByteLanes::Both), 0x8f02);
            if [0xc00000, 0xc00002].contains(&address) {
                assert!(matches!(
                    response.result,
                    Err(BusError::VdpPort(VdpPortError::Data(_)))
                ));
            } else {
                assert_eq!(
                    response.result,
                    Err(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Write
                    })
                );
            }
            assert_eq!(machine.vdp(), &expected);
        }
        rewind(&mut machine);
        machine.bus.advance_machine(16).unwrap();
        let response = transfer(&mut machine, access(0xc00004, ByteLanes::Both), 0x8f02);
        assert_eq!(response.result, Ok(()));
        assert_eq!(machine.bus.now.get(), 100);
        assert_eq!(
            machine.bus.cpu_clock.0,
            PRIME_CLOCKS + u64::from(TRANSFER_CLOCKS)
        );
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0x0f02,
                code: 0x12
            }
        );
        assert_eq!(machine.vdp().control().pending_word(), None);
        assert_eq!(
            machine
                .vdp()
                .registers()
                .pending()
                .unwrap()
                .publish_at
                .get(),
            120
        );
        rewind(&mut machine);
        machine.bus.now = MasterTicks::new(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(
            transfer(&mut machine, access(0xc00004, ByteLanes::Both), 0x8f02).result,
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
    }
}

#[test]
fn bus_port_faults() {
    bus_port_faults_with(write);
}

fn program(words: &[u16], concurrent: bool) -> Emulator {
    let mut machine = super::machine(words);
    machine.concurrent_sound = concurrent;
    machine
}

pub fn actual_guest_control_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for (opcode, payload, address, increment) in [
            (0x33fc, 0x8f02, 0xc00004, 2),
            (0x13fc, 0x008f, 0xc00004, 0x8f),
            (0x13fc, 0x008f, 0xc00005, 0x8f),
            (0x33fc, 0xaf02, 0xd8ff06, 2),
        ] {
            let mut machine = program(
                &[
                    opcode,
                    payload,
                    (address >> 16) as u16,
                    address as u16,
                    0x23fc,
                    0x4567,
                    0x0003,
                    0x00c0,
                    0x0004,
                    0x4e71,
                    0x4e71,
                ],
                concurrent,
            );
            let outcome = step(&mut machine).unwrap();
            assert!(matches!(outcome.cpu, TimedOutcome::Instruction(_)));
            assert_eq!(machine.vdp().registers().auto_increment(), Some(increment));
            let copied = machine.clone();
            step(&mut machine).unwrap();
            assert_eq!(
                machine.vdp().command(),
                ControlCommand {
                    address: 0xc567,
                    code: 1
                }
            );
            assert_eq!(machine.vdp().control().pending_word(), None);
            assert_eq!(machine.vdp().registers().auto_increment(), Some(increment));
            let mut replay = copied;
            step(&mut replay).unwrap();
            assert_eq!(machine, replay);
        }
    }
}

#[test]
fn actual_guest_control() {
    actual_guest_control_with(Emulator::step_instruction);
}

pub fn actual_guest_fault_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = program(
            &[
                0x33fc, 0x5a69, 0x00ff, 0x8000, 0x23fc, 0x0123, 0, 0x00c0, 4, 0x4e71, 0x4e71,
            ],
            concurrent,
        );
        step(&mut machine).unwrap();
        assert!(matches!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Bus {
                source: BusError::VdpPort(VdpPortError::Read(_)),
                ..
            }))
        ));
        assert_eq!(machine.vdp().control().pending_word(), Some(0x0123));
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0x0123,
                code: 0x10
            }
        );
        assert_eq!(
            (machine.peek(0xff8000), machine.peek(0xff8001)),
            (Some(0x5a), Some(0x69))
        );
        let saved = machine.clone();
        assert_eq!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Frozen))
        );
        assert_eq!(machine, saved);
        machine.reset_cpu().unwrap();
        assert_eq!(machine.vdp().control().pending_word(), Some(0x0123));
        Reset::reset(&mut machine);
        assert_eq!(machine.vdp().control().pending_word(), None);
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0,
                code: 0
            }
        );
    }
}

#[test]
fn actual_guest_fault() {
    actual_guest_fault_with(Emulator::step_instruction);
}

pub fn reserved_register_write_with(
    mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write,
) {
    use crate::hardware::sound::{BankAccess, CandidateBus, schedule::PendingSound};
    for (release, publication) in [(84, 120), (112, 160)] {
        let mut machine = ready(true);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.bank = 0x1ff;
        machine.bus.sound.ram[..5].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80]);
        let mut snapshot = machine.bus.sound.clone();
        let mut setup = CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
        machine.sound_cpu.step_with_bus(&mut setup).unwrap();
        let mut pending = PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            56,
        )
        .unwrap();
        pending.start = 56;
        pending.deadline = release;
        pending.bank_access = Some(BankAccess {
            address: 0xff8000,
            direction: Direction::Write,
            value: 0x5a,
        });
        machine.bus.sound.requested = true;
        machine.sound_schedule.pending = Some(pending);
        let response = transfer(&mut machine, access(0xc00004, ByteLanes::Both), 0x8f02);
        assert_eq!(
            response.wait_clocks,
            ((release - 56) / MAIN_CPU_DIVISOR) as u32
        );
        assert_eq!(
            machine.bus.now.get(),
            release + u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR
        );
        assert_eq!(machine.vdp().now(), machine.bus.now);
        assert_eq!(
            machine.bus.cpu_clock.0,
            PRIME_CLOCKS + u64::from(response.wait_clocks) + u64::from(TRANSFER_CLOCKS)
        );
        assert_eq!(machine.peek(0xff8000), Some(0x5a));
        assert!(machine.sound_bus_granted());
        assert!(machine.sound_schedule.pending.is_none());
        assert_eq!(response.result, Ok(()));
        let pending = machine.vdp().registers().pending().unwrap();
        assert_eq!(pending.publish_at.get(), publication);
        assert_eq!(machine.vdp().registers().auto_increment(), Some(0));
        machine
            .bus
            .advance_machine(publication - machine.bus.now.get())
            .unwrap();
        assert_eq!(machine.vdp().registers().auto_increment(), Some(2));
    }
}

#[test]
fn reserved_register_write() {
    reserved_register_write_with(write);
}

pub fn guest_interrupt_enable_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = program(&[0x33fc, 0x8010, 0x00c0, 4, 0x4e71, 0x4e71], concurrent);
        let ticks = 2661 - machine.bus.now.get();
        if concurrent {
            ScheduledBus {
                bus: &mut machine.bus,
                cpu: &mut machine.sound_cpu,
                schedule: &mut machine.sound_schedule,
            }
            .advance_machine(ticks)
            .unwrap();
        } else {
            machine.bus.advance_machine(ticks).unwrap();
        }
        assert!(machine.vdp().interrupts().horizontal_pending());
        assert_eq!(machine.interrupt_level(), 0);
        step(&mut machine).unwrap();
        assert_eq!(machine.vdp().registers().registers()[0], Some(0x10));
        assert_eq!(machine.interrupt_level(), 4);
        machine.cpu.registers.set_status_register(0x2000);
        assert_eq!(
            step(&mut machine).unwrap().cpu,
            TimedOutcome::Interrupt {
                level: 4,
                vector: 28,
                clocks: 44
            }
        );
        assert_eq!(machine.interrupt_level(), 0);
    }
}

#[test]
fn guest_interrupt_enable() {
    guest_interrupt_enable_with(Emulator::step_instruction);
}

pub fn guarded_reads_and_reset() {
    for concurrent in [false, true] {
        for address in [0xc00000, 0xc00004, 0xc00008] {
            let mut machine = program(&[0x4e70, 0x4e71, 0x4e71], concurrent);
            assert_eq!(
                write(&mut machine, access(0xc00004, ByteLanes::Both), 0x4567).result,
                Ok(())
            );
            let before = machine.vdp().clone();
            let clock = machine.bus.cpu_clock;
            let result = if concurrent {
                ScheduledBus {
                    bus: &mut machine.bus,
                    cpu: &mut machine.sound_cpu,
                    schedule: &mut machine.sound_schedule,
                }
                .read_cycle(access(address, ByteLanes::Both), clock)
            } else {
                machine
                    .bus
                    .read_cycle(access(address, ByteLanes::Both), clock)
            };
            assert_eq!(
                result.result,
                if address == 0xc00000 {
                    Err(BusError::VdpPort(VdpPortError::Read(
                        crate::VdpReadError::UnsupportedCommand(before.command()),
                    )))
                } else if address == 0xc00004 {
                    Ok(0x0208)
                } else if (0xc00008..=0xc0000f).contains(&address) {
                    Ok(machine.vdp().read_hv(Region::NtscUs).unwrap())
                } else {
                    Err(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    })
                }
            );
            let mut expected = before.clone();
            expected
                .advance_to(machine.bus.now, Region::NtscUs)
                .unwrap();
            if address == 0xc00004 {
                expected.read_cpu_status(Region::NtscUs).unwrap();
            }
            assert_eq!(machine.vdp(), &expected);
            machine.cpu = PrefetchCpu::from_prefetched(
                machine.cpu.registers,
                [0x4e70, 0x4e71],
                machine.bus.cpu_clock,
            )
            .unwrap();
            machine.step_instruction().unwrap();
            assert_eq!(machine.reset_pulses(), 1);
            assert_eq!(
                machine.vdp().control().pending_word(),
                if address == 0xc00004 {
                    None
                } else {
                    Some(0x4567)
                }
            );
            assert_eq!(machine.vdp().command(), before.command());
        }
    }
}

#[test]
fn guarded_reads_and_reset_case() {
    guarded_reads_and_reset();
}
