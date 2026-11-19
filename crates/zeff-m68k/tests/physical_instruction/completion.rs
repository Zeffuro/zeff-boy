use super::*;

#[test]
fn selected_sequential_completions_preserve_register_banks_and_actual_queue() {
    let opcodes: Vec<_> = std::iter::once(0x4e71)
        .chain((0..8).flat_map(|register| {
            [0, 1, 127, 128, 255].map(|immediate| 0x7000 | register << 9 | immediate)
        }))
        .collect();
    for supervisor in [false, true] {
        for ccr in 0..32 {
            for &opcode in &opcodes {
                for wait in [0, 9] {
                    for pc in [0x1234_2000, 0xffff_fffc] {
                        let initial = registers(if supervisor { SUPERVISOR } else { 0 } | ccr, pc);
                        let mut cpu =
                            PrefetchCpu::from_prefetched(initial, [opcode, 0x1234], CpuClocks(31))
                                .unwrap();
                        let mut bus = Device::new(31);
                        bus.instruction_elapsed[0] = 4 + wait;
                        bus.word(pc.wrapping_add(4), 0x9876);
                        let result = cpu.step_physical_instruction(&mut bus).unwrap();
                        let mut expected = initial;
                        if opcode != 0x4e71 {
                            moveq(&mut expected, usize::from((opcode >> 9) & 7), opcode as u8);
                        }
                        expected.pc = pc.wrapping_add(2);
                        assert_eq!(cpu.registers, expected);
                        assert_eq!(cpu.queued_words(), Some([0x1234, 0x9876]));
                        assert_eq!(
                            result,
                            PhysicalInstructionStep::Instruction(zeff_m68k::timing::TimedStep {
                                opcode,
                                clocks: u64::from(4 + wait),
                                exception: None
                            })
                        );
                        assert_eq!(
                            bus.events,
                            vec![Event::Instruction(
                                access(pc.wrapping_add(4), supervisor),
                                31
                            )]
                        );
                        assert_eq!(cpu.clock().0, 35 + u64::from(wait));
                        assert_eq!(cpu.clock().0, bus.clock);
                        assert_eq!(
                            cpu.physical_inspection(),
                            PhysicalInspection::DiagnosticCompleteValues
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn bra_byte_and_word_completions_use_signed_targets_and_both_sampled_words() {
    for supervisor in [false, true] {
        for (opcode, extension, displacement) in [
            (0x6006, 0x4e71, 6),
            (0x60f8, 0x4e71, -8),
            (0x6000, 0x0020, 32),
            (0x6000, 0xffe0, -32),
        ] {
            for pc in [0x1234_2000_u32, 0xffff_fffe] {
                let target = pc.wrapping_add(2).wrapping_add_signed(displacement);
                let initial = registers(if supervisor { SUPERVISOR } else { 0 } | 0x1b, pc);
                let mut cpu =
                    PrefetchCpu::from_prefetched(initial, [opcode, extension], CpuClocks(51))
                        .unwrap();
                let mut bus = Device::new(51);
                bus.instruction_elapsed = [7, 13];
                bus.word(target, 0x1234);
                bus.word(target.wrapping_add(2), 0xabcd);
                let result = cpu.step_physical_instruction(&mut bus).unwrap();
                let mut expected = initial;
                expected.pc = target;
                assert_eq!(cpu.registers, expected);
                assert_eq!(cpu.queued_words(), Some([0x1234, 0xabcd]));
                assert_eq!(
                    result,
                    PhysicalInstructionStep::Instruction(zeff_m68k::timing::TimedStep {
                        opcode,
                        clocks: 22,
                        exception: None
                    })
                );
                assert_eq!(
                    bus.events,
                    vec![
                        Event::Idle(51, 2),
                        Event::Instruction(access(target, supervisor), 53),
                        Event::Instruction(access(target.wrapping_add(2), supervisor), 60)
                    ]
                );
                assert_eq!(cpu.clock(), CpuClocks(73));
                assert_eq!(bus.clock, 73);
            }
        }
    }
}
