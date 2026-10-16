use super::*;
use crate::hardware::vdp::{VdpPortError, VdpReadError};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

const TARGET: usize = 0x2000;
const WORDS: [u16; 4] = [0x1234, 0x5678, 0x9abc, 0xdef0];

fn command(vdp: &mut Vdp, address: u16, code: u8) {
    vdp.write_control(address | (u16::from(code & 3) << 14), Region::NtscUs)
        .unwrap();
    vdp.write_control(u16::from(code >> 2) << 4, Region::NtscUs)
        .unwrap();
}

fn fixture(concurrent: bool, opcode: u16, address: u16) -> Emulator {
    let mut machine = super::machine(&[opcode, 0x4e71, 0x4e71]);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = 0xc00000;
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(MasterTicks::new(1), Region::NtscUs).unwrap();
    vdp.write_control(0x8f02, Region::NtscUs).unwrap();
    vdp.advance_to(MasterTicks::new(56), Region::NtscUs)
        .unwrap();
    command(&mut vdp, address, 1);
    for word in WORDS {
        vdp.write_data(word, Region::NtscUs).unwrap();
    }
    command(&mut vdp, address, 0);
    machine.bus.vdp = vdp;
    machine
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
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

pub fn cpu_reads_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        for (opcode, expected, lane) in [
            (0x1010, 0x12, 0),
            (0x1010, 0x34, 1),
            (0x3010, 0x1234, 2),
            (0x2010, 0x12345678, 0),
        ] {
            let mut machine = fixture(concurrent, opcode, TARGET as u16);
            machine.cpu.registers.a[0] += lane;
            let mut copy = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copy), Ok(result.clone()));
            assert_eq!(machine, copy);
            assert_eq!(machine.registers().d[0], expected);
            assert!(result.elapsed.get() > 12 * MAIN_CPU_DIVISOR);
            assert_eq!(machine.vdp().now(), machine.bus.now);
            assert!(machine.vdp().fifo().is_empty());
            assert_eq!(
                &machine.vdp().memory().vram()[TARGET..TARGET + 8],
                &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
            );
        }
    }
}

pub fn mapped_reads_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    for concurrent in [false, true] {
        for (address, lanes) in [
            (0xc00000, ByteLanes::Both),
            (0xc00002, ByteLanes::Upper),
            (0xd8ff03, ByteLanes::Lower),
        ] {
            let mut machine = fixture(concurrent, 0x4e71, TARGET as u16);
            machine.bus.prefetch_upper = Some(0xaa);
            let response = read(&mut machine, access(address, lanes));
            assert_eq!(response.result, Ok(WORDS[0]));
            assert_eq!(response.wait_clocks, 24);
            assert_eq!(machine.bus.now.get(), 252);
            assert_eq!(machine.bus.prefetch_upper, None);
            assert!(!machine.vdp().read_cache().is_ready());
            assert_eq!(machine.vdp().command().address, TARGET as u32 + 2);
        }
    }
}

pub fn completed_wait_fault_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, 0x4e71, TARGET as u16);
        machine
            .bus
            .vdp
            .advance_to(MasterTicks::new(510), Region::NtscUs)
            .unwrap();
        machine.bus.now = MasterTicks::new(510);
        command(&mut machine.bus.vdp, 0x4e, 4);
        let response = read(&mut machine, access(0xc00000, ByteLanes::Both));
        assert_eq!(
            response.result,
            Err(BusError::VdpPort(VdpPortError::Read(
                VdpReadError::UnsupportedCommand(crate::ControlCommand {
                    address: 0x50,
                    code: 4
                })
            )))
        );
        assert_eq!(response.wait_clocks, 2);
        assert_eq!(machine.bus.now.get(), 552);
        assert!(machine.vdp().read_cache().is_ready());
        assert_eq!(machine.vdp().command().address, 0x50);
        assert_eq!(machine.vdp().read_cache().buffered_word(), Some(0x1000));
    }
}

pub fn reset_reads_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, 0x4e71, TARGET as u16);
        let pointer = machine.vdp().memory().vram().as_ptr();
        restart(&mut machine).unwrap();
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert!(!machine.vdp().read_cache().is_ready());
        machine.bus.advance_machine(119).unwrap();
        let mut completed = machine.vdp().clone();
        completed
            .advance_to(MasterTicks::new(259), Region::NtscUs)
            .unwrap();
        assert!(completed.read_cache().is_ready());
        assert!(!machine.vdp().read_cache().is_ready());
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let before = machine.clone();
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, before);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        power_on(&mut machine);
        assert_eq!(machine.vdp().read_cache().buffered_word(), None);
        assert!(machine.vdp().memory().vram().iter().all(|&byte| byte == 0));
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    }
}

pub fn reserved_read_with(
    mut read: impl FnMut(&mut Emulator, BusAccess) -> CycleResult<u16, BusError>,
) {
    let mut machine = fixture(true, 0x4e71, TARGET as u16);
    super::vdp_data::sound_hold(&mut machine, 84);
    let response = read(&mut machine, access(0xc00002, ByteLanes::Both));
    assert_eq!(response.result, Ok(WORDS[0]));
    assert_eq!(response.wait_clocks, 24);
    assert_eq!(machine.bus.now.get(), 252);
    assert_eq!(machine.peek(0xff8000), Some(0x5a));
    let mut machine = fixture(true, 0x4e71, TARGET as u16);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..10]
        .copy_from_slice(&[0x21, 1, 0x80, 0x3e, 0x5a, 0x32, 0, 0x80, 0x36, 0x5a]);
    let mut snapshot = machine.bus.sound.clone();
    let mut setup =
        crate::hardware::sound::CandidateBus::new(&mut snapshot, &machine.bus.cartridge);
    machine.sound_cpu.step_with_bus(&mut setup).unwrap();
    machine.sound_cpu.step_with_bus(&mut setup).unwrap();
    let mut pending = crate::hardware::sound::schedule::PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        0,
    )
    .unwrap();
    pending.start = 56;
    pending.deadline = 84;
    pending.bank_access = Some(crate::hardware::sound::BankAccess {
        address: 0xff8000,
        direction: zeff_m68k::bus::Direction::Write,
        value: 0x5a,
    });
    machine.sound_schedule.pending = Some(pending);
    assert_eq!(
        read(&mut machine, access(0xc00000, ByteLanes::Both)).result,
        Ok(WORDS[0])
    );
    assert_eq!(machine.bus.now.get(), 252);
    assert_eq!(machine.peek(0xff8001), Some(0));
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert_eq!(pending.bank_access.unwrap().address, 0xff8001);
    assert!(pending.start >= machine.bus.now.get());
    assert!(pending.deadline > machine.bus.now.get());
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(60)
    .unwrap();
    assert_eq!(machine.peek(0xff8001), Some(0x5a));
}

pub fn cpu_read_fault_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, 0x3010, TARGET as u16);
        command(&mut machine.bus.vdp, 0x4e, 4);
        assert!(matches!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Bus {
                source: BusError::VdpPort(VdpPortError::Read(VdpReadError::UnsupportedCommand(
                    crate::ControlCommand {
                        address: 0x50,
                        code: 4
                    }
                ))),
                ..
            }))
        ));
        assert!(machine.vdp().read_cache().is_ready());
        assert!(machine.vdp().fifo().is_empty());
        assert_eq!(machine.vdp().command().address, 0x50);
        let before = machine.clone();
        assert_eq!(
            step(&mut machine),
            Err(MachineFault::Main(TimingFault::Frozen))
        );
        assert_eq!(machine, before);
    }
}

#[test]
fn cpu_reads() {
    cpu_reads_with(Emulator::step_instruction);
}
#[test]
fn mapped_reads() {
    mapped_reads_with(transfer);
}
#[test]
fn completed_wait_fault() {
    completed_wait_fault_with(transfer);
}
#[test]
fn reset_reads() {
    reset_reads_with(Emulator::reset_cpu, Reset::reset);
}
#[test]
fn reserved_read() {
    reserved_read_with(transfer);
}
#[test]
fn cpu_read_fault() {
    cpu_read_fault_with(Emulator::step_instruction);
}
