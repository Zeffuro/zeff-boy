pub(crate) use crate::bank_reservation::cases as bank;

use bank::fixture::{BUS_REQUEST, SOUND_RAM, instruction, store_byte, store_word};
pub use bank::{WORK, WORK_BANK, finish, nop, start};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::{Emulator, MachineFault, SoundFault};
use zeff_z80::CpuState;

const LIMIT: usize = 192;
const WORD: u16 = 0x1234;
const ROM_STACK: u16 = 0x8800;

pub fn push_start(pair: u8) -> Emulator {
    assert!(pair < 4);
    let mut sound = vec![0x31, 2, 0x80];
    if pair == 3 {
        sound.extend_from_slice(&[0x3e, 0x12, 0xb7]);
    } else {
        sound.extend_from_slice(&[0x01 + (pair << 4), 0x34, 0x12]);
    }
    sound.extend_from_slice(&[0xc5 + (pair << 4), 0x76]);
    start(&sound, WORK_BANK, &[], true)
}

pub fn pair_value(machine: &Emulator, pair: u8) -> u16 {
    let regs = machine.sound_cpu().regs();
    match pair {
        0 => regs.bc(),
        1 => regs.de(),
        2 => regs.hl(),
        3 => u16::from_be_bytes([regs.a, regs.f]),
        _ => panic!("stack pair"),
    }
}

pub fn pop_start(pair: u8, ram: bool) -> Emulator {
    assert!(pair < 4);
    let address = if ram { 0x8000u16 } else { ROM_STACK };
    let [low, high] = address.to_le_bytes();
    start(
        &[0x31, low, high, 0xc1 + (pair << 4), 0x76],
        if ram { WORK_BANK } else { 0 },
        &[],
        true,
    )
}

pub fn ordinary_push_pairs_reserve_both_writes() {
    for pair in 0..4 {
        let mut machine = push_start(pair);
        assert!(finish(&mut machine) > 0);
        assert!(machine.sound_cpu().cycles() > if pair == 3 { 36 } else { 35 });
        let expected = if pair == 3 { 0x1204 } else { WORD };
        assert_eq!(pair_value(&machine, pair), expected);
        assert_eq!(machine.sound_cpu().regs().sp, 0x8000);
        assert_eq!(machine.peek(WORK), Some(expected as u8));
        assert_eq!(machine.peek(WORK + 1), Some((expected >> 8) as u8));
        assert_eq!(machine.peek(WORK + 2), Some(0xc8));
    }
}

pub fn ordinary_pop_pairs_reserve_both_reads() {
    for pair in 0..4 {
        for ram in [false, true] {
            let mut machine = pop_start(pair, ram);
            assert!(finish(&mut machine) > 0);
            assert!(machine.sound_cpu().cycles() > 24);
            assert_eq!(pair_value(&machine, pair), if ram { 0xffff } else { WORD });
            assert_eq!(
                machine.sound_cpu().regs().sp,
                if ram { 0x8002 } else { 0x8802 }
            );
            assert_eq!(machine.peek(WORK), Some(0xa6));
            assert_eq!(machine.peek(WORK + 1), Some(0xb7));
        }
    }
}

pub fn indexed_start(prefix: u8, write: bool) -> Emulator {
    assert!(matches!(prefix, 0xdd | 0xfd));
    if write {
        start(
            &[0x31, 2, 0x80, prefix, 0x21, 0x34, 0x12, prefix, 0xe5, 0x76],
            WORK_BANK,
            &[],
            true,
        )
    } else {
        start(&[0x31, 0, 0x88, prefix, 0xe1, 0x76], 0, &[], true)
    }
}

pub fn indexed_push_pop_reserve_bank_data() {
    for prefix in [0xdd, 0xfd] {
        for write in [false, true] {
            let mut machine = indexed_start(prefix, write);
            assert!(finish(&mut machine) > 0);
            assert!(machine.sound_cpu().cycles() > if write { 43 } else { 28 });
            assert_eq!(bank::indexed_pair_value(&machine, prefix), WORD);
            assert_eq!(bank::indexed_pair_value(&machine, prefix ^ 0x20), 0);
            assert_eq!(machine.sound_cpu().regs().hl(), 0);
            assert_eq!(
                machine.sound_cpu().regs().sp,
                if write { 0x8000 } else { 0x8802 }
            );
            assert_eq!(machine.peek(WORK), Some(if write { 0x34 } else { 0xa6 }));
            assert_eq!(
                machine.peek(WORK + 1),
                Some(if write { 0x12 } else { 0xb7 })
            );
        }
    }
}

pub fn exchange_start(prefix: Option<u8>) -> Emulator {
    let mut sound = vec![0x31, 0, 0x80];
    if let Some(prefix) = prefix {
        sound.push(prefix);
    }
    sound.extend_from_slice(&[0x21, 0x34, 0x12]);
    if let Some(prefix) = prefix {
        sound.push(prefix);
    }
    sound.extend_from_slice(&[0xe3, 0x76]);
    start(&sound, WORK_BANK, &[], true)
}

pub fn exchange_reserves_fixed_phases_and_retains_mixed_tails() {
    // EX read-high (4T) and write-low (5T) remain unqualified mixed tails.
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        let mut machine = exchange_start(prefix);
        assert!(finish(&mut machine) > 0);
        assert!(machine.sound_cpu().cycles() > if prefix.is_some() { 51 } else { 43 });
        let value = prefix.map_or_else(
            || machine.sound_cpu().regs().hl(),
            |p| bank::indexed_pair_value(&machine, p),
        );
        assert_eq!(value, 0xffff);
        assert_eq!(machine.sound_cpu().regs().sp, 0x8000);
        assert_eq!(machine.peek(WORK), Some(0x34));
        assert_eq!(machine.peek(WORK + 1), Some(0x12));
        assert_eq!(machine.peek(WORK + 2), Some(0xc8));
    }
}

pub fn consecutive_push_start() -> Emulator {
    start(
        &[0x31, 4, 0x80, 0x21, 0x34, 0x12, 0xe5, 0xe5, 0x76],
        WORK_BANK,
        &[],
        true,
    )
}

pub fn consecutive_pushes_preserve_order_and_main_progress() {
    let mut machine = consecutive_push_start();
    let mut copied = machine.clone();
    let mut waits = 0;
    let mut progress = 0;
    for _ in 0..LIMIT {
        if machine.sound_cpu().state() == CpuState::Halted {
            break;
        }
        let before = machine.cpu_clock().0;
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        waits += machine.cpu_clock().0 - before - 4;
        progress += 1;
    }
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    assert!(waits > 0);
    assert!(progress > 1);
    assert_eq!(machine.sound_cpu().regs().sp, 0x8000);
    for (offset, value) in [(0, 0x34), (1, 0x12), (2, 0x34), (3, 0x12)] {
        assert_eq!(machine.peek(WORK + offset), Some(value));
    }
}

pub fn pending_stack_clone_and_reset_preserve_prior_write() {
    for prior_write in [false, true] {
        let mut machine = push_start(2);
        for _ in 0..LIMIT {
            let high_written = machine.peek(WORK + 1) == Some(0x12);
            if machine.sound_cpu().regs().pc == 7
                && machine.sound_cpu().regs().sp == if prior_write { 0x8000 } else { 0x8001 }
                && machine.peek(WORK) == Some(0xa6)
                && high_written == prior_write
                && machine.sound_instruction_pending()
            {
                break;
            }
            nop(&mut machine);
        }
        assert_eq!(machine.peek(WORK), Some(0xa6));
        assert_eq!(
            machine.peek(WORK + 1),
            Some(if prior_write { 0x12 } else { 0xb7 })
        );
        assert!(machine.sound_instruction_pending());
        assert_eq!(
            machine.sound_cpu().regs().sp,
            if prior_write { 0x8000 } else { 0x8001 }
        );
        assert!(machine.sound_deadline().is_some());
        let mut copied = machine.clone();
        let now = machine.timing_snapshot();
        machine.reset_sound_cpu();
        assert_eq!(machine.timing_snapshot(), now);
        assert_eq!(machine.sound_cpu().cycles(), 0);
        assert_eq!(machine.sound_deadline(), None);
        assert!(!machine.sound_instruction_pending());
        assert_eq!(machine.peek(WORK), Some(0xa6));
        assert_eq!(
            machine.peek(WORK + 1),
            Some(if prior_write { 0x12 } else { 0xb7 })
        );
        finish(&mut copied);
        assert_eq!(copied.peek(WORK), Some(0x34));
        assert_eq!(copied.peek(WORK + 1), Some(0x12));
        Reset::reset(&mut machine);
        assert_eq!(machine.peek(WORK), Some(0));
        assert_eq!(machine.peek(SOUND_RAM), Some(0));
    }
}

pub fn stack_grant_preserves_latched_value() {
    let sound = [0x31, 2, 0x80, 0x21, 0x34, 0x12, 0xe5, 0x76];
    let mut tail = vec![0x4e71; 8];
    store_word(&mut tail, BUS_REQUEST, 0x100);
    store_byte(&mut tail, SOUND_RAM + 4, 0x78);
    store_byte(&mut tail, SOUND_RAM + 5, 0x56);
    store_byte(&mut tail, SOUND_RAM + 6, 0);
    store_word(&mut tail, BUS_REQUEST, 0);
    let mut machine = start(&sound, WORK_BANK, &tail, true);
    for _ in 0..9 {
        instruction(&mut machine);
    }
    assert!(machine.sound_bus_granted());
    assert!(machine.sound_instruction_pending());
    assert_eq!(machine.sound_cpu().regs().pc, 7);
    assert_eq!(machine.sound_cpu().regs().sp, 0x8001);
    assert_eq!(machine.peek(WORK), Some(0xa6));
    assert_eq!(machine.peek(WORK + 1), Some(0xb7));
    let saved = machine.sound_cpu().clone();
    assert_eq!(saved.regs().hl(), WORD);
    let prior = (machine.peek(WORK), machine.peek(WORK + 1));
    for _ in 0..3 {
        instruction(&mut machine);
        assert_eq!(machine.sound_cpu(), &saved);
        assert_eq!((machine.peek(WORK), machine.peek(WORK + 1)), prior);
    }
    instruction(&mut machine);
    finish(&mut machine);
    assert_eq!(machine.peek(WORK), Some(0x34));
    assert_eq!(machine.peek(WORK + 1), Some(0x12));
    assert_eq!(machine.sound_cpu().regs().hl(), WORD);
}

pub fn fault_start() -> Emulator {
    start(
        &[0x31, 1, 0x80, 0x21, 0x34, 0x12, 0xe5],
        WORK_BANK,
        &[],
        true,
    )
}

pub fn stack_fault() -> MachineFault {
    MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x7fff,
        direction: Direction::Write,
    })
}

pub fn stack_fault_retains_prior_bank_write() {
    let mut machine = fault_start();
    let expected = stack_fault();
    let mut failure = None;
    for _ in 0..LIMIT {
        if let Err(fault) = machine.step_instruction() {
            failure = Some(fault);
            break;
        }
    }
    assert_eq!(failure, Some(expected.clone()));
    assert_eq!(machine.peek(WORK), Some(0x12));
    assert_eq!(machine.peek(WORK + 1), Some(0xb7));
    assert_eq!(machine.sound_cpu().regs().sp, 0x7fff);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected));
    assert_eq!(machine, saved);
    machine.reset_sound_cpu();
    assert_eq!(machine.sound_fault(), None);
    assert_eq!(machine.peek(WORK), Some(0x12));
}

pub fn manual_and_local_stack_remain_nominal() {
    for (sound, clocks) in [
        (
            &[0x31, 2, 0x80, 0x21, 0x34, 0x12, 0xe5][..],
            &[10, 10, 11][..],
        ),
        (&[0x31, 0, 0x88, 0xc1][..], &[10, 10][..]),
        (
            &[0x31, 0, 0x80, 0x21, 0x34, 0x12, 0xe3][..],
            &[10, 10, 19][..],
        ),
    ] {
        let mut machine = start(sound, WORK_BANK, &[0x4e72, 0x2000], false);
        instruction(&mut machine);
        assert_eq!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Stopped
        );
        for &expected in clocks {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, expected);
        }
        assert_eq!(
            machine.sound_cpu().cycles(),
            clocks.iter().copied().map(u64::from).sum::<u64>()
        );
    }
    let mut local = start(
        &[0x31, 2, 1, 0x21, 0x34, 0x12, 0xe5, 0xd1, 0x76],
        WORK_BANK,
        &[],
        true,
    );
    assert_eq!(finish(&mut local), 0);
    assert_eq!(local.sound_cpu().cycles(), 45);
    assert_eq!(local.sound_cpu().regs().de(), WORD);
    assert_eq!(local.peek(SOUND_RAM + 0x100), Some(0x34));
    assert_eq!(local.peek(SOUND_RAM + 0x101), Some(0x12));
    assert_eq!(local.peek(WORK), Some(0xa6));
}

pub fn control_start(row: u8) -> Emulator {
    let (initial, data, bank) = match row {
        0 => (&[0x31, 0, 0x88, 0xc9][..], &[0x10, 0][..], 0),
        1 => (&[0x31, 2, 0x80, 0xff][..], &[0x34, 0x12][..], WORK_BANK),
        2 => (
            &[0x31, 2, 0x80, 0xcd, 0x10, 0][..],
            &[0x34, 0x12][..],
            WORK_BANK,
        ),
        3 => (&[0xc3, 0, 0x88][..], &[0xc3, 0x10, 0][..], 0),
        4 => (&[0x31, 2, 1, 0xc3, 0, 0x88][..], &[0xcd, 0x10, 0][..], 0),
        5 => (&[0xc3, 0, 0x88][..], &[0xc2, 0x10, 0][..], 0),
        6 => (
            &[0x31, 2, 1, 0xc3, 0, 0x88][..],
            &[0xcc, 0x10, 0, 0x76][..],
            0,
        ),
        _ => panic!("control row"),
    };
    let mut sound = initial.to_vec();
    sound.resize(0x39, 0);
    sound[0x10] = 0x76;
    sound[0x38] = 0x76;
    bank::start_with_rom_data(&sound, bank, &[], true, data)
}

pub fn control_nominal(row: u8) -> u64 {
    match row {
        0 => 24,
        1 => 25,
        2 => 31,
        3 | 5 => 24,
        4 => 41,
        6 => 34,
        _ => panic!("control row"),
    }
}

pub fn check_control(machine: &Emulator, row: u8) {
    let regs = machine.sound_cpu().regs();
    assert_eq!(
        regs.pc,
        if row == 1 {
            0x39
        } else if row == 6 {
            0x8804
        } else {
            0x11
        }
    );
    match row {
        0 => assert_eq!(regs.sp, 0x8802),
        1 | 2 => {
            assert_eq!(regs.sp, 0x8000);
            assert_eq!(machine.peek(WORK), Some(if row == 1 { 4 } else { 6 }));
            assert_eq!(machine.peek(WORK + 1), Some(0));
        }
        4 => {
            assert_eq!(regs.sp, 0x100);
            assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(3));
            assert_eq!(machine.peek(SOUND_RAM + 0x101), Some(0x88));
        }
        6 => assert_eq!(regs.sp, 0x102),
        _ => {}
    }
}

pub fn return_restart_call_and_jump_reserve_bank_cycles() {
    for row in 0..7 {
        let mut machine = control_start(row);
        assert!(finish(&mut machine) > 0);
        assert!(machine.sound_cpu().cycles() > control_nominal(row));
        check_control(&machine, row);
    }
}
