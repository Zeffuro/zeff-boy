pub(super) use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::{Emulator, LoadConfig, SoundFault};
use zeff_z80::CpuState;

pub fn shared_value_and_boundary_continuation() {
    let mut handler = Vec::new();
    store_word(&mut handler, BUS_REQUEST, 0x0100);
    handler.extend_from_slice(&[0x1039, 0x00a0, 0x0100]);
    store_word(&mut handler, BUS_REQUEST, 0);
    handler.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    let mut machine = stopped(&[0x3e, 0x5a, 0x32, 0, 1, 0x3c, 0x76, 0], &handler);
    let before = machine.timing_snapshot().now().get();
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 7);
    assert_eq!(
        machine.timing_snapshot().now().get(),
        (before / 15 + 7) * 15
    );
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0));
    let mut copied = machine.clone();
    assert_eq!(
        machine.step_sound_instruction(),
        copied.step_sound_instruction()
    );
    assert_eq!(machine, copied);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x5a));
    assert_eq!(machine.sound_cpu().cycles(), 20);
    wake(&mut machine);
    instruction(&mut machine);
    assert!(machine.sound_bus_requested() && machine.sound_bus_granted());
    let sound_before = machine.sound_cpu().clone();
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::MainCpuRunning)
    );
    assert_eq!(machine.sound_cpu(), &sound_before);
    instruction(&mut machine);
    assert_eq!(machine.registers().d[0] as u8, 0x5a);
    instruction(&mut machine);
    instruction(&mut machine);
    assert_eq!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Stopped
    );
    machine.step_sound_instruction().unwrap();
    assert_eq!(machine.sound_cpu().regs().a, 0x5b);
    machine.step_sound_instruction().unwrap();
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
}

pub fn main_sound_ram_lanes_and_mirrors() {
    let mut program = load_program(&[]);
    store_byte(&mut program, SOUND_RAM + 1, 0x33);
    store_word(&mut program, SOUND_RAM, 0x1234);
    program.extend_from_slice(&[0x3039, 0x00a0, 0x2000]);
    store_byte(&mut program, SOUND_RAM + 0x8001, 0x55);
    store_byte(&mut program, SOUND_RAM + 0x2000, 0x66);
    program.extend_from_slice(&[0x1239, 0x00a0, 1, 0x1439, 0x00a0, 0x8000, 0x4e71, 0x4e71]);
    let mut machine = new(&program, &[]);
    for _ in 0..4 {
        instruction(&mut machine);
    }
    assert_eq!(machine.peek(SOUND_RAM), Some(0x12));
    assert_eq!(machine.peek(SOUND_RAM + 1), Some(0x33));
    for _ in 0..5 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[0], 0x1212);
    assert_eq!(machine.registers().d[1], 0x55);
    assert_eq!(machine.registers().d[2], 0x66);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x66));
    assert_eq!(machine.peek(SOUND_RAM + 1), Some(0x55));
    assert_eq!(machine.peek(SOUND_RAM + 0xa000), Some(0x66));
}

pub fn serial_bank_and_cartridge_requester() {
    for bank in [0u16, 1, 2, 4, 8, 16, 32, 64, 0x55, 128, 0x100, 0x140, 0x1ff] {
        let mut program = load_program(&[0x3a, 0, 0x80, 0x76, 0]);
        for bit in 0..9 {
            store_byte(&mut program, SOUND_RAM + 0x6000, ((bank >> bit) & 1) as u8);
        }
        program.extend_from_slice(&[0x3039, 0x00a0, 0x6000]);
        store_word(&mut program, BUS_REQUEST, 0);
        program.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        let mut rom = image(&program, &[]);
        if bank == 1 {
            rom.resize(0x8001, 0);
            rom[0x8000] = 0x5a;
        }
        let mut machine = Emulator::new(rom, LoadConfig::default()).unwrap();
        for _ in 0..19 {
            instruction(&mut machine);
        }
        assert_eq!(machine.sound_bank(), bank);
        assert_eq!(machine.registers().d[0], 0xffff);
        assert_eq!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Stopped
        );
        if bank <= 0x7f || bank == 0x1ff {
            machine.step_sound_instruction().unwrap();
            assert_eq!(
                machine.sound_cpu().regs().a,
                if bank == 1 {
                    0x5a
                } else if bank == 0 {
                    0
                } else {
                    0xff
                }
            );
        } else {
            let before = machine.clone();
            assert!(matches!(
                machine.step_sound_instruction(),
                Err(SoundFault::UnsupportedAccess { .. })
            ));
            assert_eq!(machine, before);
        }
    }
}

pub fn sound_control_lanes_and_busy_bit() {
    let mut program = vec![0x1039, 0x00a1, 0x1100];
    store_byte(&mut program, BUS_REQUEST, 1);
    program.extend_from_slice(&[0x3239, 0x00a1, 0x1100]);
    store_byte(&mut program, RESET, 1);
    program.extend_from_slice(&[0x1439, 0x00a1, 0x1100]);
    store_byte(&mut program, SOUND_RAM, 0x5a);
    program.extend_from_slice(&[0x4e71, 0x4e71]);
    let mut machine = new(&program, &[]);
    for _ in 0..6 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[0] & 1, 1);
    assert_eq!(machine.registers().d[1] & 0x100, 0x100);
    assert_eq!(machine.registers().d[2] & 1, 0);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x5a));
    assert!(machine.sound_bus_granted());
}

pub fn reset_and_halt_ownership() {
    let mut handoff = Vec::new();
    store_word(&mut handoff, BUS_REQUEST, 0x0100);
    store_word(&mut handoff, BUS_REQUEST, 0);
    handoff.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    let mut halted = stopped(&[0x76, 0xdd], &handoff);
    halted.step_sound_instruction().unwrap();
    let cpu = halted.sound_cpu().clone();
    wake(&mut halted);
    for _ in 0..3 {
        instruction(&mut halted);
        assert_eq!(halted.sound_cpu(), &cpu);
    }
    halted.step_instruction().unwrap();
    assert_eq!(halted.step_sound_instruction().unwrap().cycles, 4);
    assert_eq!(halted.sound_cpu().state(), CpuState::Halted);

    let mut handler = Vec::new();
    store_word(&mut handler, BUS_REQUEST, 0x0100);
    store_word(&mut handler, RESET, 0);
    store_word(&mut handler, RESET, 0x0100);
    store_word(&mut handler, BUS_REQUEST, 0);
    handler.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    let mut machine = stopped_with_bank(&[0x76, 0xdd], &handler, 0x155);
    machine.step_sound_instruction().unwrap();
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    assert_eq!(machine.step_sound_instruction().unwrap().cycles, 4);
    let old = machine.timing_snapshot();
    wake(&mut machine);
    instruction(&mut machine);
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    instruction(&mut machine);
    assert!(machine.sound_reset_asserted());
    assert!(machine.sound_bus_requested() && !machine.sound_bus_granted());
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.sound_cpu().state(), CpuState::Running);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x76));
    assert_eq!(machine.sound_bank(), 0x155);
    instruction(&mut machine);
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.sound_bank(), 0x155);
    instruction(&mut machine);
    instruction(&mut machine);
    assert_eq!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Stopped
    );
    assert!(machine.timing_snapshot().elapsed_since(old).unwrap().get() > 0);
    machine.step_sound_instruction().unwrap();
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    Reset::reset(&mut machine);
    assert!(machine.sound_reset_asserted());
    assert_eq!(machine.peek(SOUND_RAM), Some(0));
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.sound_bank(), 0);
}
