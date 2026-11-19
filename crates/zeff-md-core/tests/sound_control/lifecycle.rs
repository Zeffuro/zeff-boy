use super::fixture::*;
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::{ControllerKind, ControllerPort, Emulator, LoadConfig};
use zeff_z80::CpuState;

pub fn control_mirror_hot_cpu_and_clone() {
    let mut sound = vec![0; 0x39];
    sound[..8].copy_from_slice(&[0x3e, 0x55, 0x31, 0, 0x10, 0xfb, 0, 0x76]);
    sound[0x38] = 0x76;
    for offset in (0..=0xfe).step_by(2) {
        let mut handler = Vec::new();
        store_byte(&mut handler, 0xa1_0009, 0x40);
        for level in [0, 0x40, 0, 0x40, 0, 0x40] {
            store_byte(&mut handler, 0xa1_0003, level);
        }
        for requested in [true, true, false, false] {
            store_word(
                &mut handler,
                BUS_REQUEST + offset,
                u16::from(requested) << 8,
            );
        }
        for released in [true, true, false, false, true] {
            store_word(&mut handler, RESET + offset, u16::from(released) << 8);
        }
        store_word(&mut handler, BUS_REQUEST + offset, 0);
        handler.extend_from_slice(&[0x4e72, 0x2200, 0x4e71, 0x4e71]);
        let mut machine = stopped_with_bank(&sound, &handler, 0x155);
        machine
            .input_mut()
            .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
        machine
            .input_mut()
            .set_controller_raw(ControllerPort::One, 0xff);
        machine
            .input_mut()
            .set_controller_extra_raw(ControllerPort::One, 1);
        machine.step_sound_instruction().unwrap();
        machine.step_sound_instruction().unwrap();
        assert_eq!(machine.sound_cpu().regs().a, 0x55);
        assert_eq!(machine.sound_cpu().regs().sp, 0x1000);
        let hot = machine.sound_cpu().clone();
        machine.set_sound_interrupt(true);
        wake(&mut machine);
        machine.set_interrupt_level(2);
        for _ in 0..7 {
            instruction(&mut machine);
        }
        assert_eq!(machine.peek(0xa1_0003), Some(0x71));
        let input = machine.input().clone();
        let mut copied = machine.clone();
        for index in 0..10 {
            let before = machine.timing_snapshot();
            let step = machine.step_instruction().unwrap();
            assert_eq!(copied.step_instruction(), Ok(step.clone()));
            assert_eq!(machine, copied);
            assert_eq!(
                machine.timing_snapshot().elapsed_since(before).unwrap(),
                step.elapsed
            );
            assert!(step.elapsed.get() > 0);
            if index < 6 {
                assert_eq!(machine.sound_cpu(), &hot);
            } else {
                assert_eq!(machine.sound_cpu().cycles(), 0);
                assert_eq!(machine.sound_cpu().regs().a, 0);
                assert_eq!(machine.sound_cpu().regs().pc, 0);
                assert_eq!(machine.sound_cpu().regs().sp, 0);
                assert_eq!(machine.sound_cpu().state(), CpuState::Running);
            }
            assert_eq!(machine.sound_bank(), 0x155);
            assert_eq!(machine.peek(SOUND_RAM), Some(0x3e));
            assert_eq!(machine.peek(0xa1_0003), Some(0x71));
            assert_eq!(machine.input(), &input);
            assert_eq!(machine.interrupt_level(), 2);
        }
        assert!(!machine.sound_reset_asserted());
        assert!(!machine.sound_bus_requested() && !machine.sound_bus_granted());
        instruction(&mut machine);
        assert_eq!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Stopped
        );
        assert_eq!(machine.interrupt_level(), 2);
        machine.set_interrupt_level(0);
        for _ in 0..4 {
            machine.step_sound_instruction().unwrap();
        }
        assert_eq!(machine.sound_cpu().regs().pc, 7);
        assert_eq!(machine.step_sound_instruction().unwrap().cycles, 13);
        assert!(machine.sound_cpu().last_step_was_interrupt());
        assert_eq!(machine.sound_cpu().regs().pc, 0x38);
        assert_eq!(machine.sound_cpu().regs().sp, 0x0ffe);
        let now = machine.timing_snapshot().now();
        machine.reset_sound_cpu();
        assert_eq!(machine.timing_snapshot().now(), now);
        assert_eq!(machine.input(), &input);
        assert_eq!(machine.sound_bank(), 0x155);
        Reset::reset(&mut machine);
        assert!(machine.sound_reset_asserted());
        assert!(!machine.sound_bus_requested() && !machine.sound_bus_granted());
        assert_eq!(machine.peek(SOUND_RAM), Some(0));
        assert_eq!(machine.sound_bank(), 0);
        assert_eq!(
            machine.input().controller_kind(ControllerPort::One),
            ControllerKind::SixButton
        );
        assert_eq!(
            machine.input().controller_extra_raw(ControllerPort::One),
            15
        );
    }
}

fn equivalent(left: &Emulator, right: &Emulator) {
    assert_eq!(left.registers(), right.registers());
    assert_eq!(left.cpu_clock(), right.cpu_clock());
    assert_eq!(left.timing_snapshot(), right.timing_snapshot());
    assert_eq!(left.sound_cpu(), right.sound_cpu());
    assert_eq!(left.sound_bus_requested(), right.sound_bus_requested());
    assert_eq!(left.sound_bus_granted(), right.sound_bus_granted());
    assert_eq!(left.sound_reset_asserted(), right.sound_reset_asserted());
    assert_eq!(left.sound_bank(), right.sound_bank());
    assert_eq!(left.sound_deadline(), right.sound_deadline());
    assert_eq!(
        left.sound_instruction_pending(),
        right.sound_instruction_pending()
    );
    assert_eq!(left.sound_prefix_pending(), right.sound_prefix_pending());
    assert_eq!(left.sound_fault(), right.sound_fault());
    assert_eq!(left.input(), right.input());
    assert_eq!(left.interrupt_level(), right.interrupt_level());
    for address in [SOUND_RAM, SOUND_RAM + 0x100, SOUND_RAM + 0x101, 0xff0000] {
        assert_eq!(left.peek(address), right.peek(address));
    }
}

pub fn control_mirror_concurrent_lifecycle() {
    let sound = [0x3e, 0x5a, 0x32, 0, 1, 0x76];
    for offset in (0..=0xfe).step_by(2) {
        for reset in [false, true] {
            let mut machines = [0, offset].map(|mirror| {
                let mut program = load_program(&sound);
                store_word(&mut program, BUS_REQUEST, 0);
                program.extend_from_slice(&[0x4e71; 3]);
                if reset {
                    store_word(&mut program, RESET + mirror, 0);
                    store_word(&mut program, BUS_REQUEST + mirror, 0x0100);
                    store_word(&mut program, RESET + mirror, 0x0100);
                } else {
                    store_word(&mut program, BUS_REQUEST + mirror, 0x0100);
                    program.extend_from_slice(&[0x3039, 0x00a1, (BUS_REQUEST + mirror) as u16]);
                    store_byte(&mut program, SOUND_RAM + 0x101, 0xa6);
                }
                store_word(&mut program, BUS_REQUEST + mirror, 0);
                program.extend_from_slice(&[0x4e71; 64]);
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
                    .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
                machine
                    .input_mut()
                    .set_controller_extra_raw(ControllerPort::One, 5);
                machine.set_interrupt_level(2);
                for _ in 0..sound.len() + 6 {
                    instruction(&mut machine);
                }
                machine
            });
            equivalent(&machines[0], &machines[1]);
            assert_eq!(machines[0].sound_cpu().regs().a, 0x5a);
            assert_eq!(machines[0].sound_cpu().regs().pc, 2);
            assert_eq!(machines[0].sound_cpu().cycles(), 7);
            assert!(!machines[0].sound_instruction_pending());
            assert!(machines[0].sound_deadline().is_some());
            assert_eq!(machines[0].peek(SOUND_RAM + 0x100), Some(0));
            let mut copied = machines[1].clone();
            let mut warm = machines[1].clone();
            let now = warm.timing_snapshot().now();
            warm.reset_sound_cpu();
            assert_eq!(warm.timing_snapshot().now(), now);
            assert!(!warm.sound_instruction_pending());
            assert!(warm.sound_deadline().is_none());
            assert_eq!(warm.peek(SOUND_RAM + 0x100), Some(0));
            assert_eq!(warm.interrupt_level(), 2);
            assert_eq!(warm.input(), machines[1].input());
            let mut cold = machines[1].clone();
            Reset::reset(&mut cold);
            assert!(cold.sound_reset_asserted());
            assert!(!cold.sound_instruction_pending());
            assert!(cold.sound_deadline().is_none());
            assert_eq!(cold.interrupt_level(), 0);
            assert_eq!(cold.peek(SOUND_RAM), Some(0));
            for index in 0..68 {
                let left = machines[0].step_instruction().unwrap();
                let right = machines[1].step_instruction().unwrap();
                assert_eq!(left, right);
                assert_eq!(copied.step_instruction(), Ok(right));
                assert_eq!(machines[1], copied);
                equivalent(&machines[0], &machines[1]);
                if index == 0 && reset {
                    assert!(machines[1].sound_reset_asserted());
                    assert!(!machines[1].sound_instruction_pending());
                    assert!(machines[1].sound_deadline().is_none());
                    assert_eq!(machines[1].sound_cpu().cycles(), 0);
                    assert_eq!(machines[1].peek(SOUND_RAM + 0x100), Some(0));
                }
                if index == 2 {
                    assert!(machines[1].sound_bus_granted());
                    if reset {
                        assert_eq!(machines[1].peek(SOUND_RAM + 0x100), Some(0));
                    } else {
                        assert_eq!(machines[1].registers().d[0] & 0x0100, 0);
                        assert!(machines[1].sound_instruction_pending());
                        assert_eq!(machines[1].sound_cpu().regs().pc, 5);
                        assert_eq!(machines[1].sound_cpu().cycles(), 17);
                        assert_eq!(machines[1].peek(SOUND_RAM + 0x100), Some(0));
                        assert_eq!(machines[1].peek(SOUND_RAM + 0x101), Some(0xa6));
                    }
                }
            }
            assert_eq!(machines[1].sound_cpu().state(), CpuState::Halted);
            assert_eq!(machines[1].peek(SOUND_RAM + 0x100), Some(0x5a));
            assert_eq!(machines[1].interrupt_level(), 2);
            for address in 0..0x2000 {
                assert_eq!(
                    machines[0].peek(SOUND_RAM + address),
                    machines[1].peek(SOUND_RAM + address)
                );
            }
        }
    }
}
