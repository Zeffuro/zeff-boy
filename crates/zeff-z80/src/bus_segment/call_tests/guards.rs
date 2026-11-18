use super::*;

fn rejected(cpu: &Cpu, state: &BusState) {
    assert!(matches!(
        cpu.next_physical_cycle(state),
        Err(CycleError::UnsupportedCycle)
    ));
}

pub(super) fn sample_checks(
    cpu: &Cpu,
    state: &BusState,
    receipt: CycleSample,
    inputs: CycleInterrupts,
    waits: u32,
) {
    let request = cpu.next_physical_cycle(state).unwrap();
    for (bad, error) in [
        (
            CycleSample {
                known_mask: 0xfe,
                ..receipt
            },
            CycleError::UnknownData,
        ),
        (
            CycleSample {
                address: receipt.address ^ 1,
                ..receipt
            },
            CycleError::SampleMismatch,
        ),
        (
            CycleSample {
                m1: !receipt.m1,
                ..receipt
            },
            CycleError::SampleMismatch,
        ),
        (
            CycleSample {
                write: !receipt.write,
                ..receipt
            },
            CycleError::SampleMismatch,
        ),
    ] {
        assert_eq!(request.sampled_t_states(bad), Err(error));
        assert!(
            matches!(cpu.next_physical_cycle(state).unwrap().complete(bad, waits, inputs),
            Err(actual) if actual == error)
        );
    }
    if receipt.write {
        let bad = CycleSample {
            value: receipt.value ^ 1,
            ..receipt
        };
        assert_eq!(
            request.sampled_t_states(bad),
            Err(CycleError::SampleMismatch)
        );
        assert!(matches!(
            cpu.next_physical_cycle(state)
                .unwrap()
                .complete(bad, waits, inputs),
            Err(CycleError::SampleMismatch)
        ));
    }
}

pub(super) fn overflow(cpu: &Cpu, state: BusState, receipt: CycleSample) {
    let base = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .sampled_t_states(receipt)
        .unwrap();
    for (cycles, waits) in [(u64::MAX - u64::from(base) + 1, 0), (cpu.cycles, u32::MAX)] {
        let mut candidate = cpu.clone();
        candidate.cycles = cycles;
        let before = candidate.clone();
        let mut bus = state;
        let completion = candidate
            .next_physical_cycle(&bus)
            .unwrap()
            .complete(receipt, waits, CycleInterrupts::default())
            .unwrap();
        assert_eq!(
            candidate.retire_physical_cycle(&mut bus, completion),
            Err(CycleError::ClockOverflow)
        );
        assert_eq!(candidate, before);
        assert_eq!(bus, state);
    }
}

fn replace(state: BusState, bytes: &[u8]) -> BusState {
    let operation = stack::Stack::read_continuation(&mut crate::state::Reader::new(bytes)).unwrap();
    let mut result = state;
    result.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
    result
}

pub(super) fn pending(cpu: &Cpu, state: BusState, prefix: Option<u8>, opcode: u8) {
    let Some(Transfer {
        operation: Operation::Stack(operation),
        ..
    }) = state.transfer
    else {
        return;
    };
    for variant in 0..8 {
        let mut candidate = cpu.clone();
        let mut bus = state;
        match variant {
            0 => bus.transfer.as_mut().unwrap().pc ^= 1,
            1 => candidate.last_opcode_pc ^= 1,
            2 => candidate.last_opcode = 0,
            3 => candidate.instruction_bytes[0] ^= 1,
            4 => candidate.last_m1_fetch_count += 1,
            5 => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
            6 => candidate.regs.sp ^= 1,
            _ => {
                candidate.instruction_byte_count += 1;
                candidate.regs.pc = candidate
                    .last_opcode_pc
                    .wrapping_add(u16::from(candidate.instruction_byte_count));
            }
        }
        rejected(&candidate, &bus);
    }
    if let Some(prefix) = prefix {
        let mut bus = state;
        bus.prefix = PrefixState::index(cpu.last_opcode_pc, prefix ^ 0x20);
        rejected(cpu, &bus);
        bus.prefix = PrefixState::default();
        rejected(cpu, &bus);
        let mut candidate = cpu.clone();
        candidate.instruction_bytes[1] = 0xd3;
        rejected(&candidate, &state);
    }
    let mut writer = crate::state::Writer::new();
    operation.write_continuation(&mut writer);
    let bytes = writer.into_bytes();
    let address = if bytes[0] <= 1 { 2 } else { 1 };
    let target = address + 2;
    let fetch = target + 1 + if bytes[target] == 0 { 0 } else { 2 };
    let phase = fetch + 1;
    for variant in 0..5 {
        let mut bad = bytes.clone();
        match variant {
            0 => bad[address] ^= 1,
            1 => bad[1] ^= 1,
            2 => bad[fetch] = 1,
            3 => {
                if bad[target] == 0 {
                    bad[target] = 1;
                    bad.insert(target + 1, 0x34);
                    bad.insert(target + 2, 0x12);
                } else {
                    bad[target + 1] ^= 1;
                }
            }
            _ => {
                bad.truncate(phase);
                bad.extend([5, 0x34, 0x12]);
            }
        }
        rejected(cpu, &replace(state, &bad));
    }
    let mut bad = bytes.clone();
    match bytes[phase] {
        2 | 3 | 6 | 7 | 10..=13 => bad[phase + 1] ^= 1,
        _ => panic!("unexpected call or exchange phase"),
    }
    rejected(cpu, &replace(state, &bad));
    if bytes[phase] == 3 {
        let mut bad = bytes.clone();
        bad[phase + 2] ^= 1;
        rejected(cpu, &replace(state, &bad));
    }
    if matches!(bytes[phase], 6 | 10..=12) {
        let mut bad = bytes.clone();
        bad[phase + 2] ^= 1;
        rejected(cpu, &replace(state, &bad));
    }
    if opcode != 0xcd && opcode != 0xe3 {
        let mut candidate = cpu.clone();
        candidate.regs.f ^= match (opcode >> 4) & 3 {
            0 => Z80_FLAG_ZERO,
            1 => Z80_FLAG_CARRY,
            2 => Z80_FLAG_PARITY_OVERFLOW,
            _ => Z80_FLAG_SIGN,
        };
        rejected(&candidate, &state);
    }
    if opcode == 0xe3 {
        let mut candidate = cpu.clone();
        match prefix {
            Some(Z80_PREFIX_DD) => candidate.regs.ix ^= 1,
            Some(Z80_PREFIX_FD) => candidate.regs.iy ^= 1,
            _ => candidate.regs.l ^= 1,
        }
        rejected(&candidate, &state);
        if bytes[phase] >= 11 {
            let mut opaque = bytes;
            let result = phase + if opaque[phase] == 13 { 2 } else { 3 };
            opaque[result] ^= 0x80;
            let bus = replace(state, &opaque);
            assert_eq!(
                cpu.next_physical_cycle(&bus).unwrap().kind(),
                cpu.next_physical_cycle(&state).unwrap().kind()
            );
        }
    }
    let operand = 1 + usize::from(prefix.is_some());
    if stack::Stack::is_call(opcode) && usize::from(cpu.instruction_byte_count) > operand {
        let mut candidate = cpu.clone();
        candidate.instruction_bytes[operand] ^= 1;
        rejected(&candidate, &state);
        if usize::from(cpu.instruction_byte_count) == operand + 2 {
            let mut candidate = cpu.clone();
            candidate.instruction_bytes[operand + 1] ^= 1;
            rejected(&candidate, &state);
        }
    }
}

pub(super) fn excluded() {
    for prefix in [None, Some(Z80_PREFIX_DD), Some(Z80_PREFIX_FD)] {
        let (cpu, state) = if let Some(prefix) = prefix {
            super::super::prefix_tests::start(prefix)
        } else {
            (Cpu::new(), BusState::default())
        };
        let request = cpu.next_physical_cycle(&state).unwrap();
        for opcode in [0x09, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0xd3, 0xfb] {
            assert!(
                matches!(request.sampled_t_states(sample(request.kind(), opcode)),
                Err(CycleError::UnsupportedOpcode { opcode: actual, .. }) if actual == opcode)
            );
        }
        for opcode in [0xcd, 0xc4, 0xe3] {
            assert_eq!(
                request.sampled_t_states(sample(request.kind(), opcode)),
                Ok(4)
            );
        }
    }
    let (cpu, state) = super::super::prefix_tests::start(Z80_PREFIX_ED);
    let request = cpu.next_physical_cycle(&state).unwrap();
    for opcode in [0xcd, 0xc4, 0xe3] {
        assert!(
            matches!(request.sampled_t_states(sample(request.kind(), opcode)),
            Err(CycleError::UnsupportedOpcode { opcode: actual, .. }) if actual == opcode)
        );
    }
    let (cpu, state) = super::super::prefix_tests::start(Z80_PREFIX_CB);
    let request = cpu.next_physical_cycle(&state).unwrap();
    for opcode in [0xcd, 0xc4, 0xe3] {
        assert_eq!(
            request.sampled_t_states(sample(request.kind(), opcode)),
            Ok(4)
        );
    }
}
