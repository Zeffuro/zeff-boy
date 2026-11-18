use super::*;

fn rejected(cpu: &Cpu, state: &BusState) {
    assert!(matches!(
        cpu.next_physical_cycle(state),
        Err(CycleError::UnsupportedCycle)
    ));
}

pub(super) fn pending(cpu: &Cpu, state: BusState, prefix: Option<u8>, opcode: u8) {
    let Some(Transfer {
        operation: Operation::Stack(operation),
        ..
    }) = state.transfer
    else {
        return;
    };
    for variant in 0..5 {
        let mut candidate = cpu.clone();
        let mut bus = state;
        match variant {
            0 => bus.transfer.as_mut().unwrap().pc ^= 1,
            1 => candidate.last_opcode_pc ^= 1,
            2 => candidate.instruction_bytes[0] ^= 1,
            3 => candidate.regs.sp ^= 1,
            _ => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
        }
        rejected(&candidate, &bus);
    }
    if let Some(prefix) = prefix {
        let mut candidate = cpu.clone();
        candidate.instruction_byte_count = 3;
        candidate.regs.pc = candidate.last_opcode_pc.wrapping_add(3);
        rejected(&candidate, &state);
        let mut bus = state;
        bus.prefix = PrefixState::index(cpu.last_opcode_pc, prefix ^ 0x20);
        rejected(cpu, &bus);
        bus.prefix = PrefixState::default();
        rejected(cpu, &bus);
    }
    let mut writer = crate::state::Writer::new();
    operation.write_continuation(&mut writer);
    let bytes = writer.into_bytes();
    let address = if bytes[0] <= 1 { 2 } else { 1 };
    let target = address + 2;
    let fetch = target + 1 + if bytes[target] == 0 { 0 } else { 2 };
    for variant in 0..5 {
        let mut bad = bytes.clone();
        match variant {
            0 => bad[address] ^= 1,
            1 => {
                if bad[0] <= 1 {
                    bad[1] ^= 1;
                } else {
                    bad[0] ^= 1;
                }
            }
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
                bad.truncate(fetch + 1);
                bad.extend([4, 1]);
            }
        }
        let operation =
            stack::Stack::read_continuation(&mut crate::state::Reader::new(&bad)).unwrap();
        let mut bus = state;
        bus.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
        rejected(cpu, &bus);
    }
    if opcode & 0xc7 == 0xc0 {
        let mut candidate = cpu.clone();
        candidate.regs.f ^= match (opcode >> 4) & 3 {
            0 => Z80_FLAG_ZERO,
            1 => Z80_FLAG_CARRY,
            2 => Z80_FLAG_PARITY_OVERFLOW,
            _ => Z80_FLAG_SIGN,
        };
        rejected(&candidate, &state);
    } else {
        let mut bad = bytes;
        let phase = fetch + 1;
        assert!(matches!(bad[phase], 6 | 7));
        bad[phase + 1] ^= 1;
        let operation =
            stack::Stack::read_continuation(&mut crate::state::Reader::new(&bad)).unwrap();
        let mut bus = state;
        bus.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
        rejected(cpu, &bus);
    }
}

pub(super) fn selector() {
    let cpu = Cpu::new();
    let state = BusState::default();
    let request = cpu.next_physical_cycle(&state).unwrap();
    assert_eq!(request.kind().t_states(), None);
    assert_eq!(request.kind().minimum_t_states(), 4);
    for (opcode, cycles) in [
        (0x00, 4),
        (0xc5, 5),
        (0x03, 6),
        (Z80_PREFIX_DD, 4),
        (Z80_PREFIX_ED, 4),
    ] {
        assert_eq!(
            request.sampled_t_states(sample(request.kind(), opcode)),
            Ok(cycles)
        );
    }
    for prefix in [Z80_PREFIX_CB, Z80_PREFIX_ED, Z80_PREFIX_DD, Z80_PREFIX_FD] {
        let (cpu, state) = super::super::prefix_tests::start(prefix);
        let request = cpu.next_physical_cycle(&state).unwrap();
        let kind = request.kind();
        if prefix == Z80_PREFIX_CB {
            assert_eq!(request.sampled_t_states(sample(kind, 0xc5)), Ok(4));
            assert_eq!(request.sampled_t_states(sample(kind, 0x03)), Ok(4));
        } else if prefix == Z80_PREFIX_ED {
            assert_eq!(
                request.sampled_t_states(sample(kind, 0xc5)),
                Err(CycleError::UnsupportedOpcode {
                    pc: 1,
                    opcode: 0xc5
                })
            );
        } else {
            assert_eq!(request.sampled_t_states(sample(kind, 0xc5)), Ok(5));
            assert_eq!(request.sampled_t_states(sample(kind, 0x03)), Ok(6));
        }
    }
    let mut halted = Cpu::new();
    halted.enter_halt();
    let request = halted.next_physical_cycle(&BusState::default()).unwrap();
    for opcode in [0x00, 0xc5, 0x03] {
        assert_eq!(
            request.sampled_t_states(sample(request.kind(), opcode)),
            Ok(4)
        );
    }
    for opcode in [0xc5, 0x03] {
        for (cycles, waits) in [(u64::MAX - 4, 0), (0, u32::MAX)] {
            let mut cpu = Cpu::new();
            cpu.cycles = cycles;
            let mut state = BusState::default();
            let before = cpu.clone();
            let request = cpu.next_physical_cycle(&state).unwrap();
            let kind = request.kind();
            let completion = request
                .complete(sample(kind, opcode), waits, CycleInterrupts::default())
                .unwrap();
            assert_eq!(
                cpu.retire_physical_cycle(&mut state, completion),
                Err(CycleError::ClockOverflow)
            );
            assert_eq!(cpu, before);
            assert_eq!(state, BusState::default());
        }
    }
}
