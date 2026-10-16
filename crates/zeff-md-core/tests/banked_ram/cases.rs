use crate::multi_transfer_cases as transfer;

use transfer::fixture;
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::Emulator;

#[path = "lifecycle.rs"]
mod lifecycle;
pub use lifecycle::{
    banked_ram_grant_fault_clone_reset_and_guards,
    banked_ram_main_interleave_preserves_disjoint_writes,
};

fn stopped_ram(sound: &[u8], bank: u16, initial: &[(u32, u8)]) -> Emulator {
    let mut handler = Vec::new();
    for &(address, value) in initial {
        fixture::store_byte(&mut handler, address, value);
    }
    handler.extend_from_slice(&[0x4e72, 0x2000]);
    let mut machine = fixture::stopped_with_bank(sound, &handler, bank);
    fixture::wake(&mut machine);
    for _ in 0..initial.len() + 1 {
        fixture::instruction(&mut machine);
    }
    assert_eq!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Stopped
    );
    machine
}

pub fn banked_ram_increment_reads_ff() {
    let mut machine = stopped_ram(&[0x21, 0x04, 0x80, 0x34, 0x7e], 0x1ff, &[(0xff8004, 0xde)]);
    assert_eq!(machine.sound_bank(), 0x1ff);
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 10);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x8004);
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 11);
    assert_eq!(machine.peek(0xff8004), Some(0));
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
    assert_eq!(machine.sound_cpu().regs().a, 0xff);
    assert_eq!(machine.peek(0xff8004), Some(0));
}

pub fn banked_ram_all_banks_offsets_and_mirrors() {
    let mut rows = 0;
    for bank in 0x1c0..=0x1ff {
        for (offset, value) in [
            (0x8000u16, 0x55),
            (0x8001, 0xa6),
            (0xfffe, 0),
            (0xffff, 0xff),
        ] {
            let target = (u32::from(bank) << 15) | u32::from(offset & 0x7fff);
            let canonical = target & 0xffff;
            let left = 0xe00000 | canonical.wrapping_sub(1) & 0xffff;
            let right = 0xe00000 | canonical.wrapping_add(1) & 0xffff;
            let mut machine = stopped_ram(
                &[
                    0x21,
                    offset as u8,
                    (offset >> 8) as u8,
                    0x7e,
                    0x3e,
                    value,
                    0x77,
                    0x7e,
                ],
                bank,
                &[(target, 0xde), (left, 0x12), (right, 0x34)],
            );
            assert_eq!(machine.sound_bank(), bank);
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 10);
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
            assert_eq!(machine.sound_cpu().regs().a, 0xff);
            assert_eq!(machine.peek(target), Some(0xde));
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
            assert_eq!(machine.peek(target), Some(value));
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
            assert_eq!(machine.sound_cpu().regs().a, 0xff);
            for mirror in 0..32 {
                assert_eq!(
                    machine.peek(0xe00000 | (mirror << 16) | canonical),
                    Some(value)
                );
            }
            assert_eq!(machine.peek(left), Some(0x12));
            assert_eq!(machine.peek(right), Some(0x34));
            rows += 1;
        }
    }
    assert_eq!(rows, 256);
}

pub fn banked_ram_all_byte_values_read_ff() {
    let mut values = 0;
    for value in 0..=255 {
        let mut machine = stopped_ram(
            &[0x21, 0x04, 0x80, 0x3e, value, 0x77, 0x7e],
            0x1c1,
            &[(0xe08004, 0xde), (0xe08003, 0x12), (0xe08005, 0x34)],
        );
        for clocks in [10, 7, 7, 7] {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, clocks);
        }
        assert_eq!(machine.sound_cpu().regs().a, 0xff);
        assert_eq!(machine.peek(0xe08004), Some(value));
        assert_eq!(machine.peek(0xff8004), Some(value));
        assert_eq!(machine.peek(0xe08003), Some(0x12));
        assert_eq!(machine.peek(0xe08005), Some(0x34));
        values += 1;
    }
    assert_eq!(values, 256);
}

pub fn banked_ram_manual_two_writes_and_window_wrap() {
    for (bank, low, high, address) in [(0x1c0, 0x00, 0x80, 0xe00000), (0x1ff, 0xff, 0xff, 0xffffff)]
    {
        let mut machine = stopped_ram(
            &[0x21, 0x34, 0x12, 0x22, low, high],
            bank,
            &[(address, 0xde), (0xe00001, 0xa6)],
        );
        assert_eq!(machine.step_sound_instruction().unwrap().cycles, 10);
        assert_eq!(machine.peek(address), Some(0xde));
        let original_sound = machine.peek(fixture::SOUND_RAM);
        assert_eq!(machine.step_sound_instruction().unwrap().cycles, 16);
        assert_eq!(machine.peek(address), Some(0x34));
        if bank == 0x1c0 {
            assert_eq!(machine.peek(0xe00001), Some(0x12));
            assert_eq!(machine.peek(fixture::SOUND_RAM), original_sound);
        } else {
            assert_eq!(machine.peek(fixture::SOUND_RAM), Some(0x12));
            assert_eq!(machine.peek(0xe00001), Some(0xa6));
        }
    }
}
