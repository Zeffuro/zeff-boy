pub(crate) use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

pub(crate) fn start(
    sound: &[u8],
    nops: usize,
    source: &[(u32, u8)],
    writes: &[(u32, u8)],
) -> Emulator {
    let mut program = load_program(sound);
    for &(address, value) in source {
        store_byte(&mut program, SOUND_RAM + address, value);
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend(std::iter::repeat_n(0x4e71, nops));
    store_word(&mut program, BUS_REQUEST, 0x100);
    program.extend_from_slice(&[0x1e39, 0x00a0, 0x0100]);
    for &(address, value) in writes {
        store_byte(&mut program, SOUND_RAM + address, value);
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(&[0x4e71; 120]);
    let mut machine = Emulator::new(
        image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + source.len() + nops + 5 {
        instruction(&mut machine);
    }
    assert!(machine.sound_bus_granted());
    machine
}

pub(crate) fn finish(machine: &mut Emulator) {
    let mut copied = machine.clone();
    for _ in 0..128 {
        if machine.sound_cpu().state() == CpuState::Halted {
            return;
        }
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(*machine, copied);
    }
    panic!("sound did not halt");
}

pub fn immediate_low_latched_high_live() {
    let mut machine = start(
        &[0, 0x01, 0x34, 0x12, 0x76],
        0,
        &[],
        &[(2, 0x99), (3, 0x56)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 11);
    assert_eq!(machine.sound_cpu().regs().bc(), 0);
    assert!(machine.sound_instruction_pending());
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().bc(), 0x5634);
}

pub fn address_and_partial_write_survive_grant() {
    let mut machine = start(
        &[0x21, 0x34, 0x12, 0x22, 0, 1, 0x76],
        7,
        &[],
        &[(4, 0), (5, 2), (0x100, 0x99)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 23);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x34));
    assert_eq!(machine.peek(SOUND_RAM + 0x101), Some(0));
    finish(&mut machine);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x99));
    assert_eq!(machine.peek(SOUND_RAM + 0x101), Some(0x12));
    assert_eq!(machine.peek(SOUND_RAM + 0x201), Some(0));
}

pub fn partial_write_fault_and_reset() {
    let sound = [0x21, 0x34, 0x12, 0x22, 3, 0x40, 0x76];
    let mut machine = start(&sound, 7, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 23);
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
    let cpu = machine.sound_cpu().clone();
    let mut copied = machine.clone();
    let expected = MachineFault::Sound(SoundFault::UnsupportedAccess {
        address: 0x4004,
        direction: Direction::Write,
    });
    let result = machine.step_instruction();
    assert_eq!(result, copied.step_instruction());
    assert_eq!(result, Err(expected.clone()));
    assert_eq!(machine, copied);
    assert_eq!(machine.sound_cpu(), &cpu);
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected));
    assert_eq!(machine, saved);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.opn2_snapshot().status & 0x80, 0x80);
    assert_eq!(machine.timing_snapshot().now(), now);
    assert!(!machine.sound_instruction_pending());
    Reset::reset(&mut machine);
    assert_eq!(machine.peek(SOUND_RAM + 0x1fff), Some(0));
}

pub fn partial_read_latched_low_live_high() {
    let mut machine = start(
        &[0x2a, 0, 1, 0x76],
        0,
        &[(0x100, 0x34), (0x101, 0x12)],
        &[(0x100, 0x99), (0x101, 0x56)],
    );
    assert_eq!(machine.sound_cpu().cycles(), 13);
    assert_eq!(machine.sound_cpu().regs().hl(), 0);
    finish(&mut machine);
    assert_eq!(machine.sound_cpu().regs().hl(), 0x5634);
}
