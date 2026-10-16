use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn call_reads_high_operand_after_grant() {
    let mut machine = start(
        &[0x31, 2, 1, 0xcd, 0x10, 0, 0x76],
        3,
        &[(0x110, 0x76)],
        &[(4, 0x20), (5, 1)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    assert_eq!(machine.sound_cpu().regs().sp, 0x102);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x111);
    assert_eq!(machine.peek(0xa0_0100), Some(6));
    assert_eq!(machine.peek(0xa0_0101), Some(0));
}

pub fn call_keeps_target_and_completed_write() {
    let mut machine = start(
        &[0x31, 2, 1, 0xcd, 0x10, 1, 0x76],
        6,
        &[(0x110, 0x76)],
        &[(4, 0x20), (5, 2), (0x100, 0x99), (0x101, 0x55)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 24);
    assert_eq!(machine.sound_cpu().regs().pc, 6);
    assert_eq!(machine.sound_cpu().regs().sp, 0x100);
    assert_eq!(machine.peek(0xa0_0100), Some(0));
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x111);
    assert_eq!(machine.peek(0xa0_0100), Some(6));
    assert_eq!(machine.peek(0xa0_0101), Some(0x55));
}

pub fn return_keeps_low_and_reads_high_live() {
    let mut machine = start(
        &[0x31, 0, 1, 0xc9],
        3,
        &[(0x100, 0x10), (0x101, 0), (0x110, 0x76)],
        &[(0x100, 0x20), (0x101, 1)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert_eq!(machine.sound_cpu().regs().pc, 4);
    assert_eq!(machine.sound_cpu().regs().sp, 0x101);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 0x111);
    assert_eq!(machine.sound_cpu().regs().sp, 0x102);
}

pub fn restart_keeps_completed_write_and_return_address() {
    let mut machine = start(
        &[0x31, 2, 1, 0xcf],
        4,
        &[(8, 0x76)],
        &[(0x100, 0x99), (0x101, 0x55)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 18);
    assert_eq!(machine.sound_cpu().regs().pc, 4);
    assert_eq!(machine.sound_cpu().regs().sp, 0x100);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().pc, 9);
    assert_eq!(machine.peek(0xa0_0100), Some(4));
    assert_eq!(machine.peek(0xa0_0101), Some(0x55));
}

pub fn control_faults_and_reset_preserve_progress() {
    for (sound, nops, clocks, pc, sp, address, direction) in [
        (
            &[0x31, 1, 0, 0xcd, 0x10, 1][..],
            7,
            24,
            6,
            0xffff,
            0xffff,
            Direction::Write,
        ),
        (
            &[0x31, 1, 0, 0xcf][..],
            4,
            18,
            4,
            0xffff,
            0xffff,
            Direction::Write,
        ),
        (
            &[0x31, 3, 0x40, 0xc9][..],
            3,
            17,
            4,
            0x4004,
            0x4004,
            Direction::Read,
        ),
    ] {
        let mut machine = start(sound, nops, &[(0x1fff, 0x34)], &[]);
        assert_eq!(machine.sound_cpu().cycles(), clocks);
        assert_eq!(machine.sound_cpu().regs().pc, pc);
        assert_eq!(machine.sound_cpu().regs().sp, sp);
        let cpu = machine.sound_cpu().clone();
        let mut copied = machine.clone();
        let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
            address,
            direction,
        }));
        assert_eq!(machine.step_instruction(), expected);
        assert_eq!(copied.step_instruction(), expected);
        assert_eq!(machine, copied);
        assert_eq!(machine.sound_cpu(), &cpu);
        if direction == Direction::Write {
            assert_eq!(machine.peek(0xa0_0000), Some(0));
        }
        let saved = machine.clone();
        assert_eq!(machine.step_instruction(), expected);
        assert_eq!(machine, saved);
        let now = machine.timing_snapshot().now();
        machine.reset_sound_cpu();
        assert_eq!(machine.timing_snapshot().now(), now);
        assert!(!machine.sound_instruction_pending());
    }
    let mut machine = start(&[0x31, 2, 1, 0xcd, 0x10, 1, 0x76], 7, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 24);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0100), Some(0));
    assert_eq!(machine.peek(0xa0_0101), Some(0));
    assert!(!machine.sound_instruction_pending());
}
