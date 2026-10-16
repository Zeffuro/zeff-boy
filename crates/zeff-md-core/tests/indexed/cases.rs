use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

fn start(sound: &[u8], tail: &[u16], data: &[(u32, u8)]) -> Emulator {
    let mut main = load_program(sound);
    for &(address, byte) in data {
        store_byte(&mut main, SOUND_RAM + address, byte);
    }
    store_word(&mut main, BUS_REQUEST, 0);
    main.extend_from_slice(tail);
    main.extend(std::iter::repeat_n(0x4e71, 120));
    let mut machine = Emulator::new(
        image(&main, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + data.len() + 3 {
        instruction(&mut machine);
    }
    machine
}

fn until_halt(machine: &mut Emulator) {
    for _ in 0..128 {
        if machine.sound_cpu().state() == CpuState::Halted {
            return;
        }
        instruction(machine);
    }
    panic!("sound did not halt");
}

pub fn indexed_register_memory_and_cb() {
    let mut machine = start(
        &[
            0xdd, 0x21, 0, 1, 0xfd, 0x21, 0, 2, 0xdd, 0x36, 0xff, 0x81, 0xdd, 0xcb, 0xff, 0, 0xfd,
            0x70, 1, 0x76,
        ],
        &[],
        &[],
    );
    until_halt(&mut machine);
    assert_eq!(machine.sound_cpu().regs().ix, 0x100);
    assert_eq!(machine.sound_cpu().regs().iy, 0x200);
    assert_eq!(machine.sound_cpu().regs().b, 3);
    assert_eq!(machine.peek(SOUND_RAM + 0xff), Some(3));
    assert_eq!(machine.peek(SOUND_RAM + 0x201), Some(3));
}

pub fn prefix_grant_observes_modified_terminal() {
    let sound = [0xdd; 32];
    let mut tail = Vec::new();
    store_word(&mut tail, BUS_REQUEST, 0x100);
    store_byte(&mut tail, SOUND_RAM + 3, 0);
    store_word(&mut tail, BUS_REQUEST, 0);
    let mut machine = start(&sound, &tail, &[]);
    instruction(&mut machine);
    instruction(&mut machine);
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.sound_cpu().regs().pc, 3);
    assert!(machine.sound_prefix_pending());
    assert_eq!(machine.peek(SOUND_RAM + 3), Some(0));
    let before = machine.sound_cpu().cycles();
    let mut copied = machine.clone();
    for _ in 0..2 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    assert!(!machine.sound_prefix_pending());
    assert_eq!(machine.sound_cpu().regs().pc, 4);
    assert_eq!(machine.sound_cpu().cycles(), before + 4);
}

pub fn pending_prefix_clone_and_reset() {
    let mut machine = start(&[0xdd, 0xfd, 0x21, 0x34, 0x12, 0x76], &[], &[]);
    assert!(machine.sound_deadline().is_some());
    let mut copied = machine.clone();
    for _ in 0..4 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    machine.set_interrupt_level(4);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.interrupt_level(), 4);
    assert!(!machine.sound_prefix_pending());
    assert!(machine.sound_deadline().is_none());
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.peek(SOUND_RAM), Some(0xdd));
    Reset::reset(&mut machine);
    assert!(machine.sound_reset_asserted());
    assert_eq!(machine.peek(SOUND_RAM), Some(0));
}

pub fn terminal_fault_preserves_prefixes() {
    let mut machine = start(&[0xdd, 0x21, 0x34, 0x12, 0xdd, 0x22, 3, 0x40], &[], &[]);
    let expected = SoundFault::UnsupportedAccess {
        address: 0x4004,
        direction: Direction::Write,
    };
    let mut error = None;
    for _ in 0..16 {
        if let Err(fault) = machine.step_instruction() {
            error = Some(fault);
            break;
        }
    }
    assert_eq!(error, Some(MachineFault::Sound(expected)));
    assert_eq!(machine.sound_cpu().regs().ix, 0x1234);
    assert_eq!(machine.sound_cpu().regs().pc, 8);
    assert_eq!(machine.sound_cpu().cycles(), 31);
    assert!(machine.sound_prefix_pending());
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Sound(expected))
    );
    assert_eq!(machine, saved);
}

pub fn retained_prefix_input_and_ignored_ed() {
    let mut machine = start(&[0x3e, 0x55, 0xdd], &[], &[]);
    for _ in 0..4 {
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().a, 0x55);
    assert!(machine.sound_fault().is_none());
    let mut machine = start(
        &[0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xdd, 0xed, 0xb0, 0x76],
        &[],
        &[(0x100, 0x55), (0x101, 0x66)],
    );
    until_halt(&mut machine);
    assert_eq!(machine.sound_cpu().regs().bc(), 0);
    assert_eq!(machine.peek(SOUND_RAM + 0x200), Some(0x55));
    assert_eq!(machine.peek(SOUND_RAM + 0x201), Some(0x66));
    assert_eq!(machine.sound_cpu().cycles(), 75);
}
