use super::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_md_core::{LoadConfig, MachineFault, SoundFault};
use zeff_z80::CpuState;

fn bank_sources(bank: u16) -> Vec<(u32, u8)> {
    (0..9)
        .map(|bit| (0x6000, ((bank >> bit) & 1) as u8))
        .collect()
}

fn interleaved(nops: usize) -> Emulator {
    let sound = [0x21, 0x04, 0x80, 0x3e, 0x55, 0x77, 0x76];
    let mut program = fixture::load_program(&sound);
    for (address, value) in bank_sources(0x1ff) {
        fixture::store_byte(&mut program, fixture::SOUND_RAM + address, value);
    }
    fixture::store_byte(&mut program, 0xff8004, 0xde);
    fixture::store_byte(&mut program, 0xff8005, 0x12);
    fixture::store_word(&mut program, fixture::BUS_REQUEST, 0);
    program.extend(std::iter::repeat_n(0x4e71, nops));
    fixture::store_byte(&mut program, 0xff8005, 0xa6);
    program.extend_from_slice(&[0x4e71; 128]);
    let mut machine = Emulator::new(
        fixture::image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..sound.len() + 14 + nops {
        fixture::instruction(&mut machine);
    }
    machine
}

pub fn banked_ram_main_interleave_preserves_disjoint_writes() {
    for nops in 0..=16 {
        let mut machine = interleaved(nops);
        assert_eq!(machine.peek(0xff8005), Some(0x12));
        fixture::instruction(&mut machine);
        assert_eq!(machine.peek(0xff8005), Some(0xa6));
        transfer::finish(&mut machine);
        assert_eq!(machine.peek(0xff8004), Some(0x55));
        assert_eq!(machine.peek(0xff8005), Some(0xa6));
        assert_eq!(machine.sound_cpu().regs().a, 0x55);
        assert_eq!(machine.sound_cpu().regs().hl(), 0x8004);
        assert!(machine.sound_fault().is_none());
    }
    let mut pending = (0..=16)
        .map(interleaved)
        .find(|machine| machine.sound_cpu().cycles() == 21 && machine.peek(0xff8004) == Some(0xde))
        .expect("queued banked write boundary");
    assert_eq!(pending.sound_cpu().regs().pc, 6);
    assert!(pending.sound_instruction_pending());
    assert!(pending.sound_deadline().is_some());
    let mut copied = pending.clone();
    let mut warm = pending.clone();
    let now = warm.timing_snapshot().now();
    warm.reset_sound_cpu();
    assert_eq!(warm.timing_snapshot().now(), now);
    assert_eq!(warm.peek(0xff8004), Some(0xde));
    assert!(!warm.sound_instruction_pending());
    assert!(warm.sound_deadline().is_none());
    assert_eq!(pending.step_instruction(), copied.step_instruction());
    assert_eq!(pending, copied);
    assert_eq!(pending.peek(0xff8004), Some(0x55));
    fixture::instruction(&mut warm);
    assert_eq!(warm.peek(0xff8004), Some(0xde));
    assert_eq!(warm.peek(0xff8005), Some(0xa6));
    let mut sources = bank_sources(0x1ff);
    sources.extend_from_slice(&[(0x5f8004, 0xde), (0x5f8005, 0x12)]);
    let mut machine = transfer::start(
        &[0x21, 0x04, 0x80, 0x3e, 0x55, 0x77, 0x76],
        0,
        &sources,
        &[(0x5f8004, 0x99), (0x5f8005, 0xa6)],
    );
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.peek(0xff8004), Some(0xde));
    transfer::finish(&mut machine);
    assert_eq!(machine.peek(0xff8004), Some(0x55));
    assert_eq!(machine.peek(0xff8005), Some(0xa6));
}

fn assert_sticky(machine: &mut Emulator, expected: MachineFault) {
    let cpu = machine.sound_cpu().clone();
    let mut copied = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected.clone()));
    assert_eq!(copied.step_instruction(), Err(expected.clone()));
    assert_eq!(*machine, copied);
    assert_eq!(machine.sound_cpu(), &cpu);
    let saved = machine.clone();
    assert_eq!(machine.step_instruction(), Err(expected));
    assert_eq!(*machine, saved);
}

pub fn banked_ram_grant_fault_clone_reset_and_guards() {
    let sound = [0x31, 0x01, 0x80, 0x21, 0x34, 0x12, 0xe5, 0x76];
    let fault = SoundFault::UnsupportedAccess {
        address: 0x7fff,
        direction: Direction::Write,
    };
    let mut manual = stopped_ram(&sound, 0x1c0, &[(0xe00000, 0xde)]);
    for _ in 0..2 {
        assert_eq!(manual.step_sound_instruction().unwrap().cycles, 10);
    }
    let saved = manual.clone();
    assert_eq!(manual.step_sound_instruction(), Err(fault));
    assert_eq!(manual, saved);
    assert_eq!(manual.step_sound_instruction(), Err(fault));
    assert_eq!(manual, saved);
    assert_eq!(manual.peek(0xe00000), Some(0xde));

    let mut sources = bank_sources(0x1c0);
    sources.push((0x400000, 0xde));
    let mut machine = transfer::start(&sound, 8, &sources, &[]);
    assert_eq!(machine.sound_cpu().cycles(), 29);
    assert_eq!(machine.sound_cpu().regs().sp, 0x7fff);
    assert_eq!(machine.sound_cpu().regs().pc, 7);
    assert!(machine.sound_instruction_pending());
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.peek(0xe00000), Some(0x12));
    let mut warm = machine.clone();
    let mut cold = machine.clone();
    assert_sticky(&mut machine, MachineFault::Sound(fault));
    assert_eq!(machine.peek(0xe00000), Some(0x12));
    let now = machine.timing_snapshot().now();
    machine.set_sound_interrupt(true);
    machine.reset_sound_cpu();
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xe00000), Some(0x12));
    assert!(!machine.sound_instruction_pending());
    assert!(machine.sound_fault().is_none());
    warm.reset_sound_cpu();
    assert_eq!(warm.peek(0xe00000), Some(0x12));
    assert_eq!(warm.sound_cpu().regs().sp, 0);
    Reset::reset(&mut cold);
    assert_eq!(cold.peek(0xe00000), Some(0));
    assert!(!cold.sound_instruction_pending());
    assert!(cold.sound_fault().is_none());

    for bank in [0x080, 0x100, 0x140, 0x141] {
        for (opcode, direction) in [(0x7e, Direction::Read), (0x77, Direction::Write)] {
            let mut machine = stopped_ram(&[0x21, 0x04, 0x80, opcode], bank, &[(0xe00004, 0xde)]);
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, 10);
            let saved = machine.clone();
            assert_eq!(
                machine.step_sound_instruction(),
                Err(SoundFault::UnsupportedAccess {
                    address: 0x8004,
                    direction
                })
            );
            assert_eq!(machine, saved);
            assert_eq!(machine.peek(0xe00004), Some(0xde));
        }
    }
    let mut rom_write = stopped_ram(&[0x21, 0x04, 0x80, 0x77], 0, &[(0xe00004, 0xde)]);
    assert_eq!(rom_write.step_sound_instruction().unwrap().cycles, 10);
    let saved = rom_write.clone();
    assert_eq!(
        rom_write.step_sound_instruction(),
        Err(SoundFault::UnsupportedAccess {
            address: 0x8004,
            direction: Direction::Write
        })
    );
    assert_eq!(rom_write, saved);
    let mut absent_rom = stopped_ram(&[0x21, 0x04, 0x80, 0x7e], 1, &[]);
    assert_eq!(absent_rom.step_sound_instruction().unwrap().cycles, 10);
    assert_eq!(absent_rom.step_sound_instruction().unwrap().cycles, 7);
    assert_eq!(absent_rom.sound_cpu().regs().a, 0xff);
    assert_eq!(absent_rom.sound_cpu().state(), CpuState::Running);
}
