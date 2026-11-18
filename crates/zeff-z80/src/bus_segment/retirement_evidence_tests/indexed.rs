use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_indexed_classification_comes_from_frozen_instruction_reads() {
    for prefix in [0xdd, 0xfd] {
        for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
            for opcode in [0x36, 0x7e, 0x34] {
                for waits in [0, 2] {
                    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0, 0x8000), profile);
                    cpu.regs.ix = 0x1000;
                    cpu.regs.iy = 0x1000;
                    let mut state = BusState::default();
                    seed(&mut cpu, &mut state, &[prefix, opcode]);
                    let mut bytes = Vec::new();
                    for _ in 0..5 {
                        let request = cpu.next_indexed_memory_cycle(&state).unwrap();
                        let kind = request.kind();
                        let receipt = match kind {
                            IndexedMemoryKind::Displacement { address, .. } => {
                                IndexedMemoryReceipt::Read {
                                    address,
                                    value: 1,
                                    known_mask: 255,
                                }
                            }
                            IndexedMemoryKind::Immediate { address } => {
                                IndexedMemoryReceipt::Read {
                                    address,
                                    value: 0xa6,
                                    known_mask: 255,
                                }
                            }
                            IndexedMemoryKind::Read { address }
                            | IndexedMemoryKind::ReadModify { address } => {
                                IndexedMemoryReceipt::Read {
                                    address,
                                    value: 0x81,
                                    known_mask: 255,
                                }
                            }
                            IndexedMemoryKind::Write { address, value } => {
                                IndexedMemoryReceipt::Write { address, value }
                            }
                            IndexedMemoryKind::Address { .. } => IndexedMemoryReceipt::Internal,
                        };
                        let delay = if matches!(kind, IndexedMemoryKind::Address { .. }) {
                            0
                        } else {
                            waits
                        };
                        let mut ordinary = (cpu.clone(), state);
                        let expected = ordinary
                            .0
                            .next_indexed_memory_cycle(&ordinary.1)
                            .unwrap()
                            .complete(receipt, delay)
                            .unwrap();
                        let mut stale = cpu.clone();
                        stale.regs.a ^= 1;
                        let mut stale_state = state;
                        let saved = (stale.clone(), stale_state);
                        let stale_completion = cpu
                            .next_indexed_memory_cycle(&state)
                            .unwrap()
                            .complete(receipt, delay)
                            .unwrap();
                        assert!(matches!(
                            stale.retire_indexed_memory_cycle_with_evidence(
                                &mut stale_state,
                                stale_completion
                            ),
                            Err(CycleError::StaleContinuation)
                        ));
                        assert_eq!((stale, stale_state), saved);
                        let completion = request.complete(receipt, delay).unwrap();
                        let (step, evidence) = cpu
                            .retire_indexed_memory_cycle_with_evidence(&mut state, completion)
                            .unwrap();
                        assert_eq!(
                            step,
                            ordinary
                                .0
                                .retire_indexed_memory_cycle(&mut ordinary.1, expected)
                                .unwrap()
                        );
                        assert_eq!((cpu.clone(), state), ordinary);
                        let consumed = match kind {
                            IndexedMemoryKind::Displacement { .. } => {
                                input(2, 1, InstructionByteClass::Displacement)
                            }
                            IndexedMemoryKind::Immediate { .. } => {
                                input(3, 0xa6, InstructionByteClass::Operand)
                            }
                            _ => None,
                        };
                        assert_eq!(evidence.consumed, consumed);
                        assert_eq!(evidence.complete, step.complete);
                        if let Some(byte) = evidence.consumed {
                            bytes.push(byte.value);
                        }
                        if step.complete {
                            break;
                        }
                    }
                    assert!(!state.pending());
                    assert_eq!(
                        bytes,
                        if opcode == 0x36 {
                            vec![1, 0xa6]
                        } else {
                            vec![1]
                        }
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_indexed_cb_displacement_and_terminal_exclude_rmw_data() {
    for prefix in [0xdd, 0xfd] {
        for opcode in [0x46, 0x06, 0x86, 0xc6] {
            let mut cpu = Cpu::new();
            cpu.regs.ix = 0x1000;
            cpu.regs.iy = 0x1000;
            let mut state = BusState::default();
            seed(&mut cpu, &mut state, &[prefix, 0xcb]);
            let mut bytes = Vec::new();
            for _ in 0..4 {
                let request = cpu.next_indexed_cb_cycle(&state).unwrap();
                let kind = request.kind();
                let receipt = match kind {
                    IndexedCbCycleKind::Displacement { address, .. } => {
                        IndexedMemoryReceipt::Read {
                            address,
                            value: 0xff,
                            known_mask: 255,
                        }
                    }
                    IndexedCbCycleKind::Terminal { address } => IndexedMemoryReceipt::Read {
                        address,
                        value: opcode,
                        known_mask: 255,
                    },
                    IndexedCbCycleKind::Read { address }
                    | IndexedCbCycleKind::ReadModify { address } => IndexedMemoryReceipt::Read {
                        address,
                        value: 0x81,
                        known_mask: 255,
                    },
                    IndexedCbCycleKind::Write { address, value } => {
                        IndexedMemoryReceipt::Write { address, value }
                    }
                };
                let mut ordinary = (cpu.clone(), state);
                let expected = ordinary
                    .0
                    .next_indexed_cb_cycle(&ordinary.1)
                    .unwrap()
                    .complete(receipt, 2)
                    .unwrap();
                let completion = request.complete(receipt, 2).unwrap();
                let (step, evidence) = cpu
                    .retire_indexed_cb_cycle_with_evidence(&mut state, completion)
                    .unwrap();
                assert_eq!(
                    step,
                    ordinary
                        .0
                        .retire_indexed_cb_cycle(&mut ordinary.1, expected)
                        .unwrap()
                );
                assert_eq!((cpu.clone(), state), ordinary);
                let expected = match kind {
                    IndexedCbCycleKind::Displacement { .. } => {
                        input(2, 0xff, InstructionByteClass::Displacement)
                    }
                    IndexedCbCycleKind::Terminal { .. } => {
                        input(3, opcode, InstructionByteClass::Terminal)
                    }
                    _ => None,
                };
                assert_eq!(evidence.consumed, expected);
                assert_eq!(evidence.complete, step.complete);
                if let Some(byte) = evidence.consumed {
                    bytes.push(byte.value);
                }
                if step.complete {
                    break;
                }
            }
            assert!(!state.pending());
            assert_eq!(bytes, [0xff, opcode]);
        }
    }
}
