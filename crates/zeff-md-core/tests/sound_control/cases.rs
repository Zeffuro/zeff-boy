use super::fixture::*;
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, MachineFault};
use zeff_z80::CpuState;

const REQUEST_MIRROR: u32 = BUS_REQUEST + 0xfe;
const RESET_MIRROR: u32 = RESET + 0x7e;

pub fn control_mirror_request_and_reset() {
    let mut program = Vec::new();
    store_word(&mut program, REQUEST_MIRROR, 0x0100);
    program.extend_from_slice(&[0x3039, 0x00a1, 0x11fe]);
    store_word(&mut program, RESET_MIRROR, 0x0100);
    store_byte(&mut program, SOUND_RAM, 0x5a);
    store_word(&mut program, RESET_MIRROR, 0);
    program.extend_from_slice(&[0x3239, 0x00a1, 0x11fe]);
    store_word(&mut program, RESET_MIRROR, 0x0100);
    program.extend_from_slice(&[0x3439, 0x00a1, 0x11fe, 0x4e71]);
    let mut machine = new(&program, &[]);
    instruction(&mut machine);
    assert!(machine.sound_reset_asserted());
    assert!(machine.sound_bus_requested() && !machine.sound_bus_granted());
    instruction(&mut machine);
    assert_eq!(machine.registers().d[0] & 0x0100, 0x0100);
    instruction(&mut machine);
    assert!(!machine.sound_reset_asserted());
    assert!(machine.sound_bus_granted());
    instruction(&mut machine);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x5a));
    let before = machine.timing_snapshot();
    let step = machine.step_instruction().unwrap();
    assert_eq!(
        machine.timing_snapshot().elapsed_since(before).unwrap(),
        step.elapsed
    );
    assert!(step.elapsed.get() > 0);
    assert!(machine.sound_reset_asserted());
    assert!(machine.sound_bus_requested() && !machine.sound_bus_granted());
    assert_eq!(machine.sound_cpu().cycles(), 0);
    assert_eq!(machine.sound_cpu().state(), CpuState::Running);
    assert_eq!(machine.peek(SOUND_RAM), Some(0x5a));
    instruction(&mut machine);
    assert_eq!(machine.registers().d[1] & 0x0100, 0x0100);
    instruction(&mut machine);
    assert!(!machine.sound_reset_asserted());
    assert!(machine.sound_bus_granted());
    instruction(&mut machine);
    assert_eq!(machine.registers().d[2] & 0x0100, 0);
}

pub fn control_mirror_lanes_and_bits() {
    for offset in (0..=0xfe).step_by(2) {
        for register in [BUS_REQUEST, RESET] {
            for word in [false, true] {
                for ignored in [0, 0x00ff, 0xfe00, 0xfeff] {
                    for logical_high in [0, 0xff00_0000] {
                        let mut program = load_program(&[]);
                        for asserted in [false, true, true, false] {
                            let value = ignored | if asserted { 0x0100 } else { 0 };
                            let address = logical_high | (register + offset);
                            if word {
                                store_word(&mut program, address, value);
                            } else {
                                store_byte(&mut program, address, (value >> 8) as u8);
                            }
                        }
                        program.extend_from_slice(&[0x4e71; 2]);
                        let mut machine = new(&program, &[]);
                        for _ in 0..2 {
                            instruction(&mut machine);
                        }
                        let cpu = machine.sound_cpu().clone();
                        for asserted in [false, true, true, false] {
                            instruction(&mut machine);
                            if register == BUS_REQUEST {
                                assert_eq!(machine.sound_bus_requested(), asserted);
                                assert_eq!(machine.sound_bus_granted(), asserted);
                                assert!(!machine.sound_reset_asserted());
                                assert_eq!(machine.sound_cpu(), &cpu);
                            } else {
                                assert_eq!(machine.sound_reset_asserted(), !asserted);
                                assert_eq!(machine.sound_bus_granted(), asserted);
                                assert!(machine.sound_bus_requested());
                                assert_eq!(machine.sound_cpu().cycles(), 0);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn control_mirror_busy_reads() {
    for offset in (0..=0xfe).step_by(2) {
        for requested in [false, true] {
            for reset in [false, true] {
                for word in [false, true] {
                    let mut program = Vec::new();
                    store_word(&mut program, BUS_REQUEST, u16::from(requested) << 8);
                    store_word(&mut program, RESET, u16::from(!reset) << 8);
                    program.extend_from_slice(&[
                        if word { 0x3039 } else { 0x1039 },
                        0x00a1,
                        (BUS_REQUEST + offset) as u16,
                        0x4e71,
                        0x4e71,
                    ]);
                    let mut machine = new(&program, &[]);
                    for _ in 0..3 {
                        instruction(&mut machine);
                    }
                    let mask = if word { 0x0100 } else { 1 };
                    assert_eq!(
                        machine.registers().d[0] & mask,
                        if requested && !reset { 0 } else { mask }
                    );
                    assert_eq!(machine.sound_bus_requested(), requested);
                    assert_eq!(machine.sound_bus_granted(), requested && !reset);
                    assert_eq!(machine.sound_reset_asserted(), reset);
                }
            }
        }
    }
}

pub fn control_mirror_adjacent_blocks() {
    for address in [
        0xa1_1000, 0xa1_10fe, 0xa1_1300, 0xa1_13fe, 0xa2_1100, 0xa2_1200,
    ] {
        for word in [false, true] {
            for write in [false, true] {
                let mut program = load_program(&[]);
                if write {
                    if word {
                        store_word(&mut program, address, 0x0100);
                    } else {
                        store_byte(&mut program, address, 1);
                    }
                } else {
                    program.extend_from_slice(&[
                        if word { 0x3039 } else { 0x1039 },
                        (address >> 16) as u16,
                        address as u16,
                    ]);
                }
                program.extend_from_slice(&[0x4e71; 2]);
                let mut machine = new(&program, &[]);
                for _ in 0..2 {
                    instruction(&mut machine);
                }
                let cpu = machine.sound_cpu().clone();
                assert!(matches!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Bus {
                        source: BusError::UnsupportedAccess { .. },
                        ..
                    }))
                ));
                assert!(machine.sound_bus_requested() && machine.sound_bus_granted());
                assert!(!machine.sound_reset_asserted());
                assert_eq!(machine.sound_cpu(), &cpu);
                let frozen = machine.clone();
                assert_eq!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Frozen))
                );
                assert_eq!(machine, frozen);
            }
        }
    }
}
