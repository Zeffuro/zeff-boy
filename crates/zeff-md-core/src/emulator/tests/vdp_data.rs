use super::*;
use crate::hardware::vdp::VdpPortError;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

type Write = CycleResult<(), BusError>;
const TARGET: usize = 0x2000;
const INITIAL: [u16; 4] = [0x1111, 0x2222, 0x3333, 0x4444];

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn data_access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn write(machine: &mut Emulator, access: BusAccess, value: u16) -> Write {
    let start = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access, value, start)
    } else {
        machine.bus.write_cycle(access, value, start)
    }
}

fn ready(at: u64, code: u8, concurrent: bool) -> Emulator {
    fixture(at, code, concurrent, &[0x2080, 0x4e71, 0x4e71])
}

fn fixture(at: u64, code: u8, concurrent: bool, program: &[u16]) -> Emulator {
    let mut machine = super::machine(program);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = 0xc00000;
    machine.cpu.registers.d[0] = 0x12345678;
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(tick(1), Region::NtscUs).unwrap();
    vdp.write_control(0x8f02, Region::NtscUs).unwrap();
    vdp.advance_to(tick(21), Region::NtscUs).unwrap();
    vdp.write_control(
        (TARGET as u16) | (u16::from(code & 3) << 14),
        Region::NtscUs,
    )
    .unwrap();
    vdp.write_control(u16::from(code >> 2) << 4, Region::NtscUs)
        .unwrap();
    vdp.advance_to(tick(at), Region::NtscUs).unwrap();
    machine.bus.now = tick(at);
    machine.bus.vdp = vdp;
    machine
}

pub fn data_lanes_and_service_with(
    mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write,
) {
    for concurrent in [false, true] {
        for (address, lanes, data, expected) in [
            (0xc00000, ByteLanes::Both, 0xa1b2, [0xa1, 0xb2]),
            (0xc00002, ByteLanes::Upper, 0xa1ed, [0xa1; 2]),
            (0xd8ff03, ByteLanes::Lower, 0xeda1, [0xa1; 2]),
        ] {
            let mut machine = ready(56, 1, concurrent);
            assert_eq!(
                transfer(&mut machine, data_access(address, lanes), data).result,
                Ok(())
            );
            assert_eq!(machine.bus.now.get(), 84);
            assert_eq!(&machine.vdp().memory().vram()[TARGET..TARGET + 2], &[0, 0]);
            assert_eq!(machine.vdp().command().address, TARGET as u32 + 2);
            assert_eq!(machine.vdp().fifo().len(), 1);
            machine.bus.advance_machine(6).unwrap();
            assert_eq!(
                &machine.vdp().memory().vram()[TARGET..TARGET + 2],
                &[0, expected[1]]
            );
            assert_eq!(machine.vdp().fifo().len(), 1);
            machine.bus.advance_machine(20).unwrap();
            assert_eq!(
                &machine.vdp().memory().vram()[TARGET..TARGET + 2],
                &expected
            );
            assert!(machine.vdp().fifo().is_empty());
        }
    }
}

#[test]
fn data_lanes_and_service() {
    data_lanes_and_service_with(write);
}

pub fn fifo_retirement_waits_with(
    mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write,
) {
    for concurrent in [false, true] {
        for (at, code, partial, waits, completion) in [
            (56, 1, false, 1, 91),
            (509, 1, false, 2, 551),
            (511, 1, true, 2, 553),
            (521, 3, false, 1, 556),
        ] {
            let mut machine = ready(if partial { 509 } else { at }, code, concurrent);
            machine
                .bus
                .vdp
                .write_data(INITIAL[0], Region::NtscUs)
                .unwrap();
            if partial {
                machine.bus.advance_machine(at - 509).unwrap();
            }
            for data in &INITIAL[1..] {
                machine.bus.vdp.write_data(*data, Region::NtscUs).unwrap();
            }
            let response = transfer(&mut machine, data_access(0xc00000, ByteLanes::Both), 0x5678);
            assert_eq!(response.result, Ok(()));
            assert_eq!(response.wait_clocks, waits);
            assert_eq!(machine.bus.now.get(), completion);
            assert_eq!(machine.bus.cpu_clock.0, PRIME_CLOCKS + 4 + u64::from(waits));
            assert_eq!(machine.vdp().now(), machine.bus.now);
            assert_eq!(machine.vdp().fifo().len(), 4);
            if code == 1 {
                assert_eq!(
                    &machine.vdp().memory().vram()[TARGET..TARGET + 2],
                    &[0x11; 2]
                );
            } else {
                assert_eq!(machine.vdp().memory().cram()[0], 0x0000);
            }
            assert_eq!(machine.vdp().command().address, TARGET as u32 + 10);
        }
    }
}

#[test]
fn fifo_retirement_waits() {
    fifo_retirement_waits_with(write);
}

pub fn actual_longword_waits_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = ready(56, 1, concurrent);
        for data in INITIAL {
            machine.bus.vdp.write_data(data, Region::NtscUs).unwrap();
        }
        let mut copied = machine.clone();
        let outcome = step(&mut machine).unwrap();
        assert_eq!(step(&mut copied), Ok(outcome.clone()));
        assert_eq!(machine, copied);
        assert!(
            matches!(outcome.cpu, TimedOutcome::Instruction(ref instruction) if instruction.clocks == 15)
        );
        assert_eq!(outcome.elapsed.get(), 15 * MAIN_CPU_DIVISOR);
        assert_eq!(machine.cpu_clock().0, PRIME_CLOCKS + 15);
        assert_eq!(machine.bus.now.get(), 161);
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 6],
            &[0x11, 0x11, 0x22, 0x22, 0, 0x33]
        );
        assert_eq!(machine.vdp().fifo().len(), 4);
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 12);
        assert_eq!(machine.registers().pc, 0x102);
    }
}

#[test]
fn actual_longword_waits() {
    actual_longword_waits_with(Emulator::step_instruction);
}

pub fn completed_wait_failure_with(
    mut transfer: impl FnMut(&mut Emulator, BusAccess, u16) -> Write,
) {
    for concurrent in [false, true] {
        let mut machine = ready(515, 3, concurrent);
        for data in INITIAL {
            machine.bus.vdp.write_data(data, Region::NtscUs).unwrap();
        }
        let response = transfer(&mut machine, data_access(0xc00000, ByteLanes::Both), 0x5678);
        assert_eq!(response.wait_clocks, 1);
        assert_eq!(response.result, Ok(()));
        assert_eq!(machine.bus.now.get(), 550);
        assert_eq!(machine.vdp().fifo().len(), 4);
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 10);
        assert_eq!(machine.vdp().memory().cram()[0], 0x0000);
        assert_eq!(machine.bus.cpu_clock.0, PRIME_CLOCKS + 5);
        machine.bus.now = tick(u64::MAX - 1);
        let saved = machine.clone();
        let pointer = machine.vdp().memory().vram().as_ptr();
        let failed = transfer(&mut machine, data_access(0xc00000, ByteLanes::Both), 0x9abc);
        assert_eq!(failed.result, Err(BusError::ClockOverflow));
        assert_eq!(failed.wait_clocks, 0);
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    }
}

#[test]
fn completed_wait_failure() {
    completed_wait_failure_with(write);
}

pub fn reset_memory_domains_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = ready(56, 1, concurrent);
        for data in INITIAL {
            machine.bus.vdp.write_data(data, Region::NtscUs).unwrap();
        }
        let pointer = machine.vdp().memory().vram().as_ptr();
        let mut expected = machine.vdp().clone();
        expected.advance_to(tick(112), Region::NtscUs).unwrap();
        restart(&mut machine).unwrap();
        assert_eq!(machine.vdp(), &expected);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.now = tick(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(restart(&mut machine), Err(TimingFault::ClockOverflow));
        assert_eq!(machine, saved);
        power_on(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert!(machine.vdp().memory().vram().iter().all(|&byte| byte == 0));
        assert!(machine.vdp().fifo().is_empty());
        assert_eq!(machine.vdp().now().get(), PRIME_CLOCKS * MAIN_CPU_DIVISOR);
    }
}

pub fn reset_memory_domains() {
    reset_memory_domains_with(Emulator::reset_cpu, Reset::reset);
}

#[test]
fn reset_memory_domains_case() {
    reset_memory_domains();
}

pub fn mapped_data_guards() {
    for concurrent in [false, true] {
        let mut machine = ready(56, 1, concurrent);
        let clock = machine.bus.cpu_clock;
        let result = if concurrent {
            ScheduledBus {
                bus: &mut machine.bus,
                cpu: &mut machine.sound_cpu,
                schedule: &mut machine.sound_schedule,
            }
            .read_cycle(data_access(0xc00000, ByteLanes::Both), clock)
        } else {
            machine
                .bus
                .read_cycle(data_access(0xc00000, ByteLanes::Both), clock)
        };
        assert_eq!(
            result.result,
            Err(BusError::VdpPort(VdpPortError::Read(
                crate::VdpReadError::UnsupportedCommand(machine.vdp().command())
            )))
        );
        assert!(machine.vdp().fifo().is_empty());
        machine.bus.now = tick(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(
            write(&mut machine, data_access(0xc00000, ByteLanes::Both), 1).result,
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
    }
}

#[test]
fn mapped_data_guards_case() {
    mapped_data_guards();
}

pub(super) fn sound_hold(machine: &mut Emulator, release: u64) {
    use crate::hardware::sound::{BankAccess, CandidateBus, schedule::PendingSound};
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
        0,
    )
    .unwrap();
    pending.start = machine.bus.now.get();
    pending.deadline = release;
    pending.bank_access = Some(BankAccess {
        address: 0xff8000,
        direction: Direction::Write,
        value: 0x5a,
    });
    machine.bus.sound.requested = true;
    machine.sound_schedule.pending = Some(pending);
}

pub fn live_sound_and_fifo_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for (release, clocks) in [(70, 15), (84, 16)] {
        let mut machine = ready(56, 1, true);
        for data in INITIAL {
            machine.bus.vdp.write_data(data, Region::NtscUs).unwrap();
        }
        sound_hold(&mut machine, release);
        let mut copied = machine.clone();
        let outcome = step(&mut machine).unwrap();
        assert_eq!(step(&mut copied), Ok(outcome.clone()));
        assert_eq!(machine, copied);
        assert!(
            matches!(outcome.cpu, TimedOutcome::Instruction(ref instruction) if instruction.clocks == clocks)
        );
        assert_eq!(machine.bus.now.get(), 56 + clocks * MAIN_CPU_DIVISOR);
        assert_eq!(outcome.elapsed.get(), clocks * MAIN_CPU_DIVISOR);
        assert_eq!(machine.peek(0xff8000), Some(0x5a));
        assert!(machine.sound_bus_granted());
        assert!(machine.sound_schedule.pending.is_none());
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 6],
            &[0x11, 0x11, 0x22, 0x22, 0, 0x33]
        );
        assert_eq!(machine.vdp().fifo().len(), 4);
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 12);
    }
}

pub fn actual_byte_and_word_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for (opcode, value, address, expected) in [
            (0x33fc, 0xa1b2, 0xc00002, [0xa1, 0xb2]),
            (0x13fc, 0x00a1, 0xc00000, [0xa1; 2]),
            (0x13fc, 0x00a1, 0xc00001, [0xa1; 2]),
        ] {
            let mut machine = fixture(
                56,
                1,
                concurrent,
                &[
                    opcode,
                    value,
                    (address >> 16) as u16,
                    address as u16,
                    0x4e71,
                    0x4e71,
                ],
            );
            let outcome = step(&mut machine).unwrap();
            assert!(
                matches!(outcome.cpu, TimedOutcome::Instruction(ref instruction) if instruction.clocks == 20)
            );
            assert_eq!(outcome.elapsed.get(), 20 * MAIN_CPU_DIVISOR);
            assert_eq!(
                &machine.vdp().memory().vram()[TARGET..TARGET + 2],
                &expected
            );
            assert!(machine.vdp().fifo().is_empty());
            assert_eq!(machine.vdp().command().address, TARGET as u32 + 2);
        }
    }
}

#[test]
fn actual_byte_and_word() {
    actual_byte_and_word_with(Emulator::step_instruction);
}

#[test]
fn live_sound_and_fifo() {
    live_sound_and_fifo_with(Emulator::step_instruction);
}

pub fn failed_prime_restores_memory_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
) {
    let mut machine = ready(u64::MAX - 100, 1, true);
    machine.bus.vdp.write_data(0x1234, Region::NtscUs).unwrap();
    sound_hold(&mut machine, u64::MAX - 50);
    let pointer = machine.vdp().memory().vram().as_ptr();
    let before = machine.clone();
    let mut progressed = machine.vdp().clone();
    progressed
        .advance_to(tick(u64::MAX - 16), Region::NtscUs)
        .unwrap();
    assert_eq!(
        &progressed.memory().vram()[TARGET..TARGET + 2],
        &[0x12, 0x34]
    );
    assert!(matches!(
        restart(&mut machine),
        Err(TimingFault::Bus {
            source: BusError::ClockOverflow,
            ..
        })
    ));
    assert_eq!(machine, before);
    assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    assert_eq!(&machine.vdp().memory().vram()[TARGET..TARGET + 2], &[0, 0]);
    assert_eq!(machine.peek(0xff8000), Some(0));
    assert_eq!(machine.vdp().fifo().len(), 1);
}

#[test]
fn failed_prime_restores_memory() {
    failed_prime_restores_memory_with(Emulator::reset_cpu);
}

pub fn reset_output_drains_fifo_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(56, 1, concurrent, &[0x4e70, 0x4e71, 0x4e71]);
        for data in INITIAL {
            machine.bus.vdp.write_data(data, Region::NtscUs).unwrap();
        }
        machine
            .bus
            .vdp
            .write_control(0x4567, Region::NtscUs)
            .unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        step(&mut machine).unwrap();
        assert_eq!(machine.reset_pulses(), 1);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 8],
            &[0x11, 0x11, 0x22, 0x22, 0x33, 0x33, 0x44, 0x44]
        );
        assert!(machine.vdp().fifo().is_empty());
        assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
        assert_eq!(machine.vdp().command().address, 0x0567);
    }
}

#[test]
fn reset_output_drains_fifo() {
    reset_output_drains_fifo_with(Emulator::step_instruction);
}
