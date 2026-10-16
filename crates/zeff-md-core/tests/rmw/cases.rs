use crate::multi_transfer_cases as multi;
use multi::{finish, start};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{MachineFault, SoundFault};

pub fn rmw_grant_keeps_read_result() {
    for (opcode, nops, clocks, result) in [
        (&[0x34][..], 4, 18, 0x82),
        (&[0x35][..], 4, 18, 0x80),
        (&[0xcb, 6][..], 6, 22, 3),
    ] {
        let mut sound = vec![0x21, 0, 1];
        sound.extend_from_slice(opcode);
        sound.push(0x76);
        let mut machine = start(&sound, nops, &[(0x100, 0x81)], &[(0x100, 0x55)]);
        assert_eq!(machine.sound_cpu().cycles(), clocks);
        assert_eq!(machine.peek(0xa0_0100), Some(0x81));
        let before = machine.sound_cpu().clone();
        let mut copied = machine.clone();
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        assert_eq!(machine.sound_cpu(), &before);
        assert_eq!(machine.peek(0xa0_0100), Some(0x55));
        finish(&mut machine);
        assert_eq!(machine.peek(0xa0_0100), Some(result));
        assert!(!machine.sound_instruction_pending());
    }
}

pub fn rmw_reset_discards_write() {
    let mut machine = start(&[0x21, 0, 1, 0xcb, 6, 0x76], 6, &[(0x100, 0x81)], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 22);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.peek(0xa0_0100), Some(0x81));
    assert_eq!(machine.timing_snapshot().now(), now);
    assert!(!machine.sound_instruction_pending());
    assert_eq!(machine.sound_cpu().cycles(), 0);
    Reset::reset(&mut machine);
    assert_eq!(machine.peek(0xa0_0100), Some(0));
}

pub fn rmw_write_fault_keeps_read_phase() {
    let mut machine = start(&[0x21, 0, 0x80, 0x34, 0x76], 4, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 18);
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
    assert!(machine.sound_instruction_pending());
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), expected);
    assert_eq!(machine, saved);
    machine.reset_sound_cpu();
    assert!(!machine.sound_instruction_pending());
}
