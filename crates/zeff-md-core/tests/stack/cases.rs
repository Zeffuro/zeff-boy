use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, MachineFault, SoundFault};

fn sticky_fault(machine: &mut Emulator, address: u16, direction: Direction) {
    let cpu = machine.sound_cpu().clone();
    let mut copied = machine.clone();
    let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
        address,
        direction,
    }));
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(copied.step_instruction(), expected);
    assert_eq!(*machine, copied);
    assert_eq!(machine.sound_cpu(), &cpu);
    assert!(machine.sound_instruction_pending());
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(*machine, saved);
}

pub fn push_grant_preserves_completed_high_write() {
    let mut machine = start(
        &[0x31, 2, 1, 0x21, 0x34, 0x12, 0xe5, 0x76],
        9,
        &[],
        &[(0x101, 0x55), (0x100, 0x99)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 28);
    assert_eq!(machine.sound_cpu().regs().sp, 0x100);
    assert_eq!(machine.peek(0xa0_0101), Some(0x12));
    assert_eq!(machine.peek(0xa0_0100), Some(0));
    finish(&mut machine);
    assert_eq!(machine.peek(0xa0_0101), Some(0x55));
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert_eq!(machine.sound_cpu().regs().sp, 0x100);
}

pub fn pop_grant_keeps_low_and_reads_high_live() {
    let mut machine = start(
        &[0x31, 0, 1, 0xe1, 0x76],
        3,
        &[(0x100, 0x34), (0x101, 0x12)],
        &[(0x100, 0x99), (0x101, 0x56)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert_eq!(machine.sound_cpu().regs().sp, 0x101);
    assert_eq!(machine.sound_cpu().regs().hl(), 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x5634);
    assert_eq!(machine.sound_cpu().regs().sp, 0x102);
}

pub fn exchange_grant_preserves_latched_pair_and_write_order() {
    let sound = [0x31, 0, 1, 0x21, 0x34, 0x12, 0xe3, 0x76];
    let mut machine = start(
        &sound,
        8,
        &[(0x100, 0x78), (0x101, 0x56)],
        &[(0x100, 0x99), (0x101, 0xab)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 27);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x1234);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().hl(), 0xab78);
    let mut machine = start(
        &sound,
        12,
        &[(0x100, 0x78), (0x101, 0x56)],
        &[(0x100, 0x99), (0x101, 0xaa)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 34);
    assert_eq!(machine.peek(0xa0_0101), Some(0x12));
    assert_eq!(machine.peek(0xa0_0100), Some(0x78));
    assert_eq!(machine.sound_cpu().regs().hl(), 0x1234);
    finish(&mut machine);
    assert_eq!(machine.peek(0xa0_0101), Some(0xaa));
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert_eq!(machine.sound_cpu().regs().hl(), 0x5678);
    assert_eq!(machine.sound_cpu().regs().sp, 0x100);
}

pub fn exchange_reset_and_second_write_fault() {
    let mut machine = start(
        &[0x31, 0, 1, 0x21, 0x34, 0x12, 0xe3, 0x76],
        12,
        &[(0x100, 0x78), (0x101, 0x56)],
        &[],
    );
    assert_eq!(machine.sound_cpu().cycles(), 34);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0101), Some(0x12));
    assert_eq!(machine.peek(0xa0_0100), Some(0x78));
    assert!(!machine.sound_instruction_pending());
    let mut machine = start(
        &[0x31, 0xff, 0xff, 0x21, 0x34, 0x12, 0xe3, 0x76],
        11,
        &[],
        &[],
    );
    assert_eq!(machine.sound_cpu().cycles(), 35);
    assert_eq!(machine.peek(0xa0_0000), Some(0x12));
    assert_eq!(machine.sound_cpu().regs().hl(), 0x1234);
    sticky_fault(&mut machine, 0xffff, Direction::Write);
    assert_eq!(machine.peek(0xa0_0000), Some(0x12));
    machine.reset_sound_cpu();
    assert_eq!(machine.peek(0xa0_0000), Some(0x12));
    assert!(!machine.sound_instruction_pending());
}

pub fn push_and_pop_second_access_faults_keep_progress() {
    let mut machine = start(&[0x31, 1, 0, 0x21, 0x34, 0x12, 0xe5, 0x76], 9, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 28);
    assert_eq!(machine.sound_cpu().regs().sp, 0xffff);
    assert_eq!(machine.peek(0xa0_0000), Some(0x12));
    sticky_fault(&mut machine, 0xffff, Direction::Write);
    assert_eq!(machine.peek(0xa0_0000), Some(0x12));
    let mut machine = start(&[0x31, 3, 0x40, 0xe1, 0x76], 3, &[(0x1fff, 0x34)], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert_eq!(machine.sound_cpu().regs().sp, 0x4004);
    assert_eq!(machine.sound_cpu().regs().hl(), 0);
    sticky_fault(&mut machine, 0x4004, Direction::Read);
}
