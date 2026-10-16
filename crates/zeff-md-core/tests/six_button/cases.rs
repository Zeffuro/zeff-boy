use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator};

fn read(program: &mut Vec<u16>, address: u32, register: u16) {
    program.extend_from_slice(&[
        0x1039 | register << 9,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn poll(program: &mut Vec<u16>, address: u32) {
    store_byte(program, address + 6, 0x40);
    for phase in 0..9 {
        store_byte(program, address, if phase % 2 == 0 { 0x40 } else { 0 });
        if phase < 8 {
            read(program, address, phase);
        }
    }
}

fn six(machine: &mut Emulator, port: ControllerPort, buttons: u8, extra: u8) {
    let input = machine.input_mut();
    input.set_controller_kind(port, ControllerKind::SixButton);
    input.set_controller_raw(port, buttons);
    input.set_controller_extra_raw(port, extra);
}

pub fn six_button_sequence() {
    for (port, address) in [
        (ControllerPort::One, 0xa1_0003),
        (ControllerPort::Two, 0xa1_0005),
    ] {
        let mut program = Vec::new();
        poll(&mut program, address);
        program.extend_from_slice(&[0x4e71; 2]);
        for raw in 0..4096u16 {
            let buttons = raw as u8;
            let extra = (raw >> 8) as u8;
            let mut machine = new(&program, &[]);
            six(&mut machine, port, buttons, extra);
            for _ in 0..18 {
                instruction(&mut machine);
            }
            let high = u32::from(0x40 | (buttons & 0x3f));
            let low_buttons = u32::from((buttons >> 2) & 0x30);
            let low = low_buttons | u32::from(buttons & 3);
            assert_eq!(
                machine.registers().d,
                [
                    high,
                    low,
                    high,
                    low,
                    high,
                    low_buttons,
                    u32::from(0x40 | (buttons & 0x30) | extra),
                    low_buttons | 0x0f,
                ]
            );
            assert_eq!(machine.peek(address), Some(high as u8));
            assert_eq!(machine.input().controller_raw(port), buttons);
            assert_eq!(machine.input().controller_extra_raw(port), extra);
        }
    }
}

pub fn six_button_effective_th() {
    let mut program = Vec::new();
    for level in [0, 0x40, 0, 0x40, 0] {
        store_byte(&mut program, 0xa1_0003, level);
    }
    for phase in 0..6 {
        store_byte(
            &mut program,
            0xa1_0009,
            if phase % 2 == 0 { 0x40 } else { 0 },
        );
        store_byte(
            &mut program,
            0xa1_0009,
            if phase % 2 == 0 { 0x40 } else { 0 },
        );
        read(&mut program, 0xa1_0003, phase);
        read(&mut program, 0xa1_0003, phase);
    }
    program.extend_from_slice(&[0x4e71; 2]);
    let mut machine = new(&program, &[]);
    six(&mut machine, ControllerPort::One, 0xff, 5);
    for _ in 0..29 {
        instruction(&mut machine);
    }
    assert_eq!(
        &machine.registers().d[..6],
        &[0x33, 0x7f, 0x33, 0x7f, 0x30, 0x75]
    );
    let saved = machine.clone();
    for _ in 0..8 {
        assert_eq!(machine.peek(0xa1_0003), Some(0x75));
    }
    assert_eq!(machine, saved);
}

pub fn six_button_timeout() {
    let mut program = Vec::new();
    store_byte(&mut program, 0xa1_0009, 0x40);
    for level in [0, 0x40, 0, 0x40, 0, 0x40] {
        store_byte(&mut program, 0xa1_0003, level);
    }
    program.extend_from_slice(&[0x4e71; 4098]);
    let mut machine = new(&program, &[]);
    six(&mut machine, ControllerPort::One, 0xff, 0);
    for _ in 0..7 {
        instruction(&mut machine);
    }
    assert_eq!(machine.peek(0xa1_0003), Some(0x70));
    let before = machine.timing_snapshot().now().get();
    for _ in 0..4096 {
        instruction(&mut machine);
    }
    assert!(machine.timing_snapshot().now().get() - before > 80_540);
    assert_eq!(machine.peek(0xa1_0003), Some(0x7f));
}

pub fn six_button_identity_and_mode() {
    let mut program = Vec::new();
    poll(&mut program, 0xa1_0003);
    program.extend_from_slice(&[0x4e71; 2]);
    let mut machine = new(&program, &[]);
    six(&mut machine, ControllerPort::One, 0xff, 7);
    machine.reset_cpu().unwrap();
    machine.reset_sound_cpu();
    for _ in 0..18 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[6], 0x77);
    Reset::reset(&mut machine);
    assert_eq!(
        machine.input().controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
    assert_eq!(
        machine.input().controller_kind(ControllerPort::Two),
        ControllerKind::ThreeButton
    );
    assert_eq!(machine.input().controller_raw(ControllerPort::One), 0xff);
    machine
        .input_mut()
        .set_controller_extra_raw(ControllerPort::One, 0);
    for _ in 0..18 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[5], 0x33);
    assert_eq!(machine.registers().d[6], 0x7f);
    machine
        .input_mut()
        .set_controller_extra_raw(ControllerPort::One, 0xff);
    machine.reset_cpu().unwrap();
    for _ in 0..18 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[6], 0x7f);
    Reset::reset(&mut machine);
    machine
        .input_mut()
        .set_controller_extra_raw(ControllerPort::One, 0);
    for _ in 0..18 {
        instruction(&mut machine);
    }
    assert_eq!(machine.registers().d[5], 0x30);
    assert_eq!(machine.registers().d[6], 0x70);
}

pub fn six_button_clone_and_ports() {
    let mut program = Vec::new();
    store_byte(&mut program, 0xa1_0009, 0x40);
    store_byte(&mut program, 0xa1_000b, 0x40);
    for level in [0, 0x40, 0, 0x40, 0, 0x40] {
        store_byte(&mut program, 0xa1_0003, level);
        store_byte(&mut program, 0xa1_0005, level);
    }
    program.extend_from_slice(&[0x4e71; 2]);
    let mut machine = new(&program, &[]);
    six(&mut machine, ControllerPort::One, 0xff, 1);
    six(&mut machine, ControllerPort::Two, 0xff, 2);
    for _ in 0..11 {
        instruction(&mut machine);
    }
    assert_eq!(machine.peek(0xa1_0003), Some(0x30));
    assert_eq!(machine.peek(0xa1_0005), Some(0x7f));
    let mut copied = machine.clone();
    for _ in 0..3 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    assert_eq!(machine.peek(0xa1_0003), Some(0x71));
    assert_eq!(machine.peek(0xa1_0005), Some(0x72));
    machine
        .input_mut()
        .set_controller_kind(ControllerPort::One, ControllerKind::ThreeButton);
    assert_eq!(machine.peek(0xa1_0003), Some(0x7f));
    assert_eq!(machine.peek(0xa1_0005), Some(0x72));
}

pub fn six_button_lanes_and_default() {
    let mut program = Vec::new();
    store_word(&mut program, 0xa1_0008, 0xaa40);
    for level in [0, 0x40, 0, 0x40, 0, 0x40] {
        store_word(&mut program, 0xa1_0002, 0xaa00 | level);
    }
    program.extend_from_slice(&[0x3039, 0x00a1, 0x0002, 0x4e71, 0x4e71]);
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        let mut machine = new(&program, &[]);
        assert_eq!(
            machine.input().controller_kind(ControllerPort::One),
            ControllerKind::ThreeButton
        );
        machine
            .input_mut()
            .set_controller_kind(ControllerPort::One, kind);
        machine
            .input_mut()
            .set_controller_extra_raw(ControllerPort::One, 0xfa);
        for _ in 0..8 {
            instruction(&mut machine);
        }
        let expected = if kind == ControllerKind::SixButton {
            0x7a
        } else {
            0x7f
        };
        assert_eq!(machine.peek(0xa1_0002), Some(expected));
        assert_eq!(machine.peek(0xa1_0003), Some(expected));
        assert_eq!(machine.registers().d[0], u32::from(expected) * 0x101);
    }
}
