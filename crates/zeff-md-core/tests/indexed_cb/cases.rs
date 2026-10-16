use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn indexed_cb_terminal_is_live_after_grant() {
    for prefix in [0xdd, 0xfd] {
        let mut machine = start(
            &[prefix, 0x21, 0, 1, prefix, 0xcb, 0, 0x06, 0x76],
            7,
            &[(0x100, 0x81), (0x101, 0x55)],
            &[(6, 1), (7, 0x86)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 25);
        assert_eq!(machine.sound_cpu().regs().pc, 7);
        assert!(machine.sound_prefix_pending());
        finish(&mut machine);
        assert_eq!(machine.peek(0xa0_0100), Some(0x80));
        assert_eq!(machine.peek(0xa0_0101), Some(0x55));
    }
}

pub fn indexed_cb_read_result_and_register_copy_are_latched() {
    for prefix in [0xdd, 0xfd] {
        let mut machine = start(
            &[prefix, 0x21, 0, 1, prefix, 0xcb, 0, 0, 0x76],
            12,
            &[(0x100, 0x81)],
            &[(7, 0xc0), (0x100, 0x55)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 34);
        assert_eq!(machine.sound_cpu().regs().pc, 8);
        assert_eq!(machine.sound_cpu().regs().b, 0);
        assert_eq!(machine.sound_cpu().regs().f, 0);
        assert_eq!(machine.peek(0xa0_0100), Some(0x81));
        finish(&mut machine);
        assert_eq!(machine.peek(0xa0_0100), Some(3));
        assert_eq!(machine.sound_cpu().regs().b, 3);
        assert_eq!(machine.sound_cpu().regs().f, 5);
    }
}

pub fn indexed_bit_reads_after_grant_without_register_copy() {
    let mut machine = start(
        &[0xfd, 0x21, 0, 0x28, 0xfd, 0xcb, 0, 0x77, 0x76],
        9,
        &[(0x800, 0x40)],
        &[(0x800, 0)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 30);
    assert_eq!(machine.sound_cpu().regs().pc, 8);
    assert_eq!(machine.sound_cpu().regs().f, 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().a, 0);
    assert_eq!(machine.sound_cpu().regs().f, 0x7c);
    assert_eq!(machine.peek(0xa0_0800), Some(0));
}

pub fn indexed_cb_reset_and_fault_preserve_progress() {
    let mut machine = start(
        &[0xdd, 0x21, 0, 1, 0xdd, 0xcb, 0, 0, 0x76],
        12,
        &[(0x100, 0x81)],
        &[],
    );
    assert_eq!(machine.sound_cpu().cycles(), 34);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0100), Some(0x81));
    assert!(!machine.sound_instruction_pending());
    let mut machine = start(&[0xfd, 0x21, 0, 0x80, 0xfd, 0xcb, 0, 0, 0x76], 12, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 34);
    let cpu = machine.sound_cpu().clone();
    let mut copied = machine.clone();
    let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x8000,
        direction: Direction::Write,
    }));
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(copied.step_instruction(), expected);
    assert_eq!(machine, copied);
    assert_eq!(machine.sound_cpu(), &cpu);
    assert_eq!(machine.sound_cpu().regs().b, 0);
    assert_eq!(machine.sound_cpu().regs().f, 0);
    assert!(machine.sound_instruction_pending());
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
    machine.reset_sound_cpu();
    assert!(!machine.sound_instruction_pending());
}
