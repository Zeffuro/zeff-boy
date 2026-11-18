use super::*;

fn rejected(cpu: &Cpu, state: &BusState) {
    assert!(matches!(
        cpu.next_physical_cycle(state),
        Err(CycleError::UnsupportedCycle)
    ));
}

pub(super) fn pending(cpu: &Cpu, state: BusState) {
    let Some(transfer) = state.transfer else {
        return;
    };
    if !matches!(transfer.operation, Operation::Rmw(_) | Operation::Stack(_)) {
        return;
    }
    for variant in 0..7 {
        let mut candidate = cpu.clone();
        let mut candidate_state = state;
        match variant {
            0 => candidate_state.transfer.as_mut().unwrap().pc ^= 1,
            1 => candidate.last_opcode_pc ^= 1,
            2 => candidate.last_opcode ^= 1,
            3 => candidate.instruction_bytes[0] ^= 1,
            4 => candidate.instruction_byte_count ^= 1,
            5 => candidate.last_m1_fetch_count += 1,
            _ => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
        }
        rejected(&candidate, &candidate_state);
    }
    match transfer.operation {
        Operation::Rmw(operation) => rmw(cpu, state, operation),
        Operation::Stack(operation) => stack(cpu, state, operation),
        _ => unreachable!(),
    }
}

fn replace_rmw(cpu: &Cpu, state: BusState, bytes: &[u8]) {
    let mut candidate = state;
    let operation = rmw::Rmw::read_continuation(&mut crate::state::Reader::new(bytes)).unwrap();
    candidate.transfer.as_mut().unwrap().operation = Operation::Rmw(operation);
    rejected(cpu, &candidate);
}

fn rmw(cpu: &Cpu, state: BusState, operation: rmw::Rmw) {
    let mut candidate = cpu.clone();
    candidate.regs.set_hl(candidate.regs.hl() ^ 1);
    rejected(&candidate, &state);
    let mut writer = crate::state::Writer::new();
    operation.write_continuation(&mut writer);
    let bytes = writer.into_bytes();
    assert_eq!(bytes[2], 0);
    let mut bad = bytes.clone();
    bad[0] ^= 1;
    replace_rmw(cpu, state, &bad);
    let mut bad = bytes.clone();
    bad[2] = 1;
    bad.insert(3, 0);
    replace_rmw(cpu, state, &bad);
    if bytes[3] == 0 {
        let mut bad = bytes.clone();
        if bad[4] == 2 {
            bad[5] ^= 8;
        } else {
            bad[4] ^= 1;
        }
        replace_rmw(cpu, state, &bad);
    }
    let [low, high] = cpu.regs.hl().to_le_bytes();
    for bad in [
        vec![low, high, 0, 0, 3, 0x67, 0xa5, 0],
        vec![low, high, 0, 1, 0x12, 0x34, 0xa5],
        vec![low, high, 0, 2, 0x12, 0x34, 1, 0xa5],
    ] {
        replace_rmw(cpu, state, &bad);
    }
    if cpu.last_opcode == Z80_PREFIX_CB {
        let mut candidate = cpu.clone();
        candidate.instruction_bytes[1] = 0x00;
        rejected(&candidate, &state);
        let mut candidate = cpu.clone();
        candidate.last_opcode = Z80_PREFIX_ED;
        candidate.instruction_bytes[0] = Z80_PREFIX_ED;
        rejected(&candidate, &state);
        candidate = cpu.clone();
        candidate.instruction_bytes[1] = 0x46;
        let write = [low, high, 0, 2, 0x12, 0x34, 0];
        replace_rmw(&candidate, state, &write);
    }
}

fn replace_stack(cpu: &Cpu, state: BusState, bytes: &[u8]) {
    let mut candidate = state;
    let operation = stack::Stack::read_continuation(&mut crate::state::Reader::new(bytes)).unwrap();
    candidate.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
    rejected(cpu, &candidate);
}

fn stack(cpu: &Cpu, state: BusState, operation: stack::Stack) {
    let mut candidate = cpu.clone();
    candidate.regs.sp ^= 1;
    rejected(&candidate, &state);
    let mut writer = crate::state::Writer::new();
    operation.write_continuation(&mut writer);
    let bytes = writer.into_bytes();
    let address = if bytes[0] <= 1 { 2 } else { 1 };
    let target = address + 2;
    assert_eq!(bytes[target], 0);
    assert_eq!(bytes[target + 1], 0);
    for variant in 0..5 {
        let mut bad = bytes.clone();
        match variant {
            0 => bad[address] ^= 1,
            1 => {
                if bad[0] == 0 {
                    bad[1] ^= 1
                } else {
                    bad[0] ^= 1
                }
            }
            2 => bad[target + 1] = 1,
            3 => {
                bad[target] = 1;
                bad.insert(target + 1, 0x34);
                bad.insert(target + 2, 0x12);
            }
            _ => {
                bad.truncate(target + 2);
                bad.extend([4, 1]);
            }
        }
        replace_stack(cpu, state, &bad);
    }
    if cpu.last_opcode == Z80_PREFIX_ED {
        for (iff1, iff2, delay) in [
            (true, false, 0),
            (false, true, 0),
            (true, true, 0),
            (false, false, 1),
            (false, false, 2),
        ] {
            let mut candidate = cpu.clone();
            candidate.interrupt_flip_flop_1 = iff1;
            candidate.interrupt_flip_flop_2 = iff2;
            candidate.enable_interrupts_delay = delay;
            rejected(&candidate, &state);
        }
        let mut candidate = cpu.clone();
        candidate.instruction_bytes[1] = 0x44;
        rejected(&candidate, &state);
        let mut candidate = cpu.clone();
        candidate.last_opcode = Z80_PREFIX_CB;
        candidate.instruction_bytes[0] = Z80_PREFIX_CB;
        rejected(&candidate, &state);
    }
}

pub(super) fn ed_return_admission(opcode: u8) {
    let (cpu, state) = super::super::prefix_tests::start(Z80_PREFIX_ED);
    for (iff1, iff2, delay) in [
        (true, false, 0),
        (false, true, 0),
        (true, true, 0),
        (false, false, 1),
        (false, false, 2),
    ] {
        let mut candidate = cpu.clone();
        candidate.interrupt_flip_flop_1 = iff1;
        candidate.interrupt_flip_flop_2 = iff2;
        candidate.enable_interrupts_delay = delay;
        let candidate_state = state;
        let request = candidate.next_physical_cycle(&candidate_state).unwrap();
        let kind = request.kind();
        let completion = request.complete(
            sample(kind, opcode),
            2,
            CycleInterrupts {
                maskable_pending: true,
                nmi_pending: true,
            },
        );
        let before = candidate.clone();
        assert!(
            matches!(completion, Err(CycleError::UnsupportedOpcode { pc: 1, opcode: actual }) if actual == opcode)
        );
        assert_eq!(candidate, before);
        assert_eq!(candidate_state, state);
    }
}

pub(super) fn bit_closed(opcode: u8) {
    for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
        bit_closed_profile(opcode, profile);
    }
}
fn bit_closed_profile(opcode: u8, profile: MemptrProfile) {
    let (mut cpu, mut state) = super::super::prefix_tests::start(Z80_PREFIX_CB);
    cpu.memptr_profile = profile;
    let request = cpu.next_physical_cycle(&state).unwrap();
    let kind = request.kind();
    let sample = sample(kind, opcode);
    let completion = request.complete(sample, 2, CycleInterrupts::default());
    let before = cpu.clone();
    let before_state = state;
    assert!(
        matches!(completion, Err(CycleError::UnsupportedOpcode { pc: 1, opcode: actual }) if actual == opcode)
    );
    assert_eq!(cpu, before);
    assert_eq!(state, before_state);
    let mut bus = Bus {
        sample,
        interrupts: CycleInterrupts::default(),
        calls: std::cell::Cell::new(0),
    };
    assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    assert_eq!(bus.calls.get(), 1);
    rejected(&cpu, &state);
    let [low, high] = cpu.regs.hl().to_le_bytes();
    if profile == MemptrProfile::Nmos {
        cpu.set_memptr(0x2800);
        assert!(cpu.next_physical_cycle(&state).is_ok());
    }
    replace_rmw(&cpu, state, &[low, high, 0, 2, 0x12, 0x34, 0]);
}
