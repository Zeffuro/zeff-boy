use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn nibble_read_is_live_after_grant() {
    for (opcode, other, accumulator, memory) in [(0x67, 0x6f, 0x16, 0x25), (0x6f, 0x67, 0x15, 0x62)]
    {
        let mut machine = start(
            &[0x21, 0, 1, 0x3e, 0x12, 0xed, opcode, 0x76],
            7,
            &[(0x100, 0x34)],
            &[(6, other), (0x100, 0x56)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 25);
        assert_eq!(machine.sound_cpu().regs().a, 0x12);
        finish(&mut machine);
        assert_eq!(machine.sound_cpu().regs().a, accumulator);
        assert_eq!(machine.peek(0xa0_0100), Some(memory));
    }
}

pub fn nibble_grant_keeps_read_result() {
    for (opcode, accumulator, memory, flags) in [(0x67, 0x14, 0x23, 4), (0x6f, 0x13, 0x42, 0)] {
        let mut machine = start(
            &[0x21, 0, 1, 0x3e, 0x12, 0xed, opcode, 0x76],
            9,
            &[(0x100, 0x34)],
            &[(0x100, 0x99)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 28);
        assert_eq!(machine.sound_cpu().regs().a, 0x12);
        assert_eq!(machine.sound_cpu().regs().f, 0);
        finish(&mut machine);
        assert_eq!(machine.sound_cpu().regs().a, accumulator);
        assert_eq!(machine.sound_cpu().regs().f, flags);
        assert_eq!(machine.peek(0xa0_0100), Some(memory));
    }
}

pub fn nibble_reset_and_fault_keep_uncommitted_registers() {
    let mut machine = start(
        &[0x21, 0, 1, 0x3e, 0x12, 0xed, 0x67, 0x76],
        11,
        &[(0x100, 0x34)],
        &[],
    );
    assert_eq!(machine.sound_cpu().cycles(), 32);
    assert_eq!(machine.sound_cpu().regs().a, 0x12);
    assert_eq!(machine.sound_cpu().regs().f, 0);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0100), Some(0x34));
    assert!(!machine.sound_instruction_pending());
    let mut machine = start(&[0x21, 0, 0x80, 0x3e, 0x12, 0xed, 0x67, 0x76], 11, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 32);
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
    assert_eq!(machine.sound_cpu().regs().a, 0x12);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
}

pub fn arithmetic_and_special_internal_grants() {
    let mut machine = start(
        &[0x21, 0xff, 0x7f, 0x11, 1, 0, 0xed, 0x5a, 0x76],
        11,
        &[],
        &[],
    );
    assert_eq!(machine.sound_cpu().cycles(), 32);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x7fff);
    assert_eq!(machine.sound_cpu().regs().f, 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x8000);
    assert_eq!(machine.sound_cpu().regs().f, 0x94);
    let mut machine = start(&[0x3e, 0x80, 0xed, 0x4f, 0x76], 2, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 15);
    assert_eq!(machine.sound_cpu().regs().r, 3);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().r, 0x81);
}
