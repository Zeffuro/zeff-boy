use super::*;

fn rejected(cpu: &Cpu, state: &BusState) {
    assert!(matches!(
        cpu.next_physical_cycle(state),
        Err(CycleError::UnsupportedCycle)
    ));
}

pub(super) fn unsupported(prefix: u8, opcode: u8) {
    let (cpu, state) = super::super::prefix_tests::start(prefix);
    let before = cpu.clone();
    let before_state = state;
    let request = cpu.next_physical_cycle(&state).unwrap();
    let kind = request.kind();
    let completion = request.complete(
        sample(kind, opcode),
        2,
        CycleInterrupts {
            maskable_pending: true,
            nmi_pending: true,
        },
    );
    assert!(
        matches!(completion, Err(CycleError::UnsupportedOpcode { pc: 1, opcode: actual }) if actual == opcode)
    );
    assert_eq!(cpu, before);
    assert_eq!(state, before_state);
}

pub(super) fn repeated(prefix: u8) {
    for second in [Z80_PREFIX_DD, Z80_PREFIX_FD] {
        let (mut cpu, mut state) = super::super::prefix_tests::start(prefix);
        let mut bus = Bus {
            sample: sample(m1(1, 0), second),
            interrupts: CycleInterrupts::default(),
            calls: std::cell::Cell::new(0),
        };
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        assert_eq!(cpu.instruction_byte_count, 2);
        assert_eq!(cpu.last_m1_fetch_count, 2);
        rejected(&cpu, &state);
        bus.sample = sample(m1(2, 0), 0x21);
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
        assert_eq!(cpu.instruction_byte_count, 3);
        assert_eq!(cpu.last_m1_fetch_count, 3);
        rejected(&cpu, &state);
    }
}

pub(super) fn pending(cpu: &Cpu, state: BusState) {
    let Some((pc, prefix)) = state.prefix.selected_index() else {
        return;
    };
    for variant in 0..9 {
        let mut candidate = cpu.clone();
        let mut candidate_state = state;
        match variant {
            0 => candidate_state.prefix = PrefixState::index(pc ^ 1, prefix),
            1 => candidate_state.prefix = PrefixState::index(pc, prefix ^ 0x20),
            2 => candidate.last_opcode ^= 0x20,
            3 => candidate.instruction_bytes[0] ^= 0x20,
            4 => candidate.instruction_byte_count ^= 1,
            5 => candidate.last_m1_fetch_count += 1,
            6 => candidate.last_opcode_pc ^= 1,
            7 => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
            _ => candidate_state.prefix = PrefixState::default(),
        }
        rejected(&candidate, &candidate_state);
    }
    let Some(transfer) = state.transfer else {
        return;
    };
    let mut bad = state;
    bad.transfer.as_mut().unwrap().pc ^= 1;
    rejected(cpu, &bad);
    let mut candidate = cpu.clone();
    candidate.instruction_bytes[1] = 0x36;
    rejected(&candidate, &state);
    let opcode = cpu.instruction_bytes[1];
    match transfer.operation {
        Operation::Single { .. } => {
            for (stored, index) in [
                (opcode ^ 8, Some(prefix)),
                (opcode, None),
                (opcode, Some(prefix ^ 0x20)),
            ] {
                let mut bad = state;
                bad.transfer.as_mut().unwrap().operation = Operation::Single {
                    opcode: stored,
                    index,
                };
                rejected(cpu, &bad);
            }
        }
        Operation::Load(_) => {
            for byte in 2..usize::from(cpu.instruction_byte_count) {
                let mut candidate = cpu.clone();
                candidate.instruction_bytes[byte] ^= 1;
                rejected(&candidate, &state);
            }
            if matches!(opcode, 0x21 | 0x22 | 0x2a) {
                let mut bad = state;
                bad.transfer.as_mut().unwrap().operation =
                    Operation::Load(load::Load::decode(cpu, opcode, Some(prefix ^ 0x20)).unwrap());
                rejected(cpu, &bad);
            }
            if opcode == 0x22 {
                let mut candidate = cpu.clone();
                if prefix == Z80_PREFIX_FD {
                    candidate.regs.iy ^= 0x100
                } else {
                    candidate.regs.ix ^= 0x100
                }
                rejected(&candidate, &state);
            }
            if opcode == 0x32 {
                let mut candidate = cpu.clone();
                candidate.regs.a ^= 1;
                rejected(&candidate, &state);
            }
            let operation =
                load::Load::read_continuation(&mut crate::state::Reader::new(&[0, 0, 0])).unwrap();
            let mut bad = state;
            bad.transfer.as_mut().unwrap().operation = Operation::Load(operation);
            rejected(cpu, &bad);
        }
        Operation::Stack(_) => {
            let mut candidate = cpu.clone();
            candidate.regs.sp ^= 1;
            rejected(&candidate, &state);
            if cpu.instruction_byte_count == 3 {
                candidate = cpu.clone();
                candidate.instruction_bytes[2] ^= 1;
                rejected(&candidate, &state);
            }
            if matches!(opcode, 0xe1 | 0xe2 | 0xea) {
                let mut bad = state;
                bad.transfer.as_mut().unwrap().operation = Operation::Stack(
                    stack::Stack::decode(cpu, opcode, Some(prefix ^ 0x20)).unwrap(),
                );
                rejected(cpu, &bad);
            }
        }
        _ => panic!("unexpected physical index family"),
    }
}
