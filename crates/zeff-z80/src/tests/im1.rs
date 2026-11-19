use super::*;

pub(super) fn check() {
    for halted in [false, true] {
        for sp in [0, 1, 0x2000] {
            for waits in [0, 2] {
                let mut cpu = Cpu::new();
                cpu.regs.pc = 0xffed;
                cpu.regs.sp = sp;
                cpu.regs.i = 0xa7;
                cpu.regs.r = 0xff;
                cpu.interrupt_mode = InterruptMode::Im1;
                cpu.interrupt_flip_flop_1 = true;
                cpu.interrupt_flip_flop_2 = true;
                cpu.state = if halted {
                    CpuState::Halted
                } else {
                    CpuState::Running
                };
                let mut state = BusState::default();
                let mut legacy = cpu.clone();
                let mut legacy_state = state;
                let mut bus = Bus::new(&[]);
                bus.irq_pending = true;
                for phase in 0..3 {
                    let before = (cpu.clone(), state);
                    let request = cpu.next_im1_cycle(&state, phase == 0).unwrap();
                    let receipt = match request.kind() {
                        Im1CycleKind::Acknowledge { pc, refresh } => {
                            assert_eq!(phase, 0);
                            assert_eq!((pc, refresh), (0xffed, 0xa7ff));
                            assert_eq!(request.kind().t_states(), 7);
                            assert!(
                                cpu.next_im1_cycle(&state, true)
                                    .unwrap()
                                    .complete(
                                        Im1Receipt::Written {
                                            address: pc,
                                            value: 0
                                        },
                                        0
                                    )
                                    .is_err()
                            );
                            Im1Receipt::Acknowledged
                        }
                        Im1CycleKind::StackWrite {
                            address,
                            value,
                            final_write,
                        } => {
                            assert_eq!(request.kind().t_states(), 3);
                            assert_eq!(address, sp.wrapping_sub(phase));
                            assert_eq!(value, if phase == 1 { 0xff } else { 0xed });
                            assert_eq!(final_write, phase == 2);
                            assert!(
                                cpu.next_im1_cycle(&state, false)
                                    .unwrap()
                                    .complete(
                                        Im1Receipt::Written {
                                            address: address ^ 1,
                                            value
                                        },
                                        0
                                    )
                                    .is_err()
                            );
                            Im1Receipt::Written { address, value }
                        }
                    };
                    assert_eq!((cpu.clone(), state), before);
                    let mut stale = cpu.clone();
                    stale.regs.r ^= 1;
                    let saved = (stale.clone(), state);
                    let stale_completion = cpu
                        .next_im1_cycle(&state, phase == 0)
                        .unwrap()
                        .complete(receipt, waits)
                        .unwrap();
                    assert!(
                        stale
                            .retire_im1_cycle(&mut state, stale_completion)
                            .is_err()
                    );
                    assert_eq!((stale, state), saved);
                    let completion = request.complete(receipt, waits).unwrap();
                    let step = cpu.retire_im1_cycle(&mut state, completion).unwrap();
                    let mut expected = legacy
                        .step_bus_segment(&mut bus, &mut legacy_state)
                        .unwrap();
                    assert!(legacy.extend_bus_segment(&mut expected.instruction, waits));
                    assert_eq!(step, expected);
                    assert_eq!((&cpu, state), (&legacy, legacy_state));
                    assert_eq!(step.complete, phase == 2);
                    let mut writer = crate::state::Writer::new();
                    cpu.write_continuation(&state, &mut writer);
                    let bytes = writer.into_bytes();
                    let mut restored = Cpu::new();
                    let mut restored_state = BusState::default();
                    restored
                        .read_continuation(
                            &mut restored_state,
                            &mut crate::state::Reader::new(&bytes),
                        )
                        .unwrap();
                    assert_eq!((&restored, restored_state), (&cpu, state));
                    if phase < 2 {
                        assert_eq!(
                            restored
                                .next_im1_cycle(&restored_state, false)
                                .unwrap()
                                .kind(),
                            cpu.next_im1_cycle(&state, false).unwrap().kind()
                        );
                    }
                }
                assert_eq!(
                    (cpu.regs.pc, cpu.regs.sp, cpu.regs.r),
                    (0x38, sp.wrapping_sub(2), 0x80)
                );
                assert!(!cpu.interrupts_enabled() && !cpu.saved_interrupts_enabled());
                assert_eq!(cpu.cycles(), 13 + u64::from(waits) * 3);
            }
        }
    }
    for mode in [InterruptMode::Im0, InterruptMode::Im1, InterruptMode::Im2] {
        for accepted in [false, true] {
            for delay in [0, 1, 2] {
                let mut cpu = Cpu::new();
                cpu.interrupt_mode = mode;
                cpu.interrupt_flip_flop_1 = true;
                cpu.enable_interrupts_delay = delay;
                let state = BusState::default();
                assert_eq!(
                    cpu.next_im1_cycle(&state, accepted).is_ok(),
                    mode == InterruptMode::Im1 && accepted && delay == 0
                );
            }
        }
    }
    let mut cpu = Cpu::new();
    cpu.interrupt_mode = InterruptMode::Im1;
    cpu.interrupt_flip_flop_1 = true;
    cpu.cycles = u64::MAX - 6;
    let before = cpu.clone();
    assert!(
        cpu.next_im1_cycle(&BusState::default(), true)
            .unwrap()
            .complete(Im1Receipt::Acknowledged, 0)
            .is_err()
    );
    assert_eq!(cpu, before);
}
