use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn indexed_address_and_read_after_grant() {
    let mut machine = start(
        &[0xfd, 0x21, 1, 1, 0xfd, 0x7e, 0xff, 0x76],
        7,
        &[(0x100, 0x11)],
        &[(6, 0), (0x100, 0x5a), (0x101, 0x99)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 25);
    assert_eq!(machine.sound_cpu().regs().pc, 7);
    assert_eq!(machine.sound_cpu().regs().a, 0);
    assert!(machine.sound_prefix_pending());
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().a, 0x5a);
    assert_eq!(machine.peek(0xa0_0101), Some(0x99));
}

pub fn indexed_rmw_keeps_read_result() {
    let mut machine = start(
        &[0xdd, 0x21, 0, 1, 0xdd, 0x34, 0, 0x76],
        12,
        &[(0x100, 0x7f)],
        &[(0x100, 0x55)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 34);
    assert_eq!(machine.peek(0xa0_0100), Some(0x7f));
    finish(&mut machine);
    assert_eq!(machine.peek(0xa0_0100), Some(0x80));
    assert_eq!(machine.sound_cpu().regs().f, 0x94);
}

pub fn indexed_immediate_write_is_latched() {
    let mut machine = start(
        &[0xdd, 0x21, 0, 1, 0xdd, 0x36, 0, 0x81, 0x76],
        9,
        &[],
        &[(7, 0x55), (0x100, 0x99)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 30);
    assert_eq!(machine.peek(0xa0_0100), Some(0));
    finish(&mut machine);
    assert_eq!(machine.peek(0xa0_0100), Some(0x81));
}

pub fn indexed_reset_and_write_fault() {
    let mut machine = start(&[0xdd, 0x21, 0, 1, 0xdd, 0x36, 0, 0x81, 0x76], 9, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 30);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0100), Some(0));
    assert!(!machine.sound_instruction_pending());
    let mut machine = start(&[0xfd, 0x21, 0, 0x80, 0xfd, 0x77, 0, 0x76], 9, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 30);
    let before = machine.sound_cpu().clone();
    let mut copied = machine.clone();
    let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x8000,
        direction: Direction::Write,
    }));
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(copied.step_instruction(), expected);
    assert_eq!(machine, copied);
    assert_eq!(machine.sound_cpu(), &before);
    assert!(machine.sound_prefix_pending());
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
}
