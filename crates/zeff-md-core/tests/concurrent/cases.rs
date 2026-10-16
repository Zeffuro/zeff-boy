use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

fn start(sound: &[u8], tail: &[u16]) -> Emulator {
    let mut program = load_program(sound);
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(tail);
    program.extend(std::iter::repeat_n(0x4e71, 96));
    let mut machine = Emulator::new(
        image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + 3 {
        instruction(&mut machine);
    }
    machine
}

fn pending_store() -> Emulator {
    let mut machine = start(&[0x3e, 0x5a, 0x32, 0, 1, 0x76], &[]);
    for _ in 0..8 {
        if machine.sound_cpu().regs().pc == 5 {
            break;
        }
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().pc, 5);
    assert_eq!(machine.sound_cpu().cycles(), 17);
    assert!(machine.sound_deadline().unwrap().get() > machine.timing_snapshot().now().get());
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0));
    machine
}

pub fn delayed_write_and_pending_clone() {
    let mut machine = pending_store();
    let deadline = machine.sound_deadline().unwrap().get();
    let mut copied = machine.clone();
    for _ in 0..16 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        let now = machine.timing_snapshot().now().get();
        assert_eq!(
            machine.peek(SOUND_RAM + 0x100),
            Some(if now < deadline { 0 } else { 0x5a })
        );
        if now >= deadline {
            break;
        }
    }
    assert!(machine.timing_snapshot().now().get() >= deadline);
    assert_eq!(machine.sound_cpu().cycles(), 20);
}

pub fn requested_read_waits_once() {
    let mut tail = Vec::new();
    store_word(&mut tail, BUS_REQUEST, 0x0100);
    tail.extend_from_slice(&[0x1039, 0x00a0, 0x0100]);
    let mut machine = start(&[0x3e, 0x5a, 0x32, 0, 1, 0x76], &tail);
    instruction(&mut machine);
    assert!(machine.sound_bus_requested());
    let deadline = machine.sound_deadline().map(|time| time.get());
    let before = machine.timing_snapshot().now().get();
    let step = machine.step_instruction().unwrap();
    let expected_wait = deadline
        .map(|end| end.saturating_sub(before + 84).div_ceil(7))
        .unwrap_or(0);
    let zeff_m68k::timing::TimedOutcome::Instruction(cpu) = step.cpu else {
        panic!("instruction")
    };
    assert_eq!(cpu.clocks, 16 + expected_wait);
    assert_eq!(step.elapsed.get(), cpu.clocks * 7);
    assert!(machine.sound_bus_granted());
    assert_eq!(expected_wait, 0);
    assert_eq!(machine.registers().d[0] as u8, 0);
    let sound = machine.sound_cpu().clone();
    instruction(&mut machine);
    assert_eq!(machine.sound_cpu(), &sound);
}

pub fn reset_discards_unpublished_store() {
    let mut tail = vec![0x4e71; 3];
    store_word(&mut tail, RESET, 0);
    let mut machine = start(&[0x3e, 0x5a, 0x32, 0, 1, 0x76], &tail);
    for _ in 0..3 {
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().a, 0x5a);
    assert_eq!(machine.sound_cpu().regs().pc, 2);
    let before = machine.timing_snapshot().now().get();
    assert!(machine.sound_deadline().is_some());
    instruction(&mut machine);
    assert!(machine.sound_reset_asserted());
    assert!(machine.sound_deadline().is_none());
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0));
    assert!(machine.timing_snapshot().now().get() > before);
}

pub fn main_reset_preserves_pending_time() {
    let mut machine = pending_store();
    let before = machine.timing_snapshot();
    machine.set_interrupt_level(4);
    let mut advanced = machine.clone();
    instruction(&mut advanced);
    instruction(&mut advanced);
    machine.reset_cpu().unwrap();
    assert_eq!(
        machine
            .timing_snapshot()
            .elapsed_since(before)
            .unwrap()
            .get(),
        56
    );
    assert_eq!(machine.sound_cpu(), advanced.sound_cpu());
    assert_eq!(machine.sound_deadline(), advanced.sound_deadline());
    assert_eq!(machine.interrupt_level(), 4);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x5a));
    machine.reset_sound_cpu();
    assert!(machine.sound_deadline().is_none());
    assert_eq!(machine.interrupt_level(), 4);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x3e));
}

pub fn unsupported_sound_latches_after_prior_progress() {
    let mut machine = start(&[0x3e, 0x55, 0xdb, 0x7f, 0x76], &[]);
    for _ in 0..16 {
        if machine.sound_cpu().state() == CpuState::Halted {
            break;
        }
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().regs().a, 0xff);
    assert!(machine.sound_fault().is_none());
    let mut machine = start(&[0x3e, 0x55, 0x3a, 4, 0x40], &[]);
    let mut fault = None;
    for _ in 0..8 {
        if let Err(error) = machine.step_instruction() {
            fault = Some(error);
            break;
        }
    }
    assert_eq!(
        fault,
        Some(MachineFault::Sound(SoundFault::UnsupportedAccess {
            address: 0x4004,
            direction: zeff_m68k::bus::Direction::Read
        }))
    );
    assert_eq!(machine.sound_cpu().regs().a, 0x55);
    let saved = machine.clone();
    assert_eq!(
        machine.step_instruction(),
        Err(MachineFault::Sound(machine.sound_fault().unwrap()))
    );
    assert_eq!(machine, saved);
    machine.reset_cpu().unwrap();
    assert!(machine.sound_fault().is_some());
    machine.reset_sound_cpu();
    assert!(machine.sound_fault().is_none());
    Reset::reset(&mut machine);
    assert_eq!(machine.sound_cpu().cycles(), 0);
}

pub fn halted_sound_ownership() {
    let mut tail = vec![0x4e71; 4];
    store_word(&mut tail, BUS_REQUEST, 0x0100);
    store_word(&mut tail, BUS_REQUEST, 0);
    let mut machine = start(&[0x76, 0xdd], &tail);
    for _ in 0..4 {
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    instruction(&mut machine);
    instruction(&mut machine);
    for _ in 0..4 {
        instruction(&mut machine);
    }
    assert_eq!(machine.sound_cpu().state(), CpuState::Halted);
    assert!(machine.sound_fault().is_none());
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::ConcurrentSchedulingEnabled)
    );
}
