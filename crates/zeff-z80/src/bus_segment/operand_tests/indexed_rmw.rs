use super::*;

pub(super) fn retirement() {
    for prefix in [0xdd, 0xfd] {
        for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
            for opcode in (0..=0xff).filter(|opcode| opcode >> 6 != 1) {
                for (base, displacement, value, flags, waits) in [
                    (0, 0x80, 0, 0, 0),
                    (0xffff, 0x7f, 0xff, 0xff, 2),
                    (0x1798, 0, 0x81, Z80_FLAG_CARRY, 0),
                ] {
                    case(
                        (prefix, opcode),
                        profile,
                        (base, displacement),
                        value,
                        flags,
                        waits,
                    );
                }
            }
        }
    }
}

fn case(
    instruction: (u8, u8),
    profile: MemptrProfile,
    indexed: (u16, u8),
    value: u8,
    flags: u8,
    waits: u32,
) {
    let (prefix, opcode) = instruction;
    let (base, displacement) = indexed;
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0xfffe, 0x2345), profile);
    cpu.regs.ix = base;
    cpu.regs.iy = base;
    cpu.regs.set_bc(0x1234);
    cpu.regs.set_de(0x5678);
    cpu.regs.set_hl(0x9abc);
    cpu.regs.a = 0xde;
    cpu.regs.f = flags;
    cpu.regs.r = 0xff;
    let mut state = BusState::default();
    for byte in [prefix, 0xcb] {
        cpu.step_bus_segment(&mut read(cpu.regs.pc, byte, false), &mut state)
            .unwrap();
    }
    cpu.enable_interrupts_delay = 1;
    let before = cpu.regs;
    let effective = base.wrapping_add_signed(i16::from(displacement as i8));
    for (phase, byte) in [displacement, opcode, value].into_iter().enumerate() {
        let kind = cpu.next_indexed_cb_cycle(&state).unwrap().kind();
        assert_eq!(kind.t_states(), [3, 5, 4][phase]);
        assert!(!kind.finishes());
        if phase == 2 {
            assert_eq!(kind, IndexedCbCycleKind::ReadModify { address: effective });
        }
        let receipt = IndexedMemoryReceipt::Read {
            address: kind.address(),
            value: byte,
            known_mask: 255,
        };
        retire(&mut cpu, &mut state, receipt, waits);
        assert_eq!(cpu.regs.f, flags);
        assert_eq!(cpu.enable_interrupts_delay, 1);
        assert_eq!(
            (cpu.regs.bc(), cpu.regs.de(), cpu.regs.hl(), cpu.regs.a),
            (before.bc(), before.de(), before.hl(), before.a)
        );
    }
    let (result, result_flags, write) = cb::effect(opcode, value, flags);
    assert!(write);
    let request = cpu.next_indexed_cb_cycle(&state).unwrap();
    assert_eq!(
        request.kind(),
        IndexedCbCycleKind::Write {
            address: effective,
            value: result
        }
    );
    assert_eq!(request.preceding_address(), effective);
    assert!(request.kind().finishes());
    if prefix == 0xdd && profile == MemptrProfile::Nmos && displacement == 0 && opcode & 7 == 6 {
        write_guards(&cpu, state, effective, result);
    }
    retire(
        &mut cpu,
        &mut state,
        IndexedMemoryReceipt::Write {
            address: effective,
            value: result,
        },
        waits,
    );
    assert!(!state.pending());
    assert_eq!(cpu.regs.pc, 2);
    assert_eq!(cpu.cycles(), 23 + 4 * u64::from(waits));
    assert_eq!(cpu.enable_interrupts_delay, 0);
    assert_eq!(cpu.regs.f, result_flags);
    let mut expected = before;
    expected.pc = 2;
    expected.f = result_flags;
    match opcode & 7 {
        0 => expected.b = result,
        1 => expected.c = result,
        2 => expected.d = result,
        3 => expected.e = result,
        4 => expected.h = result,
        5 => expected.l = result,
        6 => {}
        7 => expected.a = result,
        _ => unreachable!(),
    }
    assert_eq!(cpu.regs, expected);
    assert_eq!(
        cpu.memptr(),
        (profile == MemptrProfile::Nmos).then_some(effective)
    );
}

fn retire(cpu: &mut Cpu, state: &mut BusState, receipt: IndexedMemoryReceipt, waits: u32) {
    let (mut oracle, mut after) = (cpu.clone(), *state);
    let (address, value, write) = match receipt {
        IndexedMemoryReceipt::Read { address, value, .. } => (address, value, false),
        IndexedMemoryReceipt::Write { address, value } => (address, value, true),
        _ => unreachable!(),
    };
    let mut bus = read(address, value, true);
    bus.sample.write = write;
    let mut expected = oracle.step_bus_segment(&mut bus, &mut after).unwrap();
    assert_eq!(bus.calls.get(), 1);
    assert!(oracle.extend_bus_segment(&mut expected.instruction, waits));
    let (mut restored_cpu, mut restored_state) = restored(cpu, state);
    let completion = cpu
        .next_indexed_cb_cycle(state)
        .unwrap()
        .complete(receipt, waits)
        .unwrap();
    assert_eq!(
        cpu.retire_indexed_cb_cycle(state, completion).unwrap(),
        expected
    );
    let completion = restored_cpu
        .next_indexed_cb_cycle(&restored_state)
        .unwrap()
        .complete(receipt, waits)
        .unwrap();
    assert_eq!(
        restored_cpu
            .retire_indexed_cb_cycle(&mut restored_state, completion)
            .unwrap(),
        expected
    );
    assert_eq!((&*cpu, *state), (&oracle, after));
    assert_eq!((&restored_cpu, restored_state), (&*cpu, *state));
}

fn write_guards(cpu: &Cpu, state: BusState, address: u16, value: u8) {
    let opcode = cpu.instruction_bytes[3];
    let (_, expected_flags, _) = cb::effect(opcode, 0x81, cpu.regs.f);
    forged_write(
        cpu,
        state,
        address,
        (value, expected_flags),
        (Some((opcode & 7) ^ 1), None),
    );
    forged_write(
        cpu,
        state,
        address ^ 1,
        (value, expected_flags),
        (Some(opcode & 7), None),
    );
    forged_write(
        cpu,
        state,
        address,
        (value, expected_flags),
        (Some(opcode & 7), Some(0)),
    );
    forged_write(
        cpu,
        state,
        address,
        (value, expected_flags ^ Z80_FLAG_HALF_CARRY),
        (Some(opcode & 7), None),
    );
    let bad_value = match opcode >> 6 {
        2 | 3 => value ^ (1 << ((opcode >> 3) & 7)),
        _ => match (opcode >> 3) & 7 {
            0 | 2 | 4 | 6 => value ^ 1,
            _ => value ^ 0x80,
        },
    };
    let bad_flags = if opcode >> 6 == 0 {
        flags::szp_flags(bad_value) | (expected_flags & Z80_FLAG_CARRY)
    } else {
        expected_flags
    };
    forged_write(
        cpu,
        state,
        address,
        (bad_value, bad_flags),
        (Some(opcode & 7), None),
    );
    let mut bit_write = cpu.clone();
    bit_write.instruction_bytes[3] = 0x46;
    assert!(bit_write.next_indexed_cb_cycle(&state).is_err());
    let mut writer = crate::state::Writer::new();
    writer.write_u8(2);
    writer.write_u16(address);
    crate::state::write_option_u8(Some(opcode & 7), &mut writer);
    writer.write_u8(0);
    writer.write_u8(0);
    let bytes = writer.into_bytes();
    let non_cb =
        indexed_cb::IndexedCb::read_continuation(&mut crate::state::Reader::new(&bytes)).unwrap();
    let mut forged = state;
    forged.transfer.as_mut().unwrap().operation = Operation::IndexedCb(non_cb);
    assert!(cpu.next_indexed_cb_cycle(&forged).is_err());
    for receipt in [
        IndexedMemoryReceipt::Internal,
        IndexedMemoryReceipt::Read {
            address,
            value,
            known_mask: 255,
        },
        IndexedMemoryReceipt::Write {
            address: address ^ 1,
            value,
        },
        IndexedMemoryReceipt::Write {
            address,
            value: value ^ 1,
        },
    ] {
        assert!(
            cpu.next_indexed_cb_cycle(&state)
                .unwrap()
                .complete(receipt, 0)
                .is_err()
        );
    }
    let receipt = IndexedMemoryReceipt::Write { address, value };
    assert!(
        cpu.next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, u32::MAX)
            .is_err()
    );
    let mut overflow = cpu.clone();
    overflow.cycles = u64::MAX;
    assert!(
        overflow
            .next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, 0)
            .is_err()
    );
    let completion = cpu
        .next_indexed_cb_cycle(&state)
        .unwrap()
        .complete(receipt, 0)
        .unwrap();
    let mut stale = cpu.clone();
    stale.regs.f ^= 1;
    let before = stale.clone();
    let mut after = state;
    assert!(matches!(
        stale.retire_indexed_cb_cycle(&mut after, completion),
        Err(CycleError::StaleContinuation)
    ));
    assert_eq!((stale, after), (before, state));
}

fn forged_write(
    cpu: &Cpu,
    state: BusState,
    address: u16,
    result: (u8, u8),
    destination: (Option<u8>, Option<u8>),
) {
    let (value, flags) = result;
    let (register, a) = destination;
    let mut writer = crate::state::Writer::new();
    writer.write_u8(2);
    writer.write_u16(address);
    crate::state::write_option_u8(register, &mut writer);
    writer.write_u8(2);
    writer.write_u8(value);
    writer.write_u8(flags);
    crate::state::write_option_u8(a, &mut writer);
    let bytes = writer.into_bytes();
    let operation =
        indexed_cb::IndexedCb::read_continuation(&mut crate::state::Reader::new(&bytes)).unwrap();
    let mut forged = state;
    forged.transfer.as_mut().unwrap().operation = Operation::IndexedCb(operation);
    assert!(cpu.indexed_cb_cycle_pending(&forged));
    assert!(cpu.next_indexed_cb_cycle(&forged).is_err());
}
