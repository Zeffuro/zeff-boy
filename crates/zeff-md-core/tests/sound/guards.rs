use super::cases::fixture::*;
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, Emulator, SoundFault};

pub fn rejected_sound_steps_preserve_machine() {
    for reset in [true, false] {
        let mut program = Vec::new();
        if !reset {
            program = load_program(&[0]);
        }
        program.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        let mut machine = new(&program, &[]);
        for _ in 0..if reset { 1 } else { 4 } {
            instruction(&mut machine);
        }
        machine.step_instruction().unwrap();
        let before = machine.clone();
        assert_eq!(
            machine.step_sound_instruction(),
            Err(if reset {
                SoundFault::ResetAsserted
            } else {
                SoundFault::BusRequested
            })
        );
        assert_eq!(machine, before);
    }
    for opcode in [0xcb, 0xdd, 0xed, 0xfd] {
        let mut machine = stopped(&[opcode, opcode, opcode], &[]);
        let before = machine.clone();
        if matches!(opcode, 0xcb | 0xed) {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 8);
            assert_eq!(machine.sound_cpu().regs().pc, 2);
            assert_eq!(
                machine.sound_cpu().regs().e,
                if opcode == 0xcb { 2 } else { 0 }
            );
            continue;
        }
        assert_eq!(
            machine.step_sound_instruction(),
            Err(SoundFault::UnsupportedOpcode { pc: 0, opcode })
        );
        assert_eq!(machine, before);
    }
    for program in [&[0x32, 0, 0x7f][..], &[0xd3, 0x7f][..], &[0xdb, 0x7f][..]] {
        let mut machine = stopped(program, &[]);
        let before = machine.clone();
        if program[0] != 0x32 {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 11);
            assert_eq!(machine.sound_cpu().regs().pc, 2);
            assert_eq!(
                machine.sound_cpu().regs().a,
                if program[0] == 0xdb { 0xff } else { 0 }
            );
            assert_eq!(machine.sound_bank(), before.sound_bank());
            assert_eq!(machine.peek(SOUND_RAM), before.peek(SOUND_RAM));
            continue;
        }
        assert!(matches!(
            machine.step_sound_instruction(),
            Err(SoundFault::UnsupportedAccess { .. })
        ));
        assert_eq!(machine, before);
    }
    let mut machine = stopped(&[0], &[]);
    machine.set_interrupt_level(4);
    let before = machine.clone();
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::MainInterruptPending)
    );
    assert_eq!(machine, before);

    let mut machine = stopped(&[0], &[0x3039, 0x0040, 0, 0x4e71, 0x4e71]);
    wake(&mut machine);
    assert!(machine.step_instruction().is_err());
    machine.set_interrupt_level(0);
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::MainCpuRunning)
    );
}

pub fn unavailable_main_access_is_host_fault() {
    for (address, kind) in [
        (SOUND_RAM, 0),
        (SOUND_RAM, 1),
        (SOUND_RAM + 0x7f00, 2),
        (BUS_REQUEST + 1, 2),
        (RESET, 3),
    ] {
        let mut program = Vec::new();
        if kind >= 1 {
            store_word(&mut program, BUS_REQUEST, 0x0100);
        }
        if kind >= 2 {
            store_word(&mut program, RESET, 0x0100);
        }
        program.extend_from_slice(&[
            0x1039,
            (address >> 16) as u16,
            address as u16,
            0x4e71,
            0x4e71,
        ]);
        let mut machine = new(&program, &[]);
        for _ in 0..usize::from(kind >= 1) + usize::from(kind >= 2) {
            instruction(&mut machine);
        }
        if kind == 1 {
            assert!(machine.sound_bus_requested() && !machine.sound_bus_granted());
        }
        if kind < 2 {
            instruction(&mut machine);
            assert_eq!(machine.registers().d[0], 0x4e);
            assert!(machine.sound_reset_asserted());
            assert!(!machine.sound_bus_granted());
            instruction(&mut machine);
            continue;
        }
        if address == BUS_REQUEST + 1 || address == RESET {
            let cpu = machine.sound_cpu().clone();
            let bank = machine.sound_bank();
            instruction(&mut machine);
            assert_eq!(
                machine.registers().d[0],
                if address == BUS_REQUEST + 1 {
                    0x71
                } else {
                    0x4e
                }
            );
            assert!(machine.sound_bus_requested() && machine.sound_bus_granted());
            assert!(!machine.sound_reset_asserted());
            assert_eq!(machine.sound_cpu(), &cpu);
            assert_eq!(machine.sound_bank(), bank);
            instruction(&mut machine);
            continue;
        }
        unsupported_main_access(&mut machine, address, Direction::Read);
    }
    for address in [BUS_REQUEST + 1, RESET + 1] {
        let mut program = Vec::new();
        store_byte(&mut program, address, 1);
        program.extend_from_slice(&[0x4e71, 0x4e71]);
        let mut machine = new(&program, &[]);
        unsupported_main_access(&mut machine, address & !1, Direction::Write);
        assert!(!machine.sound_bus_requested());
        assert!(machine.sound_reset_asserted());
    }
}

pub fn independent_component_reset() {
    let mut machine = stopped_with_bank(&[0x3e, 0x55, 0x76], &[0x4e70, 0x4e71, 0x4e71], 0x55);
    machine.step_sound_instruction().unwrap();
    let sound = machine.sound_cpu().clone();
    wake(&mut machine);
    machine.set_interrupt_level(4);
    instruction(&mut machine);
    assert_eq!(machine.reset_pulses(), 1);
    assert_eq!(machine.sound_cpu(), &sound);
    assert_eq!(machine.sound_bank(), 0x55);
    assert!(!machine.sound_reset_asserted());
    machine.set_interrupt_level(4);
    let before = machine.timing_snapshot();
    let sound = machine.sound_cpu().clone();
    machine.reset_cpu().unwrap();
    assert_eq!(machine.sound_cpu(), &sound);
    assert_eq!(machine.interrupt_level(), 4);
    assert_eq!(
        machine
            .timing_snapshot()
            .elapsed_since(before)
            .unwrap()
            .get(),
        56
    );
    assert_eq!(machine.peek(SOUND_RAM), Some(0x3e));
    assert_eq!(machine.sound_bank(), 0x55);
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::MainCpuRunning)
    );
}

fn unsupported_main_access(machine: &mut Emulator, address: u32, direction: Direction) {
    let fault = machine.step_instruction().map_err(main_fault);
    assert!(
        matches!(
            fault,
            Err(TimingFault::Bus {
                source: BusError::UnsupportedAccess { address: actual, direction: actual_direction },
                ..
            }) if actual == address && actual_direction == direction
        ),
        "access={address:06x} direction={direction:?}: {fault:?}"
    );
    let frozen = machine.clone();
    assert_eq!(
        machine.step_instruction().map_err(main_fault),
        Err(TimingFault::Frozen)
    );
    assert_eq!(*machine, frozen);
}

fn main_fault(fault: zeff_md_core::MachineFault) -> TimingFault<BusError> {
    match fault {
        zeff_md_core::MachineFault::Main(fault) => fault,
        zeff_md_core::MachineFault::Sound(fault) => panic!("sound fault: {fault:?}"),
        zeff_md_core::MachineFault::Input(fault) => panic!("input fault: {fault:?}"),
        zeff_md_core::MachineFault::PreciseTiming(fault) => panic!("precise timing fault: {fault}"),
    }
}
