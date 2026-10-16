use super::*;
use crate::VdpInterrupts;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

const TARGET: usize = 0x2000;
const NEXT: u16 = 0x4e71;
const WORDS: [u16; 4] = [0x0080, 0, 1, 0x0080];

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn command(vdp: &mut Vdp, code: u8) {
    vdp.write_control(TARGET as u16 | (u16::from(code & 3) << 14), Region::NtscUs)
        .unwrap();
    vdp.write_control(u16::from(code >> 2) << 4, Region::NtscUs)
        .unwrap();
}

pub(super) fn fixture(at: u64, program: &[u16], concurrent: bool) -> Emulator {
    let mut machine = super::machine(program);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = 0xc00000;
    machine.cpu.registers.d[0] = 0x12345678;
    let mut registers = *Vdp::bootstrap_mode5().registers();
    for (index, value) in [(1, 0x44), (12, 0), (10, 0xff), (15, 2), (5, 0x10)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let pending = registers.queue_register(index, value).unwrap();
        registers.advance_to(pending.publish_at).unwrap();
    }
    registers.advance_to(tick(at)).unwrap();
    let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0xff, false, false);
    while interrupts
        .advance_to(tick(at), Region::NtscUs, &registers)
        .unwrap()
        .is_some()
    {}
    machine.bus.vdp =
        Vdp::from_stable_display_state(interrupts, registers, Region::NtscUs).unwrap();
    machine.bus.now = tick(at);
    command(&mut machine.bus.vdp, 1);
    machine
}

fn access() -> BusAccess {
    BusAccess {
        logical_address: 0xc00000,
        bus_address: 0xc00000,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    }
}

pub fn transfer(machine: &mut Emulator, word: u16) -> CycleResult<(), BusError> {
    let start = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access(), word, start)
    } else {
        machine.bus.write_cycle(access(), word, start)
    }
}

pub fn cpu_writes_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        for (opcode, address, bytes, clocks) in [
            (0x1080, 0xc00000, [0x78, 0x78, 0, 0], 8),
            (0x1080, 0xd8ff03, [0x78, 0x78, 0, 0], 8),
            (0x3080, 0xc00002, [0x56, 0x78, 0, 0], 8),
            (0x2080, 0xc00000, [0x12, 0x34, 0x56, 0x78], 12),
        ] {
            let mut machine = fixture(112, &[opcode, NEXT, NEXT], concurrent);
            machine.cpu.registers.a[0] = address;
            let mut copy = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copy), Ok(result.clone()));
            assert_eq!(machine, copy);
            assert_eq!(result.elapsed.get(), clocks * MAIN_CPU_DIVISOR);
            assert_eq!(&machine.vdp().memory().vram()[TARGET..TARGET + 4], &[0; 4]);
            assert_eq!(machine.vdp().now(), machine.bus.now);
            machine
                .bus
                .advance_machine(850 - machine.bus.now.get())
                .unwrap();
            assert_eq!(&machine.vdp().memory().vram()[TARGET..TARGET + 4], &bytes);
            assert!(machine.vdp().fifo().is_empty());
            assert_eq!(
                machine
                    .vdp()
                    .sprite_cache()
                    .unwrap()
                    .entry_words(0)
                    .unwrap()[0],
                Some(u16::from_be_bytes([bytes[0], bytes[1]]) & 0x03ff)
            );
        }
    }
}

pub fn fifo_waits_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        let mut machine = fixture(112, &[0x2080, NEXT, NEXT], concurrent);
        for word in WORDS {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        let mut copy = machine.clone();
        let result = step(&mut machine).unwrap();
        assert_eq!(step(&mut copy), Ok(result.clone()));
        assert_eq!(machine, copy);
        assert_eq!(result.elapsed.get(), 110 * MAIN_CPU_DIVISOR);
        assert_eq!(machine.bus.now.get(), 882);
        assert_eq!(machine.vdp().fifo().len(), 4);
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 12);
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 8],
            &[0, 0x80, 0, 0, 0, 0, 0, 0]
        );
        machine
            .bus
            .advance_machine(2590 - machine.bus.now.get())
            .unwrap();
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 12],
            &[0, 0x80, 0, 0, 0, 1, 0, 0x80, 0x12, 0x34, 0x56, 0x78]
        );
        assert!(machine.vdp().fifo().is_empty());
    }
}

pub fn cpu_reads_with(mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>) {
    for concurrent in [false, true] {
        for (opcode, expected, clocks) in [(0x3010, 0x0080, 293), (0x2010, 0x00800000, 358)] {
            let mut machine = fixture(113, &[opcode, NEXT, NEXT], concurrent);
            for word in WORDS {
                machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
            }
            command(&mut machine.bus.vdp, 0);
            let mut copy = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copy), Ok(result.clone()));
            assert_eq!(machine, copy);
            let expected = if opcode == 0x3010 {
                0x12340000 | expected
            } else {
                expected
            };
            assert_eq!(machine.registers().d[0], expected);
            assert_eq!(result.elapsed.get(), clocks * MAIN_CPU_DIVISOR);
            assert_eq!(machine.vdp().now(), machine.bus.now);
            assert!(machine.vdp().fifo().is_empty());
        }
    }
}

pub fn live_sound_fifo_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for release in [126, 140] {
        let mut machine = fixture(112, &[0x2080, NEXT, NEXT], true);
        for word in WORDS {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        super::vdp_data::sound_hold(&mut machine, release);
        let mut copy = machine.clone();
        let result = step(&mut machine).unwrap();
        assert_eq!(step(&mut copy), Ok(result.clone()));
        assert_eq!(machine, copy);
        assert_eq!(result.elapsed.get(), 110 * MAIN_CPU_DIVISOR);
        assert_eq!(machine.bus.now.get(), 882);
        assert_eq!(machine.peek(0xff8000), Some(0x5a));
        assert!(machine.sound_bus_granted());
        assert!(machine.sound_schedule.pending.is_none());
        assert_eq!(machine.vdp().fifo().len(), 4);
    }
}

pub fn completed_wait_fault_with(
    mut write: impl FnMut(&mut Emulator, u16) -> CycleResult<(), BusError>,
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(115, &[NEXT, NEXT], concurrent);
        for word in WORDS {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        let first = write(&mut machine, 0x1234);
        assert_eq!(first.result, Ok(()));
        assert_eq!(first.wait_clocks, 33);
        let second = write(&mut machine, 0x5678);
        assert_eq!(second.wait_clocks, 64);
        assert_eq!(second.result, Ok(()));
        assert_eq!(machine.bus.now.get(), 850);
        assert_eq!(machine.vdp().fifo().len(), 4);
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 12);
        assert_eq!(
            &machine.vdp().memory().vram()[TARGET..TARGET + 4],
            &[0, 0x80, 0, 0]
        );

        let mut machine = fixture(112, &[0x2010, NEXT, NEXT], concurrent);
        for word in WORDS {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        command(&mut machine.bus.vdp, 0);
        assert!(step(&mut machine).is_ok());
        assert_eq!(machine.registers().d[0], 0x00800000);
        assert!(!machine.vdp().read_cache().is_ready());
        assert_eq!(machine.vdp().command().address, TARGET as u32 + 4);

        let mut fault = fixture(112, &[0x3010, NEXT, NEXT], concurrent);
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

pub fn reset_domains_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(208, &[NEXT, NEXT], concurrent);
        machine.bus.vdp.write_data(0x1234, Region::NtscUs).unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        machine.bus.cartridge.initial.pc = 0xc00000;
        let saved = machine.clone();
        let mut progressed = machine.clone();
        progressed.bus.advance_machine(28).unwrap();
        assert_eq!(
            &progressed.vdp().memory().vram()[TARGET..TARGET + 2],
            &[0, 0x34]
        );
        assert!(restart(&mut machine).is_err());
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        restart(&mut machine).unwrap();
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        machine.bus.vdp.write_data(0x5678, Region::NtscUs).unwrap();
        power_on(&mut machine);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(&machine.vdp().memory().vram()[TARGET..TARGET + 4], &[0; 4]);
        assert_eq!(
            machine
                .vdp()
                .sprite_cache()
                .unwrap()
                .entry_words(0)
                .unwrap(),
            [None; 2]
        );
        command(&mut machine.bus.vdp, 1);
        assert_eq!(machine.bus.vdp.write_data(0x1234, Region::NtscUs), Ok(()));
    }
}

#[test]
fn cpu_writes() {
    cpu_writes_with(Emulator::step_instruction);
}
#[test]
fn fifo_waits() {
    fifo_waits_with(Emulator::step_instruction);
}
#[test]
fn cpu_reads() {
    cpu_reads_with(Emulator::step_instruction);
}
#[test]
fn live_sound_fifo() {
    live_sound_fifo_with(Emulator::step_instruction);
}
#[test]
fn completed_wait_fault() {
    completed_wait_fault_with(transfer, Emulator::step_instruction);
}
#[test]
fn reset_domains() {
    reset_domains_with(Emulator::reset_cpu, Reset::reset);
}
