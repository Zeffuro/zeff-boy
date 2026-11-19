use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_ed_word_and_copy_internal_phases_do_not_consume_bytes() {
    for opcode in [0x42, 0x4a] {
        let mut cpu = Cpu::new();
        let mut state = BusState::default();
        seed(&mut cpu, &mut state, &[0xed, opcode]);
        for terminal in [false, true] {
            let mut ordinary = (cpu.clone(), state);
            let completion = cpu.next_ed_word_cycle(&state).unwrap().complete(0).unwrap();
            let expected = ordinary
                .0
                .next_ed_word_cycle(&ordinary.1)
                .unwrap()
                .complete(0)
                .unwrap();
            let (step, evidence) = cpu
                .retire_ed_word_cycle_with_evidence(&mut state, completion)
                .unwrap();
            assert_eq!(
                step,
                ordinary
                    .0
                    .retire_ed_word_cycle(&mut ordinary.1, expected)
                    .unwrap()
            );
            assert_eq!((cpu.clone(), state), ordinary);
            assert_eq!(evidence.consumed, None);
            assert_eq!(evidence.complete, terminal);
            assert_eq!(
                evidence.cpu_after - evidence.cpu_before,
                if terminal { 3 } else { 4 }
            );
        }
    }
    for opcode in [0xa0, 0xb0] {
        let mut cpu = Cpu::new();
        cpu.regs.set_bc(2);
        cpu.regs.set_hl(0x1000);
        cpu.regs.set_de(0x2000);
        let mut state = BusState::default();
        seed(&mut cpu, &mut state, &[0xed, opcode]);
        for phase in 0..if opcode == 0xb0 { 3 } else { 2 } {
            let request = cpu.next_copy_cycle(&state).unwrap();
            let receipt = match request.kind() {
                CopyCycleKind::Read { .. } => CopyReceipt::Read {
                    value: 0x5a,
                    known: 255,
                },
                CopyCycleKind::Write { .. } => CopyReceipt::Write,
                CopyCycleKind::Repeat { .. } => CopyReceipt::Repeat,
            };
            let mut ordinary = (cpu.clone(), state);
            let expected = ordinary
                .0
                .next_copy_cycle(&ordinary.1)
                .unwrap()
                .complete(receipt, 0)
                .unwrap();
            let completion = request.complete(receipt, 0).unwrap();
            let (step, evidence) = cpu
                .retire_copy_cycle_with_evidence(&mut state, completion)
                .unwrap();
            assert_eq!(
                step,
                ordinary
                    .0
                    .retire_copy_cycle(&mut ordinary.1, expected)
                    .unwrap()
            );
            assert_eq!((cpu.clone(), state), ordinary);
            assert_eq!(evidence.consumed, None);
            assert_eq!(
                evidence.complete,
                phase == if opcode == 0xb0 { 2 } else { 1 }
            );
        }
        assert!(!state.pending());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_im1_terminal_comes_from_actual_completion() {
    for halted in [false, true] {
        for waits in [0, 2] {
            let mut cpu = Cpu::new();
            cpu.regs.pc = 0xfffe;
            cpu.regs.sp = 1;
            cpu.interrupt_mode = InterruptMode::Im1;
            cpu.interrupt_flip_flop_1 = true;
            cpu.interrupt_flip_flop_2 = true;
            if halted {
                cpu.state = CpuState::Halted;
            }
            let mut state = BusState::default();
            for phase in 0..3 {
                let request = cpu.next_im1_cycle(&state, phase == 0).unwrap();
                let receipt = match request.kind() {
                    Im1CycleKind::Acknowledge { .. } => Im1Receipt::Acknowledged,
                    Im1CycleKind::StackWrite { address, value, .. } => {
                        Im1Receipt::Written { address, value }
                    }
                };
                let mut ordinary = (cpu.clone(), state);
                let expected = ordinary
                    .0
                    .next_im1_cycle(&ordinary.1, phase == 0)
                    .unwrap()
                    .complete(receipt, waits)
                    .unwrap();
                let mut stale = cpu.clone();
                stale.regs.a ^= 1;
                let mut stale_state = state;
                let saved = (stale.clone(), stale_state);
                let stale_completion = cpu
                    .next_im1_cycle(&state, phase == 0)
                    .unwrap()
                    .complete(receipt, waits)
                    .unwrap();
                assert!(matches!(
                    stale.retire_im1_cycle_with_evidence(&mut stale_state, stale_completion),
                    Err(CycleError::StaleContinuation)
                ));
                assert_eq!((stale, stale_state), saved);
                let completion = request.complete(receipt, waits).unwrap();
                let (step, evidence) = cpu
                    .retire_im1_cycle_with_evidence(&mut state, completion)
                    .unwrap();
                assert_eq!(
                    step,
                    ordinary
                        .0
                        .retire_im1_cycle(&mut ordinary.1, expected)
                        .unwrap()
                );
                assert_eq!((cpu.clone(), state), ordinary);
                assert_eq!(evidence.kind, RetirementKind::Interrupt);
                assert_eq!(evidence.consumed, None);
                assert_eq!(evidence.complete, phase == 2);
                assert_eq!(evidence.complete, step.complete);
                assert_eq!(
                    evidence.cpu_after - evidence.cpu_before,
                    if phase == 0 { 7 } else { 3 } + u64::from(waits)
                );
                let mut enabled = crate::state::Writer::new();
                cpu.write_continuation(&state, &mut enabled);
                let mut disabled = crate::state::Writer::new();
                ordinary.0.write_continuation(&ordinary.1, &mut disabled);
                assert_eq!(enabled.into_bytes(), disabled.into_bytes());
            }
            assert_eq!(cpu.regs.pc, Z80_INTERRUPT_VECTOR_IM1);
        }
    }
}
