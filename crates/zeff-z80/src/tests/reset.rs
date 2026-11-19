use super::*;

#[test]
fn reset_uses_the_configured_host_state() {
    let reset_state = ResetState::new(0x1234, 0xabcd);
    let mut cpu = Cpu::new_with_reset(reset_state);
    cpu.regs.pc = 0;
    cpu.regs.sp = 0;
    cpu.reset();
    assert_eq!(cpu.regs().pc, reset_state.pc);
    assert_eq!(cpu.regs().sp, reset_state.sp);

    for program in [&[0xdd, 0x21, 0x34, 0x12][..], &[0x3e, 0x5a], &[0x76]] {
        for cycles in [17, u64::MAX] {
            let mut cpu = Cpu::new_with_reset(reset_state);
            cpu.regs.pc = 0;
            let mut memory = Bus::new(program);
            let mut state = BusState::default();
            cpu.step_bus_segment(&mut memory, &mut state).unwrap();
            assert!(state.pending() || cpu.is_halted());
            cpu.regs = Registers {
                a: 0x11,
                f: 0x22,
                b: 0x33,
                c: 0x44,
                d: 0x55,
                e: 0x66,
                h: 0x77,
                l: 0x88,
                ix: 0x1234,
                iy: 0x5678,
                sp: 0x9abc,
                i: 0xde,
                r: 0xf0,
                ..cpu.regs
            };
            cpu.shadow = ShadowRegisters {
                a: 0x12,
                f: 0x23,
                b: 0x34,
                c: 0x45,
                d: 0x56,
                e: 0x67,
                h: 0x78,
                l: 0x89,
            };
            cpu.cycles = cycles;
            cpu.interrupt_mode = InterruptMode::Im2;
            cpu.interrupt_flip_flop_1 = true;
            cpu.interrupt_flip_flop_2 = true;
            cpu.enable_interrupts_delay = 1;
            let registers = cpu.regs;
            let shadow = cpu.shadow;

            cpu.apply_qualified_reset(&mut state).unwrap();
            assert_eq!(
                cpu.regs,
                Registers {
                    pc: 0,
                    i: 0,
                    r: 0,
                    ..registers
                }
            );
            assert_eq!(cpu.shadow, shadow);
            assert_eq!(cpu.cycles(), cycles);
            assert_eq!(cpu.reset_state, reset_state);
            assert_eq!(cpu.state(), CpuState::Running);
            assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);
            assert!(!cpu.interrupts_enabled() && !cpu.saved_interrupts_enabled());
            assert!(!cpu.interrupt_enable_pending() && !cpu.last_step_was_interrupt());
            assert_eq!(
                (
                    cpu.last_opcode_pc(),
                    cpu.last_opcode(),
                    cpu.last_m1_fetch_count()
                ),
                (0, 0, 0)
            );
            assert!(cpu.instruction_bytes().is_empty() && cpu.trap().is_none());
            assert_eq!(state, BusState::default());
            if cycles == 17 {
                memory.memory[0] = 0;
                let step = cpu.step_bus_segment(&mut memory, &mut state).unwrap();
                assert_eq!((step.instruction.pc, step.instruction.opcode), (0, 0));
                assert_eq!((cpu.regs().pc, cpu.cycles()), (1, 21));
            }
        }
    }

    for trapped in [false, true] {
        let mut cpu = Cpu::new();
        let mut state = BusState::default();
        cpu.step_bus_segment(&mut Bus::new(&[0xdd]), &mut state)
            .unwrap();
        if trapped {
            cpu.trap = Some(CpuTrap::UnsupportedOpcode { pc: 0, opcode: 0 });
        } else {
            cpu.suspend();
        }
        let before = (cpu.clone(), state);
        assert_eq!(
            cpu.apply_qualified_reset(&mut state),
            Err(CycleError::UnsupportedState)
        );
        assert_eq!((cpu, state), before);
    }
}
