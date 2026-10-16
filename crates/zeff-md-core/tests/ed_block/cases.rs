use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn block_read_grants_are_live() {
    for opcode in [0xa0, 0xa8, 0xa1, 0xa9] {
        let mut machine = start(
            &[0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xed, opcode, 0x76],
            14,
            &[(0x100, 0x5a)],
            &[(10, 0), (0x100, 0x66)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 38);
        assert_eq!(machine.sound_cpu().regs().bc(), 2);
        finish(&mut machine);
        assert_eq!(machine.sound_cpu().regs().bc(), 1);
        assert_eq!(
            machine.sound_cpu().regs().hl(),
            if opcode & 8 == 0 { 0x101 } else { 0xff }
        );
        if opcode & 1 == 0 {
            assert_eq!(machine.peek(0xa0_0200), Some(0x66));
        } else {
            assert_eq!(machine.sound_cpu().regs().f, 0x9e);
        }
    }
}

pub fn block_write_and_reset_keep_prior_byte() {
    for reset in [false, true] {
        let mut machine = start(
            &[0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xed, 0xa0, 0x76],
            17,
            &[(0x100, 0x5a)],
            &[(0x100, 0x66), (0x200, 0x99)],
        );
        assert_eq!(machine.sound_cpu().cycles(), 44);
        assert_eq!(machine.sound_cpu().regs().bc(), 2);
        assert_eq!(machine.sound_cpu().regs().hl(), 0x100);
        assert_eq!(machine.peek(0xa0_0200), Some(0x5a));
        if reset {
            let now = machine.timing_snapshot().now();
            machine.reset_sound_cpu();
            assert!(!machine.sound_instruction_pending());
            assert_eq!(machine.timing_snapshot().now(), now);
            assert_eq!(machine.peek(0xa0_0200), Some(0x5a));
        } else {
            finish(&mut machine);
            assert_eq!(machine.sound_cpu().regs().bc(), 1);
            assert_eq!(machine.sound_cpu().regs().hl(), 0x101);
            assert_eq!(machine.sound_cpu().regs().f, 0x2c);
            assert_eq!(machine.peek(0xa0_0200), Some(0x99));
        }
    }
}

pub fn block_failed_write_keeps_read_state() {
    for input in [false, true] {
        let mut machine = if input {
            start(
                &[0x21, 4, 0x40, 0x01, 2, 0x12, 0xed, 0xa2, 0x76],
                11,
                &[],
                &[],
            )
        } else {
            start(
                &[0x21, 0, 1, 0x11, 4, 0x40, 0x01, 2, 0, 0xed, 0xa0, 0x76],
                16,
                &[(0x100, 0x5a)],
                &[],
            )
        };
        assert_eq!(machine.sound_cpu().cycles(), if input { 33 } else { 41 });
        let cpu = machine.sound_cpu().clone();
        let mut copied = machine.clone();
        let expected = Err(MachineFault::Sound(SoundFault::UnsupportedAccess {
            address: 0x4004,
            direction: Direction::Write,
        }));
        assert_eq!(machine.step_instruction(), expected);
        assert_eq!(copied.step_instruction(), expected);
        assert_eq!(machine, copied);
        assert_eq!(machine.sound_cpu(), &cpu);
        let saved = machine.clone();
        assert_eq!(machine.step_instruction(), expected);
        assert_eq!(machine, saved);
    }
}

pub fn block_io_grants_hold_input_and_output() {
    for (opcode, flags) in [(0xa2, 0x13), (0xaa, 0x17), (0xa3, 4), (0xab, 0x11)] {
        let mut machine = start(
            &[0x21, 0, 1, 0x01, 2, 0x12, 0xed, opcode, 0x76],
            11,
            &[(0x100, 0x5a)],
            &[(7, 0), (0x100, 0x66)],
        );
        assert_eq!(
            machine.sound_cpu().cycles(),
            if opcode & 1 == 0 { 33 } else { 32 }
        );
        assert_eq!(machine.sound_cpu().regs().b, 0x12);
        assert_eq!(machine.sound_cpu().regs().f, 0);
        finish(&mut machine);
        assert_eq!(machine.sound_cpu().regs().b, 0x11);
        assert_eq!(
            machine.sound_cpu().regs().hl(),
            if opcode & 8 == 0 { 0x101 } else { 0xff }
        );
        assert_eq!(machine.sound_cpu().regs().f, flags);
        assert_eq!(
            machine.peek(0xa0_0100),
            Some(if opcode & 1 == 0 { 0xff } else { 0x66 })
        );
        assert!(machine.sound_fault().is_none());
    }
}

pub fn block_repeat_phase_keeps_base_effects() {
    let mut machine = start(
        &[0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xed, 0xb0, 0x76],
        19,
        &[(0x100, 0x5a), (0x101, 0x66)],
        &[(10, 0xa0), (0x200, 0x99)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 46);
    assert_eq!(machine.sound_cpu().regs().pc, 11);
    assert_eq!(machine.sound_cpu().regs().bc(), 1);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x101);
    assert_eq!(machine.sound_cpu().regs().de(), 0x201);
    assert_eq!(machine.sound_cpu().regs().f, 0x2c);
    assert!(machine.sound_instruction_pending());
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().bc(), 0);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x102);
    assert_eq!(machine.sound_cpu().regs().de(), 0x202);
    assert_eq!(machine.peek(0xa0_0200), Some(0x99));
    assert_eq!(machine.peek(0xa0_0201), Some(0x66));
}
