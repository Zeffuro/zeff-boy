use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadConfig, SoundFault};

fn step(machine: &mut Emulator, clocks: u32) {
    let before = machine.timing_snapshot().now().get();
    let cycle = machine.step_sound_instruction().unwrap();
    assert_eq!(cycle.cycles, clocks);
    assert_eq!(
        machine.timing_snapshot().now().get(),
        (before / 15 + u64::from(clocks)) * 15
    );
}

pub fn cb_register_and_memory_operations() {
    for opcode in 0..=255u8 {
        let mut machine = stopped(&[0xcb, opcode, 0x76], &[]);
        let clocks = if opcode & 7 == 6 {
            if opcode >> 6 == 1 { 12 } else { 15 }
        } else {
            8
        };
        step(&mut machine, clocks);
        assert_eq!(machine.sound_cpu().regs().pc, 2);
        assert_eq!(machine.sound_cpu().regs().r & 0x7f, 2);
    }
    let mut machine = stopped(
        &[
            0x06, 0x81, 0xcb, 0x00, 0x21, 0, 1, 0x36, 0x81, 0xcb, 0x06, 0xcb, 0x46, 0xcb, 0x86,
            0xcb, 0xfe, 0x76,
        ],
        &[],
    );
    step(&mut machine, 7);
    step(&mut machine, 8);
    assert_eq!(machine.sound_cpu().regs().b, 3);
    assert_eq!(machine.sound_cpu().regs().f, 5);
    step(&mut machine, 10);
    step(&mut machine, 10);
    step(&mut machine, 15);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(3));
    step(&mut machine, 12);
    assert_eq!(machine.sound_cpu().regs().f & 0x51, 0x11);
    step(&mut machine, 15);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(2));
    step(&mut machine, 15);
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x82));
}

pub fn ed_block_iterations_and_wraparound() {
    for (opcode, begin, copied) in [(0xb0, 0x100u16, [0x11, 0x22]), (0xb8, 0x101, [0x22, 0x11])] {
        let program = [
            0x21,
            begin as u8,
            (begin >> 8) as u8,
            0x11,
            0,
            2,
            0x01,
            2,
            0,
            0xed,
            opcode,
            0x76,
        ];
        let mut image_program = load_program(&program);
        store_byte(&mut image_program, SOUND_RAM + 0x100, 0x11);
        store_byte(&mut image_program, SOUND_RAM + 0x101, 0x22);
        store_word(&mut image_program, BUS_REQUEST, 0);
        image_program.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        let mut machine = new(&image_program, &[]);
        for _ in 0..program.len() + 6 {
            instruction(&mut machine);
        }
        machine.step_instruction().unwrap();
        for clocks in [10, 10, 10] {
            step(&mut machine, clocks);
        }
        let mut cloned = machine.clone();
        for device in [&mut machine, &mut cloned] {
            step(device, 21);
            assert_eq!(device.sound_cpu().regs().bc(), 1);
            assert_eq!(device.sound_cpu().regs().pc, 9);
            assert_eq!(device.peek(SOUND_RAM + 0x200), Some(copied[0]));
            step(device, 16);
            assert_eq!(device.sound_cpu().regs().bc(), 0);
            assert_eq!(device.sound_cpu().regs().pc, 11);
            let destination = if opcode == 0xb0 { 0x201 } else { 0x1ff };
            assert_eq!(device.peek(SOUND_RAM + destination), Some(copied[1]));
        }
        assert_eq!(machine, cloned);
    }
    let mut machine = stopped(&[0x21, 0, 1, 0x11, 0, 2, 0x01, 0, 0, 0xed, 0xb0], &[]);
    for clocks in [10, 10, 10] {
        step(&mut machine, clocks);
    }
    step(&mut machine, 21);
    assert_eq!(machine.sound_cpu().regs().bc(), 0xffff);
    assert_eq!(machine.sound_cpu().regs().pc, 9);
    for (opcode, begin, values) in [
        (0xb1, 0x100u16, [0x11, 0x22]),
        (0xb9, 0x101u16, [0x22, 0x11]),
    ] {
        for value in [values[0], values[1], 0x33] {
            let program = [
                0x21,
                begin as u8,
                (begin >> 8) as u8,
                0x01,
                2,
                0,
                0x3e,
                value,
                0xed,
                opcode,
                0x76,
            ];
            let mut machine = loaded(&program, &[(0x100, 0x11), (0x101, 0x22)]);
            for clocks in [10, 10, 7] {
                step(&mut machine, clocks);
            }
            if value == values[0] {
                step(&mut machine, 16);
                assert_eq!(machine.sound_cpu().regs().bc(), 1);
            } else {
                step(&mut machine, 21);
                assert_eq!(machine.sound_cpu().regs().pc, 8);
                step(&mut machine, 16);
                assert_eq!(machine.sound_cpu().regs().bc(), 0);
            }
            assert_eq!(machine.sound_cpu().regs().pc, 10);
            assert_eq!(
                machine.sound_cpu().regs().f & 0x40,
                if value == 0x33 { 0 } else { 0x40 }
            );
        }
    }
}

pub fn ed_faults_are_transactional() {
    for opcode in [
        0x40, 0x41, 0x70, 0x71, 0xa2, 0xa3, 0xaa, 0xab, 0xb2, 0xb3, 0xba, 0xbb,
    ] {
        let mut machine = stopped(&[0xed, opcode], &[]);
        let mut copied = machine.clone();
        let result = machine.step_sound_instruction();
        assert_eq!(result, copied.step_sound_instruction());
        assert_eq!(machine, copied);
        assert_eq!(
            result.unwrap().cycles,
            if opcode < 0xa0 {
                12
            } else if opcode < 0xb0 {
                16
            } else {
                21
            }
        );
        assert_eq!(machine.sound_bank(), 0);
        if opcode >= 0xa0 && opcode & 1 == 0 {
            assert_eq!(machine.peek(SOUND_RAM), Some(0xff));
        }
    }
    let mut machine = stopped(&[0x21, 0x34, 0x12, 0xed, 0x63, 3, 0x40], &[]);
    step(&mut machine, 10);
    let before = machine.clone();
    assert_eq!(
        machine.step_sound_instruction(),
        Err(SoundFault::UnsupportedAccess {
            address: 0x4004,
            direction: Direction::Write
        })
    );
    assert_eq!(machine.peek(SOUND_RAM + 0x1fff), Some(0));
    assert_eq!(machine, before);
    let mut machine = stopped(&[0x21, 1, 0, 0xed, 0x63, 0xff, 0x60], &[]);
    step(&mut machine, 10);
    step(&mut machine, 20);
    assert_eq!(machine.sound_cpu().regs().pc, 7);
    assert_eq!(machine.sound_bank(), 0x100);
    assert_eq!(machine.peek(SOUND_RAM + 0x6100), Some(0xff));
}

fn loaded(program: &[u8], source: &[(u32, u8)]) -> Emulator {
    let mut main = load_program(program);
    for &(address, byte) in source {
        store_byte(&mut main, SOUND_RAM + address, byte);
    }
    store_word(&mut main, BUS_REQUEST, 0);
    main.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    let mut machine = new(&main, &[]);
    for _ in 0..program.len() + source.len() + 4 {
        instruction(&mut machine);
    }
    machine.step_instruction().unwrap();
    machine
}

fn concurrent(program: &[u8], tail: &[u16], source: &[(u32, u8)]) -> Emulator {
    let mut main = load_program(program);
    for &(address, byte) in source {
        store_byte(&mut main, SOUND_RAM + address, byte);
    }
    store_word(&mut main, BUS_REQUEST, 0);
    main.extend_from_slice(tail);
    main.extend(std::iter::repeat_n(0x4e71, 96));
    let mut machine = Emulator::new(
        image(&main, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..program.len() + source.len() + 3 {
        instruction(&mut machine);
    }
    machine
}

pub fn concurrent_ed_handover_and_pending_clone() {
    let program = [0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xed, 0xb0, 0x76];
    for nops in [16, 20] {
        let mut tail = vec![0x4e71; nops];
        store_word(&mut tail, BUS_REQUEST, 0x100);
        tail.extend_from_slice(&[0x3039, 0x00a0, 0x0200]);
        let mut machine = concurrent(&program, &tail, &[(0x100, 0x5a), (0x101, 0x66)]);
        let clocks = if nops == 20 { 38 } else { 41 };
        for _ in 0..24 {
            if machine.sound_cpu().regs().pc == 11 && machine.sound_cpu().cycles() == clocks {
                break;
            }
            instruction(&mut machine);
        }
        assert_eq!(machine.sound_cpu().regs().pc, 11);
        assert_eq!(machine.sound_cpu().cycles(), clocks);
        assert_eq!(machine.sound_cpu().regs().bc(), 2);
        assert_eq!(machine.peek(SOUND_RAM + 0x200), Some(0));
        let write_deadline = machine.sound_deadline().map(|clock| clock.get() + 3 * 15);
        assert_eq!(write_deadline.is_some(), nops == 20);
        let mut copied = machine.clone();
        for _ in 0..48 {
            if machine.sound_bus_granted() {
                break;
            }
            assert_eq!(machine.step_instruction(), copied.step_instruction());
            assert_eq!(machine, copied);
            let now = machine.timing_snapshot().now().get();
            assert_eq!(
                machine.peek(SOUND_RAM + 0x200),
                Some(if write_deadline.is_some_and(|at| now >= at) {
                    0x5a
                } else {
                    0
                })
            );
        }
        assert!(machine.sound_bus_granted());
        assert_eq!(
            machine.sound_cpu().regs().bc(),
            if nops == 20 { 1 } else { 2 }
        );
        instruction(&mut machine);
        assert_eq!(
            machine.registers().d[0] & 0xffff,
            if nops == 20 { 0x5a5a } else { 0 }
        );
    }
}

pub fn concurrent_cb_memory_is_delayed() {
    let mut machine = concurrent(&[0x21, 0, 1, 0xcb, 0x06, 0x76], &[], &[(0x100, 0x81)]);
    for _ in 0..12 {
        if machine.sound_cpu().regs().pc == 5 && machine.sound_cpu().cycles() == 22 {
            break;
        }
        instruction(&mut machine);
    }
    let deadline = machine.sound_deadline().unwrap().get();
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(0x81));
    let mut copied = machine.clone();
    for _ in 0..16 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
        let now = machine.timing_snapshot().now().get();
        assert_eq!(
            machine.peek(SOUND_RAM + 0x100),
            Some(if now < deadline { 0x81 } else { 3 })
        );
        if now >= deadline {
            break;
        }
    }
    assert_eq!(machine.peek(SOUND_RAM + 0x100), Some(3));
}

pub fn extended_fetch_and_bank_guards() {
    for terminal in [0xcb, 0xdd, 0xed, 0xfd] {
        let mut machine = stopped(&[0xed, terminal, 0x76], &[]);
        step(&mut machine, 8);
        assert_eq!(machine.sound_cpu().regs().pc, 2);
    }
    let mut machine = loaded(&[0x21, 1, 0, 0xed, 0x63, 0, 0x60], &[]);
    step(&mut machine, 10);
    step(&mut machine, 20);
    assert_eq!(machine.sound_bank(), 0x80);
    for prefix in [0xcb, 0xed] {
        let mut main = load_program(&[0xc3, 0xff, 0x3f]);
        store_byte(&mut main, SOUND_RAM + 0x1fff, prefix);
        store_word(&mut main, BUS_REQUEST, 0);
        main.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        let mut machine = new(&main, &[]);
        for _ in 0..8 {
            instruction(&mut machine);
        }
        machine.step_instruction().unwrap();
        step(&mut machine, 10);
        step(&mut machine, 8);
        assert_eq!(machine.sound_cpu().regs().pc, 0x4001);
        for _ in 0..3 {
            step(&mut machine, 4);
        }
        let before = machine.clone();
        assert_eq!(
            machine.step_sound_instruction(),
            Err(SoundFault::UnsupportedAccess {
                address: 0x4004,
                direction: Direction::Read,
            })
        );
        assert_eq!(machine, before);
    }
}
