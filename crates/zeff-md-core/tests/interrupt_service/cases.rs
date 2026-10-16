pub(crate) use crate::multi_transfer_cases as multi;
use multi::fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{
    ControllerKind, ControllerPort, Emulator, LoadConfig, MachineFault, SoundFault,
};
use zeff_z80::InterruptMode;

pub(crate) fn ready(sound: &[u8], tail: &[u16], source: &[(u32, u8)]) -> Emulator {
    let mut program = load_program(sound);
    for &(address, value) in source {
        store_byte(&mut program, SOUND_RAM + address, value);
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(tail);
    program.extend_from_slice(&[0x4e71; 128]);
    let mut machine = Emulator::new(
        image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + source.len() + 3 {
        instruction(&mut machine);
    }
    machine
}

pub(crate) fn halt_ready(machine: &mut Emulator) {
    for _ in 0..64 {
        if machine.sound_cpu().is_halted() && machine.sound_cpu().interrupts_enabled() {
            return;
        }
        instruction(machine);
    }
    panic!("sound halt");
}

pub fn manual_irq_modes_precede_prefixed_opcodes() {
    for (mode, target, clocks) in [(0x46, 0x38, 13), (0x56, 0x38, 13), (0x5e, 0x1234, 19)] {
        let mut sound = vec![0x31, 0, 0x10, 0xed, mode, 0x3e, 0x12, 0xed, 0x47];
        if mode == 0x5e {
            sound.extend_from_slice(&[0x21, 0xff, 0x12, 0x36, 0x34, 0x23, 0x36, 0x12]);
        }
        sound.extend_from_slice(&[0xfb, 0, 0xdd, 0xfd, 0x76]);
        let mut machine = stopped(&sound, &[]);
        for _ in 0..6 + if mode == 0x5e { 4 } else { 0 } {
            machine.step_sound_instruction().unwrap();
        }
        let pc = machine.sound_cpu().regs().pc;
        let saved = machine.clone();
        assert_eq!(
            machine.step_sound_instruction(),
            Err(SoundFault::UnsupportedOpcode { pc, opcode: 0xdd })
        );
        assert_eq!(machine, saved);
        machine.set_sound_interrupt(true);
        let now = machine.timing_snapshot().now().get();
        let main = machine.registers();
        let mut copied = machine.clone();
        let result = machine.step_sound_instruction();
        assert_eq!(result, copied.step_sound_instruction());
        assert_eq!(machine, copied);
        assert_eq!(result.unwrap().cycles, clocks);
        assert_eq!(machine.sound_cpu().regs().pc, target);
        assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
        assert_eq!(machine.peek(SOUND_RAM + 0xfff), Some((pc >> 8) as u8));
        assert_eq!(machine.peek(SOUND_RAM + 0xffe), Some(pc as u8));
        assert_eq!(machine.registers(), main);
        assert_eq!(
            machine.timing_snapshot().now().get(),
            (now / 15 + u64::from(clocks)) * 15
        );
        assert!(machine.sound_cpu().last_step_was_interrupt());
        assert!(machine.sound_fault().is_none());
    }
}

pub fn concurrent_irq_preserves_live_line_and_partial_stack() {
    let mut machine = ready(
        &[0x31, 0, 0x10, 0xfb, 0x76],
        &[],
        &[
            (0xfff, 0xaa),
            (0xffe, 0xbb),
            (0x38, 0xfb),
            (0x39, 0),
            (0x3a, 0x76),
        ],
    );
    halt_ready(&mut machine);
    machine.set_sound_interrupt(true);
    for _ in 0..16 {
        if machine.sound_cpu().regs().sp == 0xfff {
            break;
        }
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().sp, 0xfff);
    assert_eq!(machine.peek(SOUND_RAM + 0xfff), Some(0xaa));
    machine.set_sound_interrupt(false);
    for _ in 0..8 {
        if machine.sound_cpu().regs().sp == 0xffe {
            break;
        }
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
    assert_eq!(machine.peek(SOUND_RAM + 0xfff), Some(0));
    assert_eq!(machine.peek(SOUND_RAM + 0xffe), Some(0xbb));
    let mut reset = machine.clone();
    let now = reset.timing_snapshot().now();
    reset.reset_sound_cpu();
    assert_eq!(reset.timing_snapshot().now(), now);
    assert_eq!(reset.peek(SOUND_RAM + 0xfff), Some(0));
    assert_eq!(reset.peek(SOUND_RAM + 0xffe), Some(0xbb));
    assert!(!reset.sound_instruction_pending());
    let mut copied = machine.clone();
    for _ in 0..32 {
        if machine.sound_cpu().is_halted() {
            break;
        }
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    assert!(machine.sound_cpu().is_halted());
    assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
    assert_eq!(machine.peek(SOUND_RAM + 0xffe), Some(5));
    assert!(machine.sound_cpu().interrupts_enabled());
}

pub fn failed_irq_write_retains_prior_progress_and_reset_line() {
    let mut sound = vec![0x31, 1, 0x60, 0xfb, 0, 0xc3, 0, 1];
    sound.resize(0x100, 0);
    sound.push(0x76);
    let mut machine = ready(&sound, &[], &[]);
    halt_ready(&mut machine);
    machine.set_sound_interrupt(true);
    let mut copied = machine.clone();
    let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x5fff,
        direction: Direction::Write,
    }));
    for _ in 0..32 {
        let result = machine.step_instruction();
        assert_eq!(result, copied.step_instruction());
        assert_eq!(machine, copied);
        if result.is_err() {
            assert_eq!(result, expected);
            break;
        }
    }
    assert!(machine.sound_fault().is_some());
    assert_eq!(machine.sound_cpu().regs().sp, 0x5fff);
    assert_eq!(machine.sound_cpu().regs().pc, 0x101);
    assert_eq!(machine.sound_bank(), 0x100);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.sound_bank(), 0x100);
    assert_eq!(machine.sound_cpu().interrupt_mode(), InterruptMode::Im0);
    assert_eq!(machine.sound_cpu().regs().r, 0);
    assert!(machine.sound_fault().is_none());
    for _ in 0..128 {
        if machine.sound_cpu().last_step_was_interrupt() {
            break;
        }
        instruction(&mut machine);
    }
    assert!(machine.sound_cpu().last_step_was_interrupt());

    let mut machine = ready(
        &[
            0x31, 0, 0x10, 0x3e, 0x7e, 0xed, 0x47, 0xed, 0x5e, 0xfb, 0x76,
        ],
        &[],
        &[(0x1fff, 0x34)],
    );
    halt_ready(&mut machine);
    machine.set_sound_interrupt(true);
    let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x7f00,
        direction: Direction::Read,
    }));
    for _ in 0..32 {
        let result = machine.step_instruction();
        if result.is_err() {
            assert_eq!(result, expected);
            break;
        }
    }
    assert!(machine.sound_fault().is_some());
    assert_eq!(machine.sound_cpu().regs().pc, 11);
    assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
    assert_eq!(machine.peek(SOUND_RAM + 0xfff), Some(0));
    assert_eq!(machine.peek(SOUND_RAM + 0xffe), Some(11));
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
}

pub fn im2_vector_high_is_live_after_grant() {
    let sound = [
        0x31, 0, 0x10, 0x3e, 0x12, 0xed, 0x47, 0xed, 0x5e, 0xfb, 0x76,
    ];
    let mut tail = vec![0x4e71; 27];
    store_word(&mut tail, BUS_REQUEST, 0x100);
    store_byte(&mut tail, SOUND_RAM + 0x12ff, 0x99);
    store_byte(&mut tail, SOUND_RAM + 0x1300, 2);
    store_word(&mut tail, BUS_REQUEST, 0);
    let mut machine = ready(&sound, &tail, &[(0x12ff, 0x34), (0x1300, 1), (0x234, 0x76)]);
    halt_ready(&mut machine);
    machine.set_sound_interrupt(true);
    for _ in 0..32 {
        if machine.sound_bus_granted() {
            break;
        }
        instruction(&mut machine);
    }
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.sound_cpu().cycles(), 62);
    assert_eq!(machine.sound_cpu().regs().pc, 11);
    assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
    let mut copied = machine.clone();
    for _ in 0..32 {
        if machine.sound_cpu().is_halted() {
            break;
        }
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    assert!(machine.sound_cpu().is_halted());
    assert_eq!(machine.sound_cpu().regs().pc, 0x235);
    assert_eq!(machine.peek(SOUND_RAM + 0x12ff), Some(0x99));
}

pub fn reset_and_halt_interrupts_keep_controller_state() {
    let mut machine = stopped(&[0x31, 0, 0x10, 0xfb, 0x76], &[]);
    machine
        .input_mut()
        .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
    machine.step_sound_instruction().unwrap();
    machine.step_sound_instruction().unwrap();
    machine.step_sound_instruction().unwrap();
    let r = machine.sound_cpu().regs().r;
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 4);
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    assert_eq!(machine.sound_cpu().regs().r, (r & 0x80) | ((r + 1) & 0x7f));
    machine.set_sound_interrupt(true);
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 13);
    assert_eq!(machine.sound_cpu().regs().pc, 0x38);
    Reset::reset(&mut machine);
    assert_eq!(machine.sound_cpu().interrupt_mode(), InterruptMode::Im0);
    assert_eq!(machine.sound_cpu().regs().i, 0);
    assert_eq!(machine.sound_cpu().regs().r, 0);
    assert_eq!(
        machine.input().controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
}
