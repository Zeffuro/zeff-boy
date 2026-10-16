use crate::interrupt_service::cases as interrupt;
use interrupt::multi::{finish, fixture::*, start};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{ControllerKind, ControllerPort, SoundFault};

pub fn jump_low_is_latched_and_high_is_live() {
    let mut machine = start(
        &[0, 0xc3, 0x34, 1, 0x76],
        0,
        &[(0x234, 0x76)],
        &[(2, 0x99), (3, 2)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 11);
    assert_eq!(machine.sound_cpu().regs().pc, 3);
    assert!(machine.sound_instruction_pending());
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x235);
    assert_eq!(machine.peek(SOUND_RAM + 2), Some(0x99));
}

pub fn jump_reset_discards_pending_operand() {
    let mut machine = start(
        &[0, 0xc3, 0x34, 1, 0x76],
        0,
        &[(0x299, 0x76)],
        &[(2, 0x99), (3, 2)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 11);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert!(!machine.sound_instruction_pending());
    assert_eq!(machine.peek(SOUND_RAM + 3), Some(1));
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x29a);
}

pub fn conditional_jump_reads_high_before_deciding() {
    let mut machine = start(&[0xaf, 0xc2, 0x34, 1, 0x76], 0, &[], &[(2, 0x99), (3, 2)]);
    assert_eq!(machine.sound_cpu().cycles(), 11);
    assert_eq!(machine.sound_cpu().regs().pc, 3);
    assert_eq!(machine.sound_cpu().regs().f, 0x44);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    for opcode in [0xc3, 0xc2] {
        let mut machine = interrupt::ready(
            &[0x31, 0, 0x10, 0xaf, 0xc3, 0xfe, 0x3f],
            &[],
            &[(0x1ffe, opcode), (0x1fff, 0x78), (0x78, 0x76)],
        );
        for _ in 0..32 {
            if machine.sound_cpu().cycles() >= 34 {
                break;
            }
            instruction(&mut machine);
        }
        assert!(machine.sound_fault().is_none());
        assert_eq!(machine.sound_cpu().regs().sp, 0x1000);
        if opcode == 0xc3 {
            assert_eq!(machine.sound_cpu().regs().pc, 0x78);
        } else {
            assert!((0x4001..=0x4004).contains(&machine.sound_cpu().regs().pc));
        }
    }
}

pub fn pending_jump_defers_irq_and_pushes_target() {
    let mut machine = start(
        &[0x31, 0, 0x10, 0xfb, 0, 0xc3, 0x34, 1, 0x76],
        7,
        &[(0x38, 0x76), (0x234, 0x76)],
        &[(6, 0x99), (7, 2)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 25);
    assert_eq!(machine.sound_cpu().regs().pc, 7);
    assert!(!machine.sound_cpu().last_step_was_interrupt());
    machine.set_sound_interrupt(true);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x39);
    assert_eq!(machine.sound_cpu().regs().sp, 0xffe);
    assert_eq!(machine.peek(SOUND_RAM + 0xfff), Some(2));
    assert_eq!(machine.peek(SOUND_RAM + 0xffe), Some(0x34));
}

pub fn immediate_loads_preserve_index_and_controller_rules() {
    let sound = [
        0x21, 0x34, 0x12, 0xdd, 0x26, 0xaa, 0xfd, 0x2e, 0xbb, 0x26, 0xcc, 0x2e, 0xdd, 0x76,
    ];
    let mut machine = stopped(&sound, &[]);
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 10);
    let saved = machine.clone();
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::UnsupportedOpcode {
            pc: 3,
            opcode: 0xdd
        })
    );
    assert_eq!(machine, saved);
    let mut machine = interrupt::ready(&sound, &[], &[]);
    machine
        .input_mut()
        .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
    finish(&mut machine);
    let registers = machine.sound_cpu().regs();
    assert_eq!(
        (registers.hl(), registers.ix, registers.iy),
        (0xccdd, 0xaa00, 0xbb)
    );
    machine.reset_sound_cpu();
    assert_eq!(
        machine.input().controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
}
