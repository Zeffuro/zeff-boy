pub(crate) use crate::sound_fixture as fixture;

use fixture::{BUS_REQUEST, SOUND_RAM, image, instruction, load_program, store_byte, store_word};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::{Emulator, LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

pub const WORK: u32 = 0xe0_0000;
pub const WORK_BANK: u16 = 0x1c0;
pub const VALUE: u8 = 0x5a;
const ROM_DATA: usize = 0x800;
const LIMIT: usize = 192;

pub fn start(sound: &[u8], bank: u16, tail: &[u16], concurrent: bool) -> Emulator {
    start_with_rom_data(sound, bank, tail, concurrent, &[0x34, 0x12])
}

pub fn start_with_rom_data(
    sound: &[u8],
    bank: u16,
    tail: &[u16],
    concurrent: bool,
    data: &[u8],
) -> Emulator {
    let mut program = load_program(sound);
    for bit in 0..9 {
        store_byte(&mut program, SOUND_RAM + 0x6000, ((bank >> bit) & 1) as u8);
    }
    for (address, value) in [(WORK, 0xa6), (WORK + 1, 0xb7), (WORK + 2, 0xc8)] {
        store_byte(&mut program, address, value);
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(tail);
    program.extend_from_slice(&[0x4e71; LIMIT]);
    program.extend_from_slice(&[0x60fe, 0x4e71]);
    let mut rom = image(&program, &[]);
    rom[ROM_DATA..ROM_DATA + data.len()].copy_from_slice(data);
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            concurrent_sound: concurrent,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + 15 {
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_bank(), bank);
    assert!(!machine.sound_reset_asserted());
    assert!(!machine.sound_bus_requested());
    machine
}

pub fn nop(machine: &mut Emulator) -> u64 {
    let before = machine.cpu_clock().0;
    let step = machine.step_instruction().unwrap();
    let TimedOutcome::Instruction(cpu) = step.cpu else {
        panic!("main instruction")
    };
    assert_eq!(cpu.opcode, 0x4e71);
    assert_eq!(cpu.clocks, machine.cpu_clock().0 - before);
    assert_eq!(step.elapsed.get(), cpu.clocks * 7);
    cpu.clocks - 4
}

pub fn finish(machine: &mut Emulator) -> u64 {
    let mut waits = 0;
    for _ in 0..LIMIT {
        if machine.sound_cpu().state() == CpuState::Halted {
            return waits;
        }
        waits += nop(machine);
    }
    panic!("sound failed to halt");
}

pub fn load_immediate_bank_write() {
    let mut machine = start(&[0x21, 0, 0x80, 0x36, VALUE, 0x76], WORK_BANK, &[], true);
    assert!(finish(&mut machine) > 0);
    assert!(machine.sound_cpu().cycles() > 24);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x8000);
    assert_eq!(machine.peek(WORK), Some(VALUE));
    assert_eq!(machine.peek(WORK + 1), Some(0xb7));
}

pub fn load_absolute_byte_write() {
    let mut machine = start(&[0x3e, VALUE, 0x32, 0, 0x80, 0x76], WORK_BANK, &[], true);
    assert!(finish(&mut machine) > 0);
    assert!(machine.sound_cpu().cycles() > 24);
    assert_eq!(machine.sound_cpu().regs().a, VALUE);
    assert_eq!(machine.peek(WORK), Some(VALUE));
    assert_eq!(machine.peek(WORK + 1), Some(0xb7));
}

pub fn load_absolute_word_write() {
    let mut machine = start(
        &[0x21, 0x34, 0x12, 0x22, 0, 0x80, 0x76],
        WORK_BANK,
        &[],
        true,
    );
    assert!(finish(&mut machine) > 0);
    assert!(machine.sound_cpu().cycles() > 30);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x1234);
    assert_eq!(machine.peek(WORK), Some(0x34));
    assert_eq!(machine.peek(WORK + 1), Some(0x12));
    assert_eq!(machine.peek(WORK + 2), Some(0xc8));
}

pub fn load_absolute_byte_read() {
    let mut machine = start(&[0x3a, 0, 0x88, 0x76], 0, &[], true);
    assert!(finish(&mut machine) > 0);
    assert!(machine.sound_cpu().cycles() > 17);
    assert_eq!(machine.sound_cpu().regs().a, 0x34);
    assert_eq!(machine.peek(ROM_DATA as u32), Some(0x34));
    let mut machine = start(&[0x3a, 0, 0x80, 0x76], WORK_BANK, &[], true);
    assert!(finish(&mut machine) > 0);
    assert_eq!(machine.sound_cpu().regs().a, 0xff);
    assert_eq!(machine.peek(WORK), Some(0xa6));
}

pub fn load_absolute_word_read() {
    let mut machine = start(&[0x2a, 0, 0x88, 0x76], 0, &[], true);
    assert!(finish(&mut machine) > 0);
    assert!(machine.sound_cpu().cycles() > 20);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x1234);
    let mut machine = start(&[0x2a, 0, 0x80, 0x76], WORK_BANK, &[], true);
    assert!(finish(&mut machine) > 0);
    assert_eq!(machine.sound_cpu().regs().hl(), 0xffff);
    assert_eq!(machine.peek(WORK), Some(0xa6));
    assert_eq!(machine.peek(WORK + 1), Some(0xb7));
}

pub fn ed_start(pair: u8, write: bool) -> Emulator {
    assert!(pair < 4);
    let offset = pair << 4;
    if write {
        start(
            &[
                0x01 + offset,
                0x34,
                0x12,
                0xed,
                0x43 + offset,
                0,
                0x80,
                0x76,
            ],
            WORK_BANK,
            &[],
            true,
        )
    } else {
        start(&[0xed, 0x4b + offset, 0, 0x88, 0x76], 0, &[], true)
    }
}

pub fn ed_pair_value(machine: &Emulator, pair: u8) -> u16 {
    let regs = machine.sound_cpu().regs();
    match pair {
        0 => regs.bc(),
        1 => regs.de(),
        2 => regs.hl(),
        3 => regs.sp,
        _ => panic!("register pair"),
    }
}

pub fn ed_absolute_pairs_reserve_reads_and_writes() {
    let mut rows = 0;
    for pair in 0..4 {
        for write in [false, true] {
            let mut machine = ed_start(pair, write);
            assert!(finish(&mut machine) > 0);
            assert!(machine.sound_cpu().cycles() > if write { 34 } else { 24 });
            assert_eq!(ed_pair_value(&machine, pair), 0x1234);
            assert_eq!(machine.peek(WORK), Some(if write { 0x34 } else { 0xa6 }));
            assert_eq!(
                machine.peek(WORK + 1),
                Some(if write { 0x12 } else { 0xb7 })
            );
            assert_eq!(machine.peek(WORK + 2), Some(0xc8));
            rows += 1;
        }
    }
    assert_eq!(rows, 8);
}

pub fn indexed_start(prefix: u8, write: bool) -> Emulator {
    assert!(matches!(prefix, 0xdd | 0xfd));
    if write {
        start(
            &[prefix, 0x21, 0x34, 0x12, prefix, 0x22, 0, 0x80, 0x76],
            WORK_BANK,
            &[],
            true,
        )
    } else {
        start(
            &[prefix, 0x21, 0xb7, 0xa6, prefix, 0x2a, 0, 0x88, 0x76],
            0,
            &[],
            true,
        )
    }
}

pub fn indexed_pair_value(machine: &Emulator, prefix: u8) -> u16 {
    let regs = machine.sound_cpu().regs();
    match prefix {
        0xdd => regs.ix,
        0xfd => regs.iy,
        _ => panic!("index prefix"),
    }
}

pub fn indexed_absolute_pairs_reserve_reads_and_writes() {
    let mut rows = 0;
    for prefix in [0xdd, 0xfd] {
        for write in [false, true] {
            let mut machine = indexed_start(prefix, write);
            assert!(finish(&mut machine) > 0);
            assert!(machine.sound_cpu().cycles() > 38);
            assert_eq!(indexed_pair_value(&machine, prefix), 0x1234);
            assert_eq!(indexed_pair_value(&machine, prefix ^ 0x20), 0);
            assert_eq!(machine.sound_cpu().regs().hl(), 0);
            assert_eq!(machine.peek(WORK), Some(if write { 0x34 } else { 0xa6 }));
            assert_eq!(
                machine.peek(WORK + 1),
                Some(if write { 0x12 } else { 0xb7 })
            );
            assert_eq!(machine.peek(WORK + 2), Some(0xc8));
            rows += 1;
        }
    }
    assert_eq!(rows, 4);
}

pub fn default_manual_loads_remain_nominal() {
    for (sound, bank, clocks) in [
        (&[0x21, 0, 0x80, 0x36, VALUE][..], WORK_BANK, &[10, 10][..]),
        (
            &[0x21, 0x34, 0x12, 0x22, 0, 0x80][..],
            WORK_BANK,
            &[10, 16][..],
        ),
        (&[0x3e, VALUE, 0x32, 0, 0x80][..], WORK_BANK, &[7, 13][..]),
        (&[0x3a, 0, 0x88][..], 0, &[13][..]),
        (&[0x2a, 0, 0x88][..], 0, &[16][..]),
    ] {
        let mut machine = start(sound, bank, &[0x4e72, 0x2000], false);
        instruction(&mut machine);
        assert_eq!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Stopped
        );
        let before = machine.timing_snapshot().now().get();
        for &expected in clocks {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, expected);
        }
        let total = clocks.iter().copied().map(u64::from).sum::<u64>();
        assert_eq!(machine.sound_cpu().cycles(), total);
        assert_eq!(
            machine.timing_snapshot().now().get(),
            before / 15 * 15 + total * 15
        );
    }
    assert!(!LoadConfig::default().concurrent_sound);
}

pub fn local_load_and_unclassified_stack_remain_nominal() {
    local_load_and_stack_timing();
}

pub fn local_load_and_stack_timing() {
    let mut local = start(&[0x21, 0, 1, 0x36, VALUE, 0x76], WORK_BANK, &[], true);
    assert_eq!(finish(&mut local), 0);
    assert_eq!(local.sound_cpu().cycles(), 24);
    assert_eq!(local.peek(SOUND_RAM + 0x100), Some(VALUE));
    assert_eq!(local.peek(WORK), Some(0xa6));
    let mut stack = start(
        &[0x31, 2, 0x80, 0x21, 0x34, 0x12, 0xe5, 0x76],
        WORK_BANK,
        &[],
        true,
    );
    assert!(finish(&mut stack) > 0);
    assert!(stack.sound_cpu().cycles() > 35);
    assert_eq!(stack.sound_cpu().regs().sp, 0x8000);
    assert_eq!(stack.peek(WORK), Some(0x34));
    assert_eq!(stack.peek(WORK + 1), Some(0x12));
}

pub fn consecutive_bank_segments_make_progress() {
    let sound = [0x21, 0x34, 0x12, 0x22, 0, 0x80, 0x22, 2, 0x80, 0x76];
    let mut machine = start(&sound, WORK_BANK, &[], true);
    let mut copied = machine.clone();
    let mut waits = 0;
    for _ in 0..LIMIT {
        if machine.sound_cpu().state() == CpuState::Halted {
            break;
        }
        let before = machine.cpu_clock().0;
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        waits += machine.cpu_clock().0 - before - 4;
    }
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    assert!(waits > 0);
    for (offset, value) in [(0, 0x34), (1, 0x12), (2, 0x34), (3, 0x12)] {
        assert_eq!(machine.peek(WORK + offset), Some(value));
    }
}

pub fn pending_bank_clone_and_reset() {
    let mut machine = start(&[0x3e, VALUE, 0x32, 0, 0x80, 0x76], WORK_BANK, &[], true);
    for _ in 0..LIMIT {
        if machine.sound_cpu().regs().pc == 5 && machine.peek(WORK) == Some(0xa6) {
            break;
        }
        nop(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    assert!(machine.sound_instruction_pending());
    let deadline = machine.sound_deadline().unwrap();
    assert!(deadline.get() > machine.timing_snapshot().now().get());
    let mut copied = machine.clone();
    let now = machine.timing_snapshot();
    machine.reset_sound_cpu();
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.sound_deadline(), None);
    assert!(!machine.sound_instruction_pending());
    assert_eq!(machine.timing_snapshot(), now);
    assert_eq!(machine.peek(WORK), Some(0xa6));
    assert!(finish(&mut copied) > 0);
    assert_eq!(copied.peek(WORK), Some(VALUE));
    Reset::reset(&mut machine);
    assert_eq!(machine.peek(WORK), Some(0));
    assert_eq!(machine.peek(SOUND_RAM), Some(0));
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.sound_deadline(), None);
}

pub fn grant_preserves_latched_address_and_prior_write() {
    let sound = [0x21, 0x34, 0x12, 0x22, 0, 0x80, 0x76];
    let mut tail = vec![0x4e71; 7];
    store_word(&mut tail, BUS_REQUEST, 0x100);
    tail.extend_from_slice(&[0x1e39, 0x00a0, 0x0100]);
    for (address, value) in [(4, 0x40), (5, 0x81)] {
        store_byte(&mut tail, SOUND_RAM + address, value);
    }
    store_word(&mut tail, BUS_REQUEST, 0);
    let mut machine = start(&sound, WORK_BANK, &tail, true);
    for _ in 0..9 {
        instruction(&mut machine);
    }
    assert!(machine.sound_bus_requested());
    assert!(machine.sound_bus_granted());
    let saved_sound = machine.sound_cpu().clone();
    let prior = machine.peek(WORK);
    for _ in 0..2 {
        instruction(&mut machine);
        assert_eq!(machine.sound_cpu(), &saved_sound);
        assert_eq!(machine.peek(WORK), prior);
    }
    instruction(&mut machine);
    finish(&mut machine);
    assert_eq!(machine.peek(WORK), Some(0x34));
    assert_eq!(machine.peek(WORK + 1), Some(0x12));
    assert_eq!(machine.peek(WORK + 0x140), Some(0));
    assert_eq!(machine.peek(WORK + 0x141), Some(0));
}

pub fn fault_retains_prior_reserved_read_and_partial_write() {
    let sound = [0x3a, 0, 0x88, 0x32, 0, 1, 0x21, 0x34, 0x12, 0x22, 3, 0x40];
    let mut machine = start(&sound, 0, &[], true);
    let expected = MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x4004,
        direction: Direction::Write,
    });
    let mut failure = None;
    for _ in 0..LIMIT {
        if let Err(fault) = machine.step_instruction() {
            failure = Some(fault);
            break;
        }
    }
    assert_eq!(failure, Some(expected.clone()));
    assert_eq!(machine.sound_cpu().regs().a, 0x34);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x34));
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected));
    assert_eq!(machine, saved);
    machine.reset_sound_cpu();
    assert_eq!(machine.sound_fault(), None);
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
}
