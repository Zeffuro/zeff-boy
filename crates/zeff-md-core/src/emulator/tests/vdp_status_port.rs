use super::*;
use crate::hardware::vdp::{VdpPortError, VdpStatusError};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

const STATUS: u32 = 0xc00004;
const NEXT: u16 = 0x4e71;
const FLOAT_MASK: u16 = 0xfc00;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn fixture(concurrent: bool, program: &[u16]) -> Emulator {
    let mut machine = super::machine(program);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = STATUS;
    machine
        .bus
        .vdp
        .write_control(0x4567, Region::NtscUs)
        .unwrap();
    machine
}

pub fn transfer(machine: &mut Emulator, access: BusAccess) -> CycleResult<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .read_cycle(access, clock)
    } else {
        machine.bus.read_cycle(access, clock)
    }
}

pub fn cpu_lanes_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        for (opcode, lane, address, expected) in [
            (0x3010, 0, STATUS, 0x4e0c),
            (0x1010, 0, STATUS + 2, 0x4e),
            (0x1010, 1, 0xd8ff06, 0x0c),
        ] {
            let mut machine = fixture(concurrent, &[opcode, NEXT, NEXT]);
            machine.cpu.registers.a[0] = address + lane;
            let command = machine.vdp().command();
            let mut copied = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copied), Ok(result.clone()));
            assert_eq!(machine, copied);
            assert_eq!(machine.registers().d[0], expected);
            assert_eq!(machine.vdp().control().pending_word(), None);
            assert_eq!(machine.vdp().command(), command);
            assert_eq!(result.elapsed.get(), 8 * MAIN_CPU_DIVISOR);
        }
    }
}

pub fn upper_history_and_mirrors_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    for concurrent in [false, true] {
        for upper in 0..=u8::MAX {
            for (address, lanes) in [(STATUS, ByteLanes::Both), (0xd8ff06, ByteLanes::Upper)] {
                let mut machine = fixture(concurrent, &[NEXT; 3]);
                machine.bus.prefetch_upper = Some(upper);
                let response = read(&mut machine, access(address, lanes));
                assert_eq!(
                    response.result,
                    Ok(((u16::from(upper) << 8) & FLOAT_MASK) | 0x020c)
                );
                assert_eq!(response.wait_clocks, 0);
                assert_eq!(machine.bus.now.get(), 84);
                assert_eq!(machine.vdp().control().pending_word(), None);
                assert_eq!(machine.bus.prefetch_upper, None);
            }
        }
    }
}

pub fn unknown_upper_and_low_lane_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    for concurrent in [false, true] {
        for lanes in [ByteLanes::Both, ByteLanes::Upper] {
            let mut machine = fixture(concurrent, &[NEXT; 3]);
            machine.bus.prefetch_upper = None;
            let command = machine.vdp().command();
            assert_eq!(read(&mut machine, access(STATUS, lanes)).result, Ok(0x020c));
            assert_eq!(machine.bus.now.get(), 84);
            assert_eq!(machine.vdp().control().pending_word(), None);
            assert_eq!(machine.vdp().command(), command);
            let response = read(&mut machine, access(STATUS + 1, ByteLanes::Lower));
            assert_eq!(response.result, Ok(0x0208));
            assert_eq!(response.wait_clocks, 0);
            assert_eq!(machine.bus.now.get(), 112);
            assert_eq!(machine.vdp().control().pending_word(), None);
        }
    }
}

pub fn active_sound_history_fault_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for lower in [false, true] {
        let mut machine = fixture(true, &[if lower { 0x1010 } else { 0x3010 }, NEXT, NEXT]);
        machine.bus.sound.set_reset(false);
        if lower {
            machine.cpu.registers.a[0] += 1;
        }
        if lower {
            step(&mut machine).unwrap();
            assert_eq!(machine.registers().d[0], 12);
            assert_eq!(machine.vdp().control().pending_word(), None);
        } else {
            step(&mut machine).unwrap();
            assert_eq!(machine.vdp().control().pending_word(), None);
            let mut fault = fixture(true, &[0x3010, NEXT, NEXT]);
            fault.bus.sound.set_reset(false);
            fault.cpu.registers.a[0] = 0xc00010;
            assert!(matches!(
                step(&mut fault),
                Err(MachineFault::Main(TimingFault::Bus {
                    source: BusError::UnsupportedAccess {
                        address: 0xc00010,
                        direction: zeff_m68k::bus::Direction::Read
                    },
                    ..
                }))
            ));
            let saved = fault.clone();
            assert_eq!(
                step(&mut fault),
                Err(MachineFault::Main(TimingFault::Frozen))
            );
            assert_eq!(fault, saved);
        }
    }
}

pub fn completed_transfer_and_overflow_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, &[NEXT; 3]);
        machine
            .bus
            .vdp
            .advance_to(MasterTicks::new(90), Region::NtscUs)
            .unwrap();
        machine.bus.vdp.read_status(Region::NtscUs).unwrap();
        machine
            .bus
            .vdp
            .write_control(0x8f02, Region::NtscUs)
            .unwrap();
        assert_eq!(
            machine.vdp().registers().pending().unwrap().publish_at,
            MasterTicks::new(100)
        );
        machine.bus.now = MasterTicks::new(90);
        let word = machine.vdp().control().pending_word();
        assert_eq!(
            read(&mut machine, access(STATUS + 1, ByteLanes::Lower)).result,
            Ok(0x0208)
        );
        assert!(word.is_none());
        assert_eq!(machine.vdp().registers().registers()[15], Some(2));
        assert!(machine.vdp().registers().pending().is_none());
        machine.bus.now = MasterTicks::new(u64::MAX - 1);
        machine.bus.prefetch_upper = Some(0xa5);
        let saved = machine.clone();
        assert_eq!(
            read(&mut machine, access(STATUS, ByteLanes::Both)).result,
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
    }
}

pub fn reset_history_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, &[NEXT; 3]);
        machine
            .bus
            .vdp
            .write_control(0x8144, Region::NtscUs)
            .unwrap();
        machine.bus.advance_machine(5).unwrap();
        machine
            .bus
            .vdp
            .write_control(0x8144, Region::NtscUs)
            .unwrap();
        machine.bus.advance_machine(20).unwrap();
        machine
            .bus
            .vdp
            .write_control(0x8104, Region::NtscUs)
            .unwrap();
        machine.bus.advance_machine(20).unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        assert_eq!(
            machine.bus.vdp.read_status(Region::NtscUs),
            Err(VdpPortError::Status(VdpStatusError::UnknownHistory))
        );
        restart(&mut machine).unwrap();
        assert_eq!(
            machine.bus.vdp.read_status(Region::NtscUs),
            Err(VdpPortError::Status(VdpStatusError::UnknownHistory))
        );
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let saved = machine.clone();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        power_on(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(machine.bus.vdp.read_status(Region::NtscUs), Ok(0x020c));
        machine
            .bus
            .vdp
            .write_control(0x4567, Region::NtscUs)
            .unwrap();
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let saved = machine.clone();
        let mut progressed = machine.clone();
        assert!(
            transfer(&mut progressed, access(STATUS, ByteLanes::Both))
                .result
                .is_ok()
        );
        assert_eq!(progressed.vdp().control().pending_word(), None);
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    }
}

pub fn longword_history_fault_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, &[0x2010, NEXT, NEXT]);
        machine.cpu.registers.a[0] = 0xc0000e;
        let command = machine.vdp().command();
        assert!(matches!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Bus {
                source: BusError::UnsupportedAccess {
                    address: 0xc00010,
                    direction: zeff_m68k::bus::Direction::Read
                },
                ..
            }))
        ));
        assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
        assert_eq!(machine.vdp().command(), command);
        assert_eq!(machine.bus.now.get(), 112);
        assert_eq!(machine.registers().d[0], 0);
        let saved = machine.clone();
        assert_eq!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Frozen))
        );
        assert_eq!(machine, saved);
    }
}

#[test]
fn cpu_lanes() {
    cpu_lanes_with(Emulator::step_instruction);
}
#[test]
fn upper_history_and_mirrors() {
    upper_history_and_mirrors_with(transfer);
}
#[test]
fn unknown_upper_and_low_lane() {
    unknown_upper_and_low_lane_with(transfer);
}
#[test]
fn active_sound_history_fault() {
    active_sound_history_fault_with(Emulator::step_instruction);
}
#[test]
fn completed_transfer_and_overflow() {
    completed_transfer_and_overflow_with(transfer);
}
#[test]
fn reset_history() {
    reset_history_with(Emulator::reset_cpu, Reset::reset);
}
#[test]
fn longword_history_fault() {
    longword_history_fault_with(Emulator::step_instruction);
}
