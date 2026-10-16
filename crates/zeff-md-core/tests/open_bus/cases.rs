use super::fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, ControllerKind, ControllerPort, Emulator, LoadConfig, MachineFault};

pub fn open_bus_literal_prefetch() {
    for concurrent_sound in [false, true] {
        for granted in [false, true] {
            for next in [0x7a55, 0x4e71] {
                let mut program = Vec::new();
                if granted {
                    store_word(&mut program, BUS_REQUEST, 0x100);
                    store_word(&mut program, RESET, 0x100);
                }
                program.extend_from_slice(&[0x1039, 0x00a1, 0x1100, next, 0x4e71]);
                let mut machine = Emulator::new(
                    image(&program, &[]),
                    LoadConfig {
                        concurrent_sound,
                        ..Default::default()
                    },
                )
                .unwrap();
                if granted {
                    instruction(&mut machine);
                    instruction(&mut machine);
                }
                assert_eq!(machine.queued_words(), Some([0x1039, 0x00a1]));
                let before = machine.timing_snapshot();
                let clock = machine.cpu_clock();
                let step = machine.step_instruction().unwrap();
                let expected = ((next >> 8) & 0xfe) | u16::from(!granted);
                assert_eq!(machine.registers().d[0], u32::from(expected));
                assert_eq!(machine.cpu_clock().0 - clock.0, 16);
                assert_eq!(step.elapsed.get(), 112);
                assert_eq!(
                    machine.timing_snapshot().elapsed_since(before).unwrap(),
                    step.elapsed
                );
                assert_eq!(machine.registers().pc, if granted { 0x116 } else { 0x106 });
                assert_eq!(machine.queued_words(), Some([next, 0x4e71]));
                assert_eq!(machine.sound_bus_granted(), granted);
                assert_eq!(machine.sound_reset_asserted(), !granted);
            }
        }
    }
}

fn machine(program: &[u16], concurrent_sound: bool) -> Emulator {
    Emulator::new(
        image(program, &[]),
        LoadConfig {
            concurrent_sound,
            ..Default::default()
        },
    )
    .unwrap()
}

fn setup(program: &mut Vec<u16>, granted: bool) -> usize {
    if granted {
        store_word(program, BUS_REQUEST, 0x100);
        store_word(program, RESET, 0x100);
    }
    program.extend_from_slice(&[0x203c, 0x1234, 0x5600]);
    1 + 2 * usize::from(granted)
}

fn controls(machine: &Emulator, granted: bool) {
    assert_eq!(machine.sound_bus_requested(), granted);
    assert_eq!(machine.sound_bus_granted(), granted);
    assert_eq!(machine.sound_reset_asserted(), !granted);
}

fn read_absolute(address: u32, next: u16, granted: bool, concurrent_sound: bool) {
    let mut program = Vec::new();
    let prefix = setup(&mut program, granted);
    program.extend_from_slice(&[0x1039, (address >> 16) as u16, address as u16, next, 0x4e71]);
    let mut machine = machine(&program, concurrent_sound);
    for _ in 0..prefix {
        instruction(&mut machine);
    }
    assert_eq!(
        machine.queued_words(),
        Some([0x1039, (address >> 16) as u16])
    );
    let pc = machine.registers().pc;
    let before = machine.timing_snapshot();
    let clock = machine.cpu_clock();
    let step = machine.step_instruction().unwrap();
    let byte = u32::from((next >> 8) & 0xfe) | u32::from(!granted);
    assert_eq!(machine.registers().d[0], 0x1234_5600 | byte);
    assert_eq!(machine.registers().pc, pc + 6);
    assert_eq!(machine.queued_words(), Some([next, 0x4e71]));
    assert_eq!(machine.cpu_clock().0 - clock.0, 16);
    assert_eq!(step.elapsed.get(), 112);
    assert_eq!(
        machine.timing_snapshot().elapsed_since(before).unwrap(),
        step.elapsed
    );
    controls(&machine, granted);
}

pub fn open_bus_aliases_and_logical_addresses() {
    for offset in (0..=0xfe).step_by(2) {
        for concurrent_sound in [false, true] {
            for granted in [false, true] {
                for next in [0x7a55, 0x4e71] {
                    for logical_high in [0, 0xff00_0000] {
                        read_absolute(
                            logical_high | (BUS_REQUEST + offset),
                            next,
                            granted,
                            concurrent_sound,
                        );
                    }
                }
            }
        }
    }
}

pub fn open_bus_all_prefetched_high_bytes_without_execution() {
    for high in 0..=255u16 {
        for concurrent_sound in [false, true] {
            for granted in [false, true] {
                read_absolute(
                    BUS_REQUEST + 0xfe,
                    (high << 8) | 0x55,
                    granted,
                    concurrent_sound,
                );
            }
        }
    }
}

pub fn open_bus_work_ram_prefetch() {
    for concurrent_sound in [false, true] {
        for granted in [false, true] {
            for target in [0x00e0_0100, 0xffff_0100] {
                let mut program = Vec::new();
                let prefix = setup(&mut program, granted);
                let words = [0x1039, 0xffa1, 0x11fe, 0x7a55, 0x4e71];
                for (index, word) in words.into_iter().enumerate() {
                    store_word(&mut program, 0xe0_0100 + index as u32 * 2, word);
                }
                program.extend_from_slice(&[0x4ef9, (target >> 16) as u16, target as u16, 0x4e71]);
                let mut machine = machine(&program, concurrent_sound);
                for _ in 0..prefix + words.len() + 1 {
                    instruction(&mut machine);
                }
                assert_eq!(machine.registers().pc, target);
                assert_eq!(machine.queued_words(), Some([0x1039, 0xffa1]));
                assert_eq!(machine.peek(0xe0_0106), Some(0x7a));
                assert_eq!(machine.peek(0xff_0106), Some(0x7a));
                let clock = machine.cpu_clock();
                instruction(&mut machine);
                assert_eq!(machine.registers().d[0], 0x1234_567a | u32::from(!granted));
                assert_eq!(machine.registers().pc, target + 6);
                assert_eq!(machine.queued_words(), Some([0x7a55, 0x4e71]));
                assert_eq!(machine.cpu_clock().0 - clock.0, 16);
                controls(&machine, granted);
            }
        }
    }
}

pub fn open_bus_indirect_clone_and_reset() {
    for concurrent_sound in [false, true] {
        for granted in [false, true] {
            let mut program = Vec::new();
            let prefix = setup(&mut program, granted);
            program.extend_from_slice(&[0x207c, 0xffa1, 0x11fe, 0x1010, 0x7a55, 0x4e71]);
            let mut machine = machine(&program, concurrent_sound);
            machine
                .input_mut()
                .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
            machine
                .input_mut()
                .set_controller_extra_raw(ControllerPort::One, 5);
            machine.set_interrupt_level(2);
            for _ in 0..prefix + 1 {
                instruction(&mut machine);
            }
            let input = machine.input().clone();
            let mut cloned = machine.clone();
            let mut warm = machine.clone();
            let now = warm.timing_snapshot().now();
            warm.reset_cpu().unwrap();
            assert_eq!(warm.timing_snapshot().now().get() - now.get(), 56);
            assert_eq!(warm.cpu_clock().0, 8);
            assert_eq!(warm.input(), &input);
            assert_eq!(warm.interrupt_level(), 2);
            controls(&warm, granted);
            let mut cold = machine.clone();
            Reset::reset(&mut cold);
            assert_eq!(cold.timing_snapshot().now().get(), 56);
            assert_eq!(cold.cpu_clock().0, 8);
            assert_eq!(cold.interrupt_level(), 0);
            controls(&cold, false);
            assert_eq!(
                cold.input().controller_kind(ControllerPort::One),
                ControllerKind::SixButton
            );
            assert_eq!(cold.input().controller_extra_raw(ControllerPort::One), 15);
            for restarted in [&mut warm, &mut cold] {
                for _ in 0..prefix + 1 {
                    instruction(restarted);
                }
                assert_eq!(restarted.queued_words(), Some([0x1010, 0x7a55]));
            }
            let pc = machine.registers().pc;
            let clock = machine.cpu_clock();
            let step = machine.step_instruction().unwrap();
            assert_eq!(cloned.step_instruction(), Ok(step));
            assert_eq!(machine, cloned);
            for candidate in [&machine, &cloned] {
                assert_eq!(candidate.input(), &input);
                assert_eq!(candidate.interrupt_level(), 2);
            }
            for restarted in [&mut warm, &mut cold] {
                instruction(restarted);
            }
            for candidate in [&machine, &warm, &cold] {
                assert_eq!(
                    candidate.registers().d[0],
                    0x1234_567a | u32::from(!granted)
                );
                assert_eq!(candidate.registers().pc, pc + 2);
                assert_eq!(candidate.queued_words(), Some([0x7a55, 0x4e71]));
                controls(candidate, granted);
            }
            assert_eq!(machine.cpu_clock().0 - clock.0, 8);
        }
    }
}

pub fn open_bus_guards_and_consumed_observation() {
    for concurrent_sound in [false, true] {
        for granted in [false, true] {
            for address in [
                RESET,
                RESET + 0xfe,
                BUS_REQUEST + 1,
                BUS_REQUEST + 0xff,
                0xa1_1000,
                0xa1_10fe,
                0xa1_1300,
                0xa1_13fe,
                0xa2_1100,
            ] {
                let mut program = Vec::new();
                let prefix = setup(&mut program, granted);
                program.extend_from_slice(&[
                    0x1039,
                    (address >> 16) as u16,
                    address as u16,
                    0x7a55,
                    0x4e71,
                ]);
                let mut machine = machine(&program, concurrent_sound);
                for _ in 0..prefix {
                    instruction(&mut machine);
                }
                let input = machine.input().clone();
                let sound = machine.sound_cpu().clone();
                assert!(matches!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Bus {
                        source: BusError::UnsupportedAccess { .. },
                        ..
                    }))
                ));
                assert_eq!(machine.registers().d[0], 0x1234_5600);
                controls(&machine, granted);
                assert_eq!(machine.input(), &input);
                assert_eq!(machine.sound_cpu(), &sound);
                let frozen = machine.clone();
                assert_eq!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Frozen))
                );
                assert_eq!(machine, frozen);
            }
            let mut program = Vec::new();
            let prefix = setup(&mut program, granted);
            program.extend_from_slice(&[0x207c, 0x00a1, 0x1100, 0x0108, 0, 0x7a55, 0x4e71]);
            let mut machine = machine(&program, concurrent_sound);
            for _ in 0..prefix + 1 {
                instruction(&mut machine);
            }
            let pc = machine.registers().pc;
            let clock = machine.cpu_clock();
            instruction(&mut machine);
            let busy = u32::from(!granted);
            assert_eq!(
                machine.registers().d[0],
                0x1234_0000 | ((0x7a | busy) << 8) | busy
            );
            assert_eq!(machine.registers().pc, pc + 4);
            assert_eq!(machine.queued_words(), Some([0x7a55, 0x4e71]));
            assert_eq!(machine.cpu_clock().0 - clock.0, 16);
            controls(&machine, granted);
        }
    }
}
