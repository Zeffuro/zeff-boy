use zeff_emu_common::time::{MachineTiming, MasterTicks, Reset};
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::{TimedOutcome, TimingFault};
use zeff_md_core::constants::{MAIN_CPU_DIVISOR, NTSC_MASTER_HZ};
use zeff_md_core::{BusError, Emulator, LoadConfig, LoadError, Region};

fn word(rom: &mut [u8], address: usize, value: u16) {
    rom[address..address + 2].copy_from_slice(&value.to_be_bytes());
}

fn rom(program: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, &value) in program.iter().enumerate() {
        word(&mut bytes, 0x100 + index * 2, value);
    }
    for vector in [9, 28, 31] {
        word(&mut bytes, vector * 4, 0);
        word(&mut bytes, vector * 4 + 2, 0x180);
    }
    word(&mut bytes, 0x180, 0x4e73);
    word(&mut bytes, 0x182, 0x4e71);
    bytes
}

fn machine(program: &[u16]) -> Emulator {
    Emulator::new(rom(program), LoadConfig::default()).unwrap()
}

fn instruction(machine: &mut Emulator) -> u64 {
    let step = machine.step_instruction().unwrap();
    let TimedOutcome::Instruction(cpu) = step.cpu else {
        panic!("instruction")
    };
    assert_eq!(step.elapsed.get(), cpu.clocks * MAIN_CPU_DIVISOR);
    cpu.clocks
}

pub fn bounded_loading_and_startup() {
    let mut image = rom(&[0x4e71, 0x4e71]);
    assert_eq!(
        Emulator::new(vec![0; 7], LoadConfig::default()),
        Err(LoadError::CartridgeSize(7))
    );
    let mut ntsc_times = Vec::new();
    for (region, version) in [(Region::NtscUs, 0xa0), (Region::NtscJapan, 0x20)] {
        let mut machine = Emulator::new(
            rom(&[0x1039, 0x00a1, 0x0001, 0x4e71]),
            LoadConfig {
                region,
                ..Default::default()
            },
        )
        .unwrap();
        instruction(&mut machine);
        assert_eq!(machine.registers().d[0], version);
        assert_eq!(machine.region(), region);
        assert_eq!(machine.region().frame_ticks(), Region::NtscUs.frame_ticks());
        assert_eq!(
            machine.timing_snapshot().rate().numerator_hz(),
            NTSC_MASTER_HZ
        );
        ntsc_times.push(machine.timing_snapshot().now());
    }
    assert_eq!(ntsc_times[0], ntsc_times[1]);
    let machine = Emulator::new(image.clone(), LoadConfig::default()).unwrap();
    assert_eq!(
        (machine.registers().pc, machine.registers().ssp),
        (0x100, 0x00ff_fffc)
    );
    assert_eq!(machine.registers().status_register(), 0x2700);
    assert_eq!(machine.queued_words(), Some([0x4e71; 2]));
    assert_eq!(machine.cpu_clock().0, 8);
    assert_eq!(machine.timing_snapshot().now(), MasterTicks::new(56));
    assert_eq!(
        machine.timing_snapshot().rate().numerator_hz(),
        NTSC_MASTER_HZ
    );
    image[7] = 1;
    assert_eq!(
        Emulator::new(image.clone(), LoadConfig::default()),
        Err(LoadError::ResetProgramCounter(0x101))
    );
    image[7] = 0;
    image[3] |= 1;
    assert_eq!(
        Emulator::new(image, LoadConfig::default()),
        Err(LoadError::ResetStackPointer(0x00ff_fffd))
    );
}

pub fn actual_program_and_boundary_copies() {
    let mut machine = machine(&[
        0x207c, 0x01ff, 0x0000, 0x70ff, 0x2080, 0x2210, 0x4e71, 0x4e71,
    ]);
    instruction(&mut machine);
    instruction(&mut machine);
    let mut copied = machine.clone();
    for _ in 0..3 {
        assert_eq!(
            machine.step_instruction().map_err(main_fault),
            copied.step_instruction().map_err(main_fault)
        );
        assert_eq!(machine, copied);
    }
    assert_eq!(machine.registers().a[0], 0x01ff_0000);
    assert_eq!(machine.registers().d[1], u32::MAX);
    for offset in 0..4 {
        assert_eq!(machine.peek(0xff_0000 + offset), Some(255));
        assert_eq!(machine.peek(0x01e1_0000 + offset), Some(255));
    }
    assert_eq!(
        machine.timing_snapshot().now().get(),
        machine.cpu_clock().0 * MAIN_CPU_DIVISOR
    );
}

pub fn lanes_and_immutable_cartridge() {
    for (address, expected) in [(0x01e0_0000u32, [0x55, 0x34]), (0x01ff_0001, [0x12, 0x55])] {
        let mut machine = machine(&[
            0x33fc,
            0x1234,
            0x00ff,
            0x0000,
            0x7055,
            0x13c0,
            (address >> 16) as u16,
            address as u16,
            0x13c0,
            0,
            0,
            0x4e71,
            0x4e71,
        ]);
        for _ in 0..4 {
            instruction(&mut machine);
        }
        assert_eq!(
            [
                machine.peek(0xff_0000).unwrap(),
                machine.peek(0xff_0001).unwrap()
            ],
            expected
        );
        assert_eq!(machine.peek(0), Some(0));
        assert_eq!(machine.peek(0x3f_ffff), Some(255));
        assert_eq!(machine.peek(0x40_0000), None);
    }
}

pub fn stopped_clock_irq_and_return() {
    let mut machine = machine(&[0x4e72, 0x2000, 0x7001, 0x4e71, 0x4e71]);
    assert_eq!(instruction(&mut machine), 4);
    for _ in 0..3 {
        let step = machine.step_instruction().unwrap();
        assert_eq!(step.cpu, TimedOutcome::Stopped);
        assert_eq!(step.elapsed.get(), 7);
    }
    assert_eq!(machine.cpu_clock().0, 12);
    assert_eq!(machine.timing_snapshot().now().get(), 105);
    assert!(machine.set_interrupt_level(4));
    assert!(!machine.set_interrupt_level(8));
    assert_eq!(machine.interrupt_level(), 4);
    let step = machine.step_instruction().unwrap();
    assert_eq!(
        step.cpu,
        TimedOutcome::Interrupt {
            level: 4,
            vector: 28,
            clocks: 44
        }
    );
    assert_eq!(step.elapsed.get(), 308);
    assert_eq!(machine.timing_snapshot().now().get(), 413);
    assert_eq!(machine.registers().ssp, 0xff_fff6);
    assert_eq!(
        [
            machine.peek(0xff_fff6).unwrap(),
            machine.peek(0xff_fff7).unwrap()
        ],
        [0x20, 0]
    );
    assert_eq!(
        [
            machine.peek(0xff_fffa).unwrap(),
            machine.peek(0xff_fffb).unwrap()
        ],
        [1, 4]
    );
    assert_eq!(machine.interrupt_level(), 4);
    machine.set_interrupt_level(0);
    assert_eq!(instruction(&mut machine), 20);
    assert_eq!(machine.registers().pc, 0x104);
    assert_eq!(machine.registers().ssp, 0xff_fffc);
    assert_eq!(machine.registers().status_register(), 0x2000);
    instruction(&mut machine);
    assert_eq!(machine.registers().d[0], 1);
}

pub fn level_seven_edge_rearming() {
    let mut machine = machine(&[0x4e71, 0x4e71, 0x4e71]);
    machine.set_interrupt_level(7);
    assert!(matches!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Interrupt {
            level: 7,
            vector: 31,
            ..
        }
    ));
    instruction(&mut machine);
    assert_eq!(machine.registers().pc, 0x100);
    instruction(&mut machine);
    assert_eq!(machine.registers().pc, 0x102);
    machine.set_interrupt_level(0);
    instruction(&mut machine);
    machine.set_interrupt_level(7);
    assert!(matches!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Interrupt { level: 7, .. }
    ));
}

pub fn distinct_reset_ownership() {
    let image = rom(&[0x7055, 0x33c0, 0x00ff, 0x0000, 0x4e70, 0x4e71, 0x4e71]);
    let mut machine = Emulator::new(image.clone(), LoadConfig::default()).unwrap();
    instruction(&mut machine);
    instruction(&mut machine);
    machine.set_interrupt_level(4);
    assert_eq!(instruction(&mut machine), 132);
    assert_eq!(machine.reset_pulses(), 1);
    assert_eq!(machine.registers().d[0], 0x55);
    assert_eq!(machine.peek(0xff_0001), Some(0x55));
    assert_eq!(machine.interrupt_level(), 4);
    let old = machine.timing_snapshot();
    machine.reset_cpu().unwrap();
    assert_eq!(
        machine.timing_snapshot().elapsed_since(old).unwrap().get(),
        56
    );
    assert_eq!(machine.peek(0xff_0001), Some(0x55));
    assert_eq!(machine.interrupt_level(), 4);
    assert_eq!(machine.reset_pulses(), 1);
    assert_eq!(machine.registers().d[0], 0);
    assert_eq!(machine.cpu_clock().0, 8);
    instruction(&mut machine);
    assert_eq!(machine.registers().d[0], 0x55);
    Reset::reset(&mut machine);
    assert_eq!(
        machine,
        Emulator::new(image, LoadConfig::default()).unwrap()
    );
}

pub fn unsupported_access_is_host_fault_without_retry() {
    let mut machine = machine(&[0x3039, 0x0040, 0x0000, 0x4e71, 0x4e71]);
    assert!(matches!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Bus {
            source: BusError::UnsupportedAccess {
                address: 0x40_0000,
                direction: Direction::Read
            },
            ..
        })
    ));
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Frozen)
    );
    assert_eq!(machine, saved);
    machine.reset_cpu().unwrap();
    assert_eq!(machine.registers().pc, 0x100);
}

pub fn address_error_remains_explicitly_unqualified() {
    let mut machine = machine(&[0x3039, 0x00ff, 0x0001, 0x4e71, 0x4e71]);
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::AddressError { address: 0xff_0001 })
    );
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Frozen)
    );
    assert_eq!(machine, saved);
}

pub fn ordinary_trace_enters_mapped_vector_and_stack() {
    let mut machine = machine(&[0x007c, 0x8000, 0x4e71, 0x4e71, 0x4e71]);
    instruction(&mut machine);
    let step = machine.step_instruction().unwrap();
    let TimedOutcome::Instruction(cpu) = step.cpu else {
        panic!("instruction")
    };
    assert_eq!(cpu.exception, Some(9));
    assert_eq!(cpu.clocks, 38);
    assert_eq!(step.elapsed.get(), 266);
    assert_eq!(machine.registers().pc, 0x180);
    assert_eq!(machine.registers().ssp, 0xff_fff6);
    assert_eq!(machine.registers().status_register() & 0x8000, 0);
}

fn main_fault(fault: zeff_md_core::MachineFault) -> TimingFault<BusError> {
    match fault {
        zeff_md_core::MachineFault::Main(fault) => fault,
        zeff_md_core::MachineFault::Sound(fault) => panic!("sound fault: {fault:?}"),
    }
}
