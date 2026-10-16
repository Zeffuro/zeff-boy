use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{ControllerPort, Emulator, LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

fn start(sound: &[u8], writes: &[(u32, u8)]) -> Emulator {
    let mut program = load_program(sound);
    store_byte(&mut program, SOUND_RAM + 0x100, 0x55);
    store_word(&mut program, BUS_REQUEST, 0);
    store_word(&mut program, BUS_REQUEST, 0x100);
    program.extend_from_slice(&[0x1e39, 0x00a0, 0x0100]);
    for &(address, byte) in writes {
        store_byte(&mut program, SOUND_RAM + address, byte);
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(&[0x4e71; 96]);
    let mut machine = Emulator::new(
        image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + 6 {
        instruction(&mut machine);
    }
    assert!(machine.sound_bus_requested());
    assert!(machine.sound_instruction_pending());
    assert!(!machine.sound_prefix_pending());
    machine
}

fn finish(machine: &mut Emulator) {
    let mut copied = machine.clone();
    for _ in 0..64 {
        if machine.sound_cpu().state() == CpuState::Halted {
            return;
        }
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(*machine, copied);
    }
    panic!("sound did not halt");
}

pub fn immediate_operand_changes_after_grant() {
    let mut machine = start(&[0x3e, 0x11, 0x3e, 0x22, 0x76], &[(2, 0xdd), (3, 0x77)]);
    assert_eq!(machine.sound_cpu().regs().pc, 3);
    assert_eq!(machine.sound_cpu().regs().a, 0x11);
    let mut copied = machine.clone();
    for _ in 0..2 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        assert_eq!(machine.sound_cpu().regs().pc, 3);
        assert_eq!(machine.sound_cpu().regs().a, 0x11);
    }
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.peek(SOUND_RAM + 2), Some(0xdd));
    assert_eq!(machine.peek(SOUND_RAM + 3), Some(0x77));
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().a, 0x77);
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    assert!(!machine.sound_instruction_pending());
    assert!(machine.sound_cpu().cycles() >= 18);
}

pub fn memory_read_and_write_resume_after_grant() {
    for opcode in [0x7e, 0x77] {
        let mut machine = start(&[0x21, 0, 1, opcode, 0x76], &[(3, 0), (0x100, 0x5a)]);
        assert_eq!(machine.sound_cpu().regs().pc, 4);
        assert_eq!(machine.sound_cpu().cycles(), 14);
        assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x55));
        let mut copied = machine.clone();
        for _ in 0..2 {
            assert_eq!(machine.step_instruction(), copied.step_instruction());
            assert_eq!(machine, copied);
        }
        assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x5a));
        finish(&mut machine);
        assert_eq!(
            machine.sound_cpu().regs().a,
            if opcode == 0x7e { 0x5a } else { 0 }
        );
        assert_eq!(
            machine.peek(SOUND_RAM + 0x100),
            Some(if opcode == 0x7e { 0x5a } else { 0 })
        );
        assert!(!machine.sound_instruction_pending());
    }
}

pub fn transfer_fault_preserves_m1() {
    let mut machine = start(&[0x21, 4, 0x40, 0x7e, 0x76], &[]);
    assert_eq!(machine.sound_cpu().regs().pc, 4);
    assert_eq!(machine.sound_cpu().cycles(), 14);
    let cpu = machine.sound_cpu().clone();
    let expected = MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x4004,
        direction: Direction::Read,
    });
    for _ in 0..8 {
        if let Err(fault) = machine.step_instruction() {
            assert_eq!(fault, expected);
            break;
        }
    }
    assert_eq!(machine.sound_cpu(), &cpu);
    assert!(machine.sound_instruction_pending());
    assert_eq!(
        machine.sound_fault(),
        Some(SoundFault::UnsupportedAccess {
            address: 0x4004,
            direction: Direction::Read
        })
    );
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected));
    assert_eq!(machine, saved);
    machine.reset_sound_cpu();
    assert!(!machine.sound_instruction_pending());
    assert!(machine.sound_fault().is_none());
}

pub fn transfer_reset_ownership() {
    let mut machine = start(&[0x21, 0, 1, 0x77, 0x76], &[]);
    machine
        .input_mut()
        .set_controller_raw(ControllerPort::One, 0x7f);
    machine.set_interrupt_level(4);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.sound_cpu().regs().pc, 0);
    assert!(!machine.sound_instruction_pending());
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x55));
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.interrupt_level(), 4);
    assert_eq!(machine.input().controller_raw(ControllerPort::One), 0x7f);
    let mut fresh = machine.clone();
    Reset::reset(&mut machine);
    Reset::reset(&mut fresh);
    assert_eq!(machine, fresh);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0));
    assert!(!machine.sound_instruction_pending());
}
