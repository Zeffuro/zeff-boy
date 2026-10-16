use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, ControllerPort, Emulator, LoadConfig, MachineFault};

fn read_byte(program: &mut Vec<u16>, address: u32, register: u16) {
    program.extend_from_slice(&[
        0x1039 | register << 9,
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub fn controller_multiplexing() {
    for (port, address) in [
        (ControllerPort::One, 0xa1_0003),
        (ControllerPort::Two, 0xa1_0005),
    ] {
        for buttons in 0..=255u8 {
            let mut program = Vec::new();
            store_byte(&mut program, address + 6, 0x40);
            store_byte(&mut program, address, 0x40);
            read_byte(&mut program, address, 0);
            store_byte(&mut program, address, 0);
            read_byte(&mut program, address, 1);
            program.extend_from_slice(&[0x4e71; 2]);
            let mut machine = new(&program, &[]);
            machine.input_mut().set_controller_raw(port, buttons);
            for _ in 0..5 {
                instruction(&mut machine);
            }
            let high = 0x40 | (buttons & 0x3f);
            let mut low = buttons & 3;
            if buttons & 0x40 != 0 {
                low |= 0x10;
            }
            if buttons & 0x80 != 0 {
                low |= 0x20;
            }
            assert_eq!(machine.registers().d[0], u32::from(high));
            assert_eq!(machine.registers().d[1], u32::from(low));
            assert_eq!(machine.peek(address ^ 1), Some(low));
            assert_eq!(machine.input().controller_raw(port), buttons);
        }
    }
}

pub fn pin_direction_and_latch() {
    let mut program = Vec::new();
    store_byte(&mut program, 0xa1_0003, 0);
    read_byte(&mut program, 0xa1_0003, 0);
    store_byte(&mut program, 0xa1_0009, 0x40);
    read_byte(&mut program, 0xa1_0003, 1);
    store_byte(&mut program, 0xa1_0009, 0);
    read_byte(&mut program, 0xa1_0003, 2);
    store_byte(&mut program, 0xa1_0003, 0x80);
    read_byte(&mut program, 0xa1_0003, 3);
    program.extend_from_slice(&[0x4e71; 2]);
    let mut machine = new(&program, &[]);
    machine
        .input_mut()
        .set_controller_raw(ControllerPort::One, 0x9f);
    for _ in 0..8 {
        instruction(&mut machine);
    }
    assert_eq!(&machine.registers().d[..4], &[0x5f, 0x23, 0x5f, 0xdf]);
    for direction in [0, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0x7f, 0xff] {
        for data in [0, 0x40, 0x80, 0xff] {
            let mut program = Vec::new();
            store_byte(&mut program, 0xa1_000d, direction);
            store_byte(&mut program, 0xa1_0007, data);
            read_byte(&mut program, 0xa1_0007, 0);
            read_byte(&mut program, 0xa1_000d, 1);
            program.extend_from_slice(&[0x4e71; 2]);
            let mut machine = new(&program, &[]);
            for _ in 0..4 {
                instruction(&mut machine);
            }
            let expected = (0x7f & !direction) | (data & (direction | 0x80));
            assert_eq!(machine.registers().d[0], u32::from(expected));
            assert_eq!(machine.registers().d[1], u32::from(direction));
            assert_eq!(machine.interrupt_level(), 0);
        }
    }
}

pub fn register_lanes() {
    let mut program = Vec::new();
    store_word(&mut program, 0xa1_0008, 0x127f);
    store_word(&mut program, 0xa1_0002, 0x1285);
    read_byte(&mut program, 0xa1_0002, 0);
    read_byte(&mut program, 0xa1_0003, 1);
    program.extend_from_slice(&[0x3439, 0x00a1, 0x0002]);
    store_byte(&mut program, 0xa1_0003, 0x42);
    read_byte(&mut program, 0xa1_0003, 3);
    read_byte(&mut program, 0xa1_0001, 4);
    store_word(&mut program, 0xa1_0000, 0);
    program.extend_from_slice(&[0x4e71; 2]);
    let mut machine = new(&program, &[]);
    for _ in 0..9 {
        instruction(&mut machine);
    }
    assert_eq!(
        &machine.registers().d[..5],
        &[0x85, 0x85, 0x8585, 0x42, 0xa0]
    );
    assert_eq!(machine.peek(0xa1_0000), Some(0xa0));
}

pub fn input_reset_and_copy() {
    let mut program = Vec::new();
    store_byte(&mut program, 0xa1_0009, 0x40);
    store_byte(&mut program, 0xa1_0003, 0x80);
    read_byte(&mut program, 0xa1_0003, 0);
    program.extend_from_slice(&[0x4e71; 3]);
    let mut machine = new(&program, &[]);
    machine
        .input_mut()
        .set_controller_raw(ControllerPort::One, 0);
    instruction(&mut machine);
    instruction(&mut machine);
    let mut copied = machine.clone();
    assert_eq!(machine.step_instruction(), copied.step_instruction());
    assert_eq!(machine, copied);
    let input = machine.input().clone();
    let data = machine.peek(0xa1_0003);
    let now = machine.timing_snapshot().now();
    machine.reset_sound_cpu();
    assert_eq!(machine.input(), &input);
    assert_eq!(machine.timing_snapshot().now(), now);
    machine.reset_cpu().unwrap();
    assert_eq!(machine.input(), &input);
    assert_eq!(machine.peek(0xa1_0003), data);
    Reset::reset(&mut machine);
    assert_eq!(machine, new(&program, &[]));
}

pub fn absent_devices_fail_once() {
    for address in [0xa1_0020u32, 0xa1_2000, 0xa1_5100, 0x80_0000] {
        for direction in [Direction::Read, Direction::Write] {
            let mut program = vec![0x7055];
            store_word(&mut program, 0xff_0100, 0x1234);
            if direction == Direction::Read {
                read_byte(&mut program, address, 1);
            } else {
                store_byte(&mut program, address, 0xff);
            }
            program.extend_from_slice(&[0x4e71; 2]);
            let mut machine = new(&program, &[]);
            instruction(&mut machine);
            instruction(&mut machine);
            assert_eq!(machine.peek(address), None);
            assert!(matches!(machine.step_instruction(), Err(MachineFault::Main(
                TimingFault::Bus { source: BusError::UnsupportedAccess { address: actual, direction: actual_direction }, .. }
            )) if actual == address && actual_direction == direction));
            assert_eq!(machine.registers().d[0], 0x55);
            assert_eq!(machine.peek(0xff_0101), Some(0x34));
            let saved = machine.clone();
            assert_eq!(
                machine.step_instruction(),
                Err(MachineFault::Main(TimingFault::Frozen))
            );
            assert_eq!(machine, saved);
        }
    }
    for address in [0xa1_0002, 0xa1_0008] {
        let mut program = Vec::new();
        store_byte(&mut program, address, 0xff);
        program.extend_from_slice(&[0x4e71; 2]);
        let mut machine = new(&program, &[]);
        let input = machine.input().clone();
        assert!(matches!(machine.step_instruction(), Err(MachineFault::Main(
            TimingFault::Bus { source: BusError::UnsupportedAccess { address: actual, direction: Direction::Write }, .. }
        )) if actual == address));
        assert_eq!(machine.input(), &input);
    }
}

pub fn serial_register_lanes_and_state() {
    for region in [zeff_md_core::Region::NtscUs, zeff_md_core::Region::Pal] {
        for transmit in [0xa1_000eu32, 0xa1_0014, 0xa1_001a] {
            let mut program = Vec::new();
            read_byte(&mut program, transmit + 1, 0);
            store_word(&mut program, transmit, 0x125a);
            read_byte(&mut program, transmit, 1);
            program.extend_from_slice(&[0x3439, (transmit >> 16) as u16, transmit as u16]);
            store_byte(&mut program, transmit + 3, 0xff);
            read_byte(&mut program, transmit + 2, 3);
            store_byte(&mut program, transmit + 5, 0xcf);
            read_byte(&mut program, transmit + 4, 4);
            program.extend_from_slice(&[0x4e71; 2]);
            let mut machine = Emulator::new(
                image(&program, &[]),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            for _ in 0..8 {
                instruction(&mut machine);
            }
            assert_eq!(&machine.registers().d[..5], &[0xff, 0x5a, 0x5a5a, 0, 0xc8]);
            assert_eq!(machine.peek(transmit + 1), Some(0x5a));
            let saved = machine.encode_state().unwrap();
            let mut restored = Emulator::new(
                image(&program, &[]),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            restored.decode_state(&saved).unwrap();
            assert_eq!(restored.encode_state().unwrap(), saved);
            assert_eq!(restored.step_instruction(), machine.step_instruction());
            assert_eq!(
                restored.encode_state().unwrap(),
                machine.encode_state().unwrap()
            );
            Reset::reset(&mut restored);
            assert_eq!(restored.peek(transmit + 1), Some(0xff));
            assert_eq!(restored.peek(transmit + 5), Some(0));
            let mut unsupported = Vec::new();
            store_byte(&mut unsupported, transmit + 5, 0x30);
            unsupported.extend_from_slice(&[0x4e71; 2]);
            let mut rejected = new(&unsupported, &[]);
            assert!(rejected.step_instruction().is_err());
            assert_eq!(rejected.peek(transmit + 5), Some(0));
        }
    }
}

pub fn concurrent_controller_access() {
    let sound = [0x00, 0x18, 0xfd];
    let mut program = load_program(&sound);
    store_word(&mut program, BUS_REQUEST, 0);
    store_byte(&mut program, 0xa1_000b, 0x40);
    store_byte(&mut program, 0xa1_0005, 0x40);
    read_byte(&mut program, 0xa1_0005, 0);
    program.extend_from_slice(&[0x4e71; 8]);
    let mut machine = Emulator::new(
        image(&program, &[]),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    machine
        .input_mut()
        .set_controller_raw(ControllerPort::Two, 0xef);
    for _ in 0..sound.len() + 3 {
        instruction(&mut machine);
    }
    let before = machine.sound_cpu().cycles();
    let mut copied = machine.clone();
    for _ in 0..3 {
        assert_eq!(machine.step_instruction(), copied.step_instruction());
        assert_eq!(machine, copied);
    }
    assert_eq!(machine.registers().d[0], 0x6f);
    assert!(machine.sound_cpu().cycles() > before);
    assert!(!machine.sound_bus_granted());
}
