use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn ed_word_address_high_is_live_after_grant() {
    let mut machine = start(
        &[0xed, 0x6b, 0, 1, 0x76],
        0,
        &[(0x200, 0x78), (0x201, 0x56)],
        &[(2, 0x20), (3, 2)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 11);
    assert_eq!(machine.sound_cpu().regs().pc, 3);
    assert_eq!(machine.sound_cpu().regs().hl(), 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x5678);
}

pub fn ed_word_keeps_low_and_reads_high_live() {
    let mut machine = start(
        &[0xed, 0x4b, 0, 1, 0x76],
        3,
        &[(0x100, 0x34), (0x101, 0x12)],
        &[(0x100, 0x99), (0x101, 0x56)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert_eq!(machine.sound_cpu().regs().bc(), 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().bc(), 0x5634);
}

pub fn ed_word_write_grant_and_reset_keep_first_byte() {
    let sound = [0x21, 0x34, 0x12, 0xed, 0x63, 0, 1, 0x76];
    let mut machine = start(&sound, 8, &[], &[(5, 0), (6, 2), (0x100, 0x99)]);
    assert_eq!(machine.sound_cpu().cycles(), 27);
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    finish(&mut machine);
    assert_eq!(machine.peek(0xa0_0100), Some(0x99));
    assert_eq!(machine.peek(0xa0_0101), Some(0x12));
    assert_eq!(machine.peek(0xa0_0201), Some(0));
    let mut machine = start(&sound, 8, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 27);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert_eq!(machine.peek(0xa0_0101), Some(0));
    assert!(!machine.sound_instruction_pending());
}

pub fn ed_returns_keep_low_and_read_high_live() {
    for opcode in [0x45, 0x4d, 0x55, 0x5d, 0x65, 0x6d, 0x75, 0x7d] {
        let mut machine = start(
            &[0x31, 0, 1, 0xed, opcode],
            5,
            &[(0x100, 0x10), (0x101, 0), (0x110, 0x76)],
            &[(0x100, 0x20), (0x101, 1)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 21);
        assert_eq!(machine.sound_cpu().regs().pc, 5);
        assert_eq!(machine.sound_cpu().regs().sp, 0x101);
        finish(&mut machine);
        assert_eq!(machine.sound_cpu().regs().pc, 0x111);
        assert_eq!(machine.sound_cpu().regs().sp, 0x102);
    }
}

pub fn ed_second_access_faults_keep_progress() {
    for (sound, nops, clocks, pc, address, direction) in [
        (
            &[0x21, 0x34, 0x12, 0xed, 0x63, 3, 0x40][..],
            8,
            27,
            7,
            0x4004,
            Direction::Write,
        ),
        (
            &[0x31, 3, 0x40, 0xed, 0x45][..],
            5,
            21,
            5,
            0x4004,
            Direction::Read,
        ),
    ] {
        let mut machine = start(sound, nops, &[(0x1fff, 0x56)], &[]);
        assert_eq!(machine.sound_cpu().cycles(), clocks);
        assert_eq!(machine.sound_cpu().regs().pc, pc);
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
            assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
        } else {
            assert_eq!(machine.sound_cpu().regs().sp, 0x4004);
        }
        let saved = machine.clone();
        assert_eq!(machine.step_instruction(), expected);
        assert_eq!(machine, saved);
        let now = machine.timing_snapshot().now();
        machine.reset_sound_cpu();
        assert_eq!(machine.timing_snapshot().now(), now);
        assert!(!machine.sound_instruction_pending());
    }
}
