use super::*;

#[path = "retirement_evidence_tests/indexed.rs"]
mod indexed;
#[path = "retirement_evidence_tests/internal.rs"]
mod internal;

struct ByteBus(u8);
impl Z80Bus for ByteBus {
    fn cpu_read(&self, _: u16) -> u8 {
        self.0
    }
    fn cpu_write(&mut self, _: u16, _: u8) {
        panic!("unexpected write")
    }
    fn io_read(&mut self, _: u8) -> u8 {
        panic!("unexpected IO")
    }
    fn io_write(&mut self, _: u8, _: u8) {
        panic!("unexpected IO")
    }
    fn maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        panic!("unexpected NMI")
    }
}

fn seed(cpu: &mut Cpu, state: &mut BusState, bytes: &[u8]) {
    for &byte in bytes {
        assert!(
            !cpu.step_bus_segment(&mut ByteBus(byte), state)
                .unwrap()
                .complete
        );
    }
}

fn input(address: u16, value: u8, class: InstructionByteClass) -> Option<ConsumedInstructionByte> {
    Some(ConsumedInstructionByte {
        address,
        value,
        class,
    })
}

fn sample(kind: CycleKind, value: u8) -> CycleSample {
    CycleSample {
        address: kind.address(),
        m1: kind.is_m1(),
        write: kind.is_write(),
        value,
        known_mask: 255,
    }
}

fn physical(cpu: &mut Cpu, state: &mut BusState, value: u8, waits: u32) -> RetirementEvidence {
    let before = cpu.cycles;
    let mut ordinary = (cpu.clone(), *state);
    let request = cpu.next_physical_cycle(state).unwrap();
    let receipt = sample(request.kind(), value);
    let completion = request
        .complete(receipt, waits, CycleInterrupts::default())
        .unwrap();
    let expected = ordinary
        .0
        .next_physical_cycle(&ordinary.1)
        .unwrap()
        .complete(receipt, waits, CycleInterrupts::default())
        .unwrap();
    let (step, evidence) = cpu
        .retire_physical_cycle_with_evidence(state, completion)
        .unwrap();
    let expected = ordinary
        .0
        .retire_physical_cycle(&mut ordinary.1, expected)
        .unwrap();
    assert_eq!(step, expected);
    assert_eq!((cpu.clone(), *state), ordinary);
    assert_eq!(
        (evidence.complete, evidence.cpu_before, evidence.cpu_after),
        (step.complete, before, cpu.cycles)
    );
    evidence
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_consumes_frozen_samples_and_excludes_halt_polls() {
    let mut cpu = Cpu::new();
    let mut state = BusState::default();
    let mut bytes = Vec::new();
    bytes.push(physical(&mut cpu, &mut state, 0x3e, 0).consumed.unwrap());
    let request = cpu.next_physical_cycle(&state).unwrap();
    let mut ram = 0x5a;
    assert_eq!(ram, 0x5a);
    ram = 0xa6;
    let completion = request
        .complete(
            sample(CycleKind::MemoryRead { address: 1 }, ram),
            2,
            CycleInterrupts::default(),
        )
        .unwrap();
    ram = 0x91;
    let (step, evidence) = cpu
        .retire_physical_cycle_with_evidence(&mut state, completion)
        .unwrap();
    assert!(step.complete);
    assert_eq!(ram, 0x91);
    assert_eq!(
        evidence.consumed,
        input(1, 0xa6, InstructionByteClass::Operand)
    );
    assert_eq!(cpu.regs.a, 0xa6);
    bytes.push(evidence.consumed.unwrap());
    for value in [0x32, 0x00, 0x10, 0xa6, 0x00, 0x76] {
        if let Some(byte) = physical(&mut cpu, &mut state, value, 0).consumed {
            bytes.push(byte);
        }
    }
    assert_eq!(
        bytes
            .iter()
            .map(|b| (b.address, b.value))
            .collect::<Vec<_>>(),
        [
            (0, 0x3e),
            (1, 0xa6),
            (2, 0x32),
            (3, 0),
            (4, 0x10),
            (5, 0),
            (6, 0x76)
        ]
    );
    let poll = physical(&mut cpu, &mut state, 0xff, 2);
    assert_eq!(poll.kind, RetirementKind::HaltPoll);
    assert_eq!(poll.consumed, None);
    assert!(poll.complete);
    assert_eq!(poll.cpu_after - poll.cpu_before, 6);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_uses_real_prefix_load_operand_and_stack_consumption() {
    let mut cpu = Cpu::new();
    let mut state = BusState::default();
    let mut observed = Vec::new();
    for value in [
        0xdd, 0x21, 0x34, 0x12, 0xed, 0x44, 0xcb, 0x00, 0xc6, 0x5a, 0xcd, 0x34, 0x12, 0, 13,
    ] {
        if let Some(byte) = physical(&mut cpu, &mut state, value, 0).consumed {
            observed.push(byte);
        }
    }
    assert_eq!(
        observed.iter().map(|b| b.value).collect::<Vec<_>>(),
        [
            0xdd, 0x21, 0x34, 0x12, 0xed, 0x44, 0xcb, 0x00, 0xc6, 0x5a, 0xcd, 0x34, 0x12
        ]
    );
    assert_eq!(
        observed.iter().map(|b| b.address).collect::<Vec<_>>(),
        (0..13).collect::<Vec<u16>>()
    );
    assert_eq!(observed[1].class, InstructionByteClass::M1);
    assert_eq!(observed[2].class, InstructionByteClass::Operand);
    assert_eq!(observed[11].class, InstructionByteClass::Operand);
    assert_eq!(cpu.regs.pc, 0x1234);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_failure_is_pure_and_prefix_refusal_stays_explicit() {
    let mut cpu = Cpu::new();
    let mut state = BusState::default();
    let completion = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .complete(
            sample(
                CycleKind::M1 {
                    address: 0,
                    halted: false,
                    refresh_address: 0,
                },
                0x3e,
            ),
            0,
            CycleInterrupts::default(),
        )
        .unwrap();
    cpu.regs.a = 1;
    let saved = (cpu.clone(), state);
    assert_eq!(
        cpu.retire_physical_cycle_with_evidence(&mut state, completion),
        Err(CycleError::StaleContinuation)
    );
    assert_eq!((cpu.clone(), state), saved);
    for _ in 0..6 {
        assert!(
            !cpu.step_bus_segment(&mut ByteBus(0xdd), &mut state)
                .unwrap()
                .complete
        );
    }
    assert_eq!(cpu.instruction_bytes(), &[0xdd; 4]);
    let saved = (cpu.clone(), state);
    assert!(cpu.next_physical_cycle(&state).is_err());
    assert_eq!((cpu, state), saved);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_retains_zero_clock_word_add_and_djnz_inputs_once() {
    for waits in [0, 2] {
        let mut cpu = Cpu::new();
        let mut state = BusState::default();
        let mut tail = None;
        for phase in 0..3 {
            let mut ordinary = (cpu.clone(), state);
            let receipt = if phase == 0 {
                WordAddReceipt::M1 {
                    address: 0,
                    opcode: 0x09,
                    known_mask: 255,
                }
            } else {
                WordAddReceipt::Internal
            };
            let delay = if phase == 0 { waits } else { 0 };
            let completion = cpu
                .next_word_add_cycle(&state, tail.as_ref())
                .unwrap()
                .complete(receipt, delay, CycleInterrupts::default())
                .unwrap();
            let expected = ordinary
                .0
                .next_word_add_cycle(&ordinary.1, tail.as_ref())
                .unwrap()
                .complete(receipt, delay, CycleInterrupts::default())
                .unwrap();
            let (retirement, evidence) = cpu
                .retire_word_add_cycle_with_evidence(&mut state, completion)
                .unwrap();
            let expected = ordinary
                .0
                .retire_word_add_cycle(&mut ordinary.1, expected)
                .unwrap();
            assert_eq!(retirement.step, expected.step);
            assert_eq!(retirement.tail, expected.tail);
            assert_eq!((cpu.clone(), state), ordinary);
            assert_eq!(evidence.complete, phase == 2);
            assert_eq!(
                evidence.consumed,
                if phase == 0 {
                    input(0, 0x09, InstructionByteClass::M1)
                } else {
                    None
                }
            );
            assert_eq!(
                evidence.cpu_after - evidence.cpu_before,
                if phase == 2 { 11 + u64::from(waits) } else { 0 }
            );
            tail = retirement.tail;
        }
        for initial_b in [1, 2] {
            let mut cpu = Cpu::new();
            cpu.regs.b = initial_b;
            let mut state = BusState::default();
            let mut tail = None;
            for phase in 0..if initial_b == 1 { 2 } else { 3 } {
                let receipt = match phase {
                    0 => DjnzReceipt::M1 {
                        address: 0,
                        opcode: 0x10,
                        known_mask: 255,
                    },
                    1 => DjnzReceipt::Read {
                        address: 1,
                        value: 0x27,
                        known_mask: 255,
                    },
                    _ => DjnzReceipt::Internal,
                };
                let delay = if phase < 2 { waits } else { 0 };
                let mut ordinary = (cpu.clone(), state);
                let completion = cpu
                    .next_djnz_cycle(&state, tail.as_ref())
                    .unwrap()
                    .complete(receipt, delay, CycleInterrupts::default())
                    .unwrap();
                let expected = ordinary
                    .0
                    .next_djnz_cycle(&ordinary.1, tail.as_ref())
                    .unwrap()
                    .complete(receipt, delay, CycleInterrupts::default())
                    .unwrap();
                let (retirement, evidence) = cpu
                    .retire_djnz_cycle_with_evidence(&mut state, completion)
                    .unwrap();
                let expected = ordinary
                    .0
                    .retire_djnz_cycle(&mut ordinary.1, expected)
                    .unwrap();
                assert_eq!(retirement.step, expected.step);
                assert_eq!(retirement.tail, expected.tail);
                assert_eq!((cpu.clone(), state), ordinary);
                assert_eq!(
                    evidence.consumed,
                    match phase {
                        0 => input(0, 0x10, InstructionByteClass::M1),
                        1 => input(1, 0x27, InstructionByteClass::Displacement),
                        _ => None,
                    }
                );
                assert_eq!(evidence.complete, retirement.step.complete);
                if !evidence.complete {
                    assert_eq!(evidence.cpu_after, evidence.cpu_before);
                }
                tail = retirement.tail;
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_evidence_jr_retains_displacement_before_deferred_accounting() {
    for opcode in [0x18, 0x20, 0x28] {
        let mut cpu = Cpu::new();
        let mut state = BusState::default();
        seed(&mut cpu, &mut state, &[opcode]);
        let before = cpu.cycles;
        let receipt = JrReceipt::Read {
            address: 1,
            value: 0x20,
            known_mask: 255,
        };
        let completion = cpu
            .next_jr_cycle(&state, None)
            .unwrap()
            .complete(receipt, 2)
            .unwrap();
        let (retirement, evidence) = cpu
            .retire_jr_cycle_with_evidence(&mut state, completion)
            .unwrap();
        assert_eq!(
            evidence.consumed,
            input(1, 0x20, InstructionByteClass::Displacement)
        );
        assert_eq!(evidence.complete, opcode == 0x28);
        if let Some(tail) = retirement.tail {
            assert_eq!(cpu.cycles, before);
            assert_eq!(evidence.cpu_after, evidence.cpu_before);
            let completion = cpu
                .next_jr_cycle(&state, Some(&tail))
                .unwrap()
                .complete(JrReceipt::Internal, 0)
                .unwrap();
            let (retirement, evidence) = cpu
                .retire_jr_cycle_with_evidence(&mut state, completion)
                .unwrap();
            assert!(retirement.step.complete && evidence.complete);
            assert_eq!(evidence.consumed, None);
            assert_eq!(evidence.cpu_after - evidence.cpu_before, 10);
        } else {
            assert_eq!(evidence.cpu_after - evidence.cpu_before, 5);
        }
    }
}
