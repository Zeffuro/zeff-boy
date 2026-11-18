use super::super::prefix_tests::{Bus, restored};
use super::*;

pub(super) fn retirement() {
    for prefix in [0xdd, 0xfd] {
        for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
            for opcode in [0x34, 0x35] {
                for (base, displacement, value, flags, waits, inc_f, dec_f) in [
                    (0, 0x80, 0, 0, 0, 0, 0xba),
                    (0xffff, 0x7f, 0xff, 0xff, 2, 0x50, 0xaa),
                    (0x1ba0, 7, 0x7f, 0xff, 2, 0x94, 0x2a),
                    (0x8000, 0xff, 0x80, 0, 0, 0x80, 0x3e),
                    (0xffff, 1, 0x0f, 0xff, 0, 0x10, 0x0a),
                    (0, 0xff, 0x10, 0, 2, 0, 0x1a),
                ] {
                    let result_flags =
                        (if opcode == 0x34 { inc_f } else { dec_f }) | (flags & Z80_FLAG_CARRY);
                    case(
                        prefix,
                        profile,
                        opcode,
                        (base, displacement),
                        (value, flags, result_flags),
                        waits,
                    );
                }
            }
        }
    }
}

fn case(
    prefix: u8,
    profile: MemptrProfile,
    opcode: u8,
    indexed: (u16, u8),
    arithmetic: (u8, u8, u8),
    waits: u32,
) {
    let (base, displacement) = indexed;
    let (value, flags, result_flags) = arithmetic;
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0xfffe, 0x2345), profile);
    cpu.regs.ix = base;
    cpu.regs.iy = base;
    cpu.regs.f = flags;
    cpu.regs.r = 0xff;
    let mut state = BusState::default();
    for byte in [prefix, opcode] {
        let mut source = bus(cpu.regs.pc, byte, false);
        let step = cpu.step_bus_segment(&mut source, &mut state).unwrap();
        assert_eq!(
            (step.instruction.cycles, step.complete, source.calls.get()),
            (4, false, 1)
        );
    }
    cpu.enable_interrupts_delay = 1;
    let before = cpu.regs;
    let address = base.wrapping_add_signed(i16::from(displacement as i8));
    let result = if opcode == 0x34 {
        value.wrapping_add(1)
    } else {
        value.wrapping_sub(1)
    };
    for (phase, cycles, receipt) in [
        (
            0,
            3,
            IndexedMemoryReceipt::Read {
                address: 0,
                value: displacement,
                known_mask: 255,
            },
        ),
        (1, 5, IndexedMemoryReceipt::Internal),
        (
            2,
            4,
            IndexedMemoryReceipt::Read {
                address,
                value,
                known_mask: 255,
            },
        ),
        (
            3,
            3,
            IndexedMemoryReceipt::Write {
                address,
                value: result,
            },
        ),
    ] {
        assert!(state.indexed_memory_pending());
        let request = cpu.next_indexed_memory_cycle(&state).unwrap();
        let kind = request.kind();
        assert_eq!(kind.t_states(), cycles);
        assert_eq!(kind.finishes(), phase == 3);
        match phase {
            0 => assert!(matches!(
                kind,
                IndexedMemoryKind::Displacement { address: 0, .. }
            )),
            1 => assert_eq!(
                kind,
                IndexedMemoryKind::Address {
                    retained_address: 0
                }
            ),
            2 => {
                assert_eq!(kind, IndexedMemoryKind::ReadModify { address });
                assert_eq!(request.preceding_address(), 0);
            }
            _ => {
                assert_eq!(
                    kind,
                    IndexedMemoryKind::Write {
                        address,
                        value: result
                    }
                );
                assert_eq!(request.preceding_address(), address);
            }
        }
        if prefix == 0xdd && profile == MemptrProfile::Nmos && base == 0x1ba0 {
            guards(&cpu, state, receipt, phase);
        }
        retire(
            &mut cpu,
            &mut state,
            receipt,
            if phase == 1 { 0 } else { waits },
        );
        assert_eq!(cpu.regs.f, if phase == 3 { result_flags } else { flags });
        assert_eq!(cpu.enable_interrupts_delay, u8::from(phase != 3));
    }
    let mut expected = before;
    expected.pc = 1;
    expected.f = result_flags;
    assert_eq!(cpu.regs, expected);
    assert_eq!(cpu.cycles(), 23 + 3 * u64::from(waits));
    assert_eq!(cpu.last_m1_fetch_count, 2);
    assert!(!state.pending());
    assert_eq!(
        cpu.memptr(),
        (profile == MemptrProfile::Nmos).then_some(address)
    );
}

fn bus(address: u16, value: u8, write: bool) -> Bus {
    Bus {
        sample: CycleSample {
            address,
            value,
            write,
            m1: false,
            known_mask: 255,
        },
        interrupts: CycleInterrupts::default(),
        calls: std::cell::Cell::new(0),
    }
}

fn retire(cpu: &mut Cpu, state: &mut BusState, receipt: IndexedMemoryReceipt, waits: u32) {
    let (mut oracle, mut after) = (cpu.clone(), *state);
    let kind = cpu.next_indexed_memory_cycle(state).unwrap().kind();
    let (value, write) = match receipt {
        IndexedMemoryReceipt::Read { value, .. } => (value, false),
        IndexedMemoryReceipt::Write { value, .. } => (value, true),
        IndexedMemoryReceipt::Internal => (0, false),
    };
    let mut source = bus(kind.address(), value, write);
    source.interrupts = CycleInterrupts {
        maskable_pending: true,
        nmi_pending: true,
    };
    let mut expected = oracle.step_bus_segment(&mut source, &mut after).unwrap();
    assert_eq!(
        source.calls.get(),
        u8::from(receipt != IndexedMemoryReceipt::Internal)
    );
    assert_eq!(expected.instruction.cycles, kind.t_states());
    assert!(oracle.extend_bus_segment(&mut expected.instruction, waits));
    let (mut clone, mut clone_state) = restored(cpu, state);
    let completion = cpu
        .next_indexed_memory_cycle(state)
        .unwrap()
        .complete(receipt, waits)
        .unwrap();
    assert_eq!(
        cpu.retire_indexed_memory_cycle(state, completion).unwrap(),
        expected
    );
    let completion = clone
        .next_indexed_memory_cycle(&clone_state)
        .unwrap()
        .complete(receipt, waits)
        .unwrap();
    assert_eq!(
        clone
            .retire_indexed_memory_cycle(&mut clone_state, completion)
            .unwrap(),
        expected
    );
    assert_eq!((&*cpu, *state), (&oracle, after));
    assert_eq!((&clone, clone_state), (&*cpu, *state));
    assert!(!cpu.last_step_was_interrupt);
}

fn guards(cpu: &Cpu, state: BusState, receipt: IndexedMemoryReceipt, phase: usize) {
    for field in 0..7 {
        let mut forged = cpu.clone();
        match field {
            0 => forged.instruction_byte_count = 4,
            1 => forged.instruction_bytes[3] = 1,
            2 => forged.regs.pc ^= 1,
            3 => forged.last_m1_fetch_count = 3,
            4 => forged.instruction_bytes[0] = 0xed,
            5 => forged.regs.ix ^= 1,
            _ => forged.last_opcode_pc ^= 1,
        }
        assert!(forged.next_indexed_memory_cycle(&state).is_err());
    }
    let kind = cpu.next_indexed_memory_cycle(&state).unwrap().kind();
    let malformed = match receipt {
        IndexedMemoryReceipt::Read { address, value, .. } => vec![
            IndexedMemoryReceipt::Internal,
            IndexedMemoryReceipt::Read {
                address: address ^ 1,
                value,
                known_mask: 255,
            },
            IndexedMemoryReceipt::Read {
                address,
                value,
                known_mask: 254,
            },
            IndexedMemoryReceipt::Write { address, value },
        ],
        IndexedMemoryReceipt::Write { address, value } => vec![
            IndexedMemoryReceipt::Internal,
            IndexedMemoryReceipt::Write {
                address: address ^ 1,
                value,
            },
            IndexedMemoryReceipt::Write {
                address,
                value: value ^ 1,
            },
            IndexedMemoryReceipt::Read {
                address,
                value,
                known_mask: 255,
            },
        ],
        IndexedMemoryReceipt::Internal => vec![IndexedMemoryReceipt::Read {
            address: kind.address(),
            value: 0,
            known_mask: 255,
        }],
    };
    for bad in malformed {
        assert!(
            cpu.next_indexed_memory_cycle(&state)
                .unwrap()
                .complete(bad, 0)
                .is_err()
        );
    }
    assert!(
        cpu.next_indexed_memory_cycle(&state)
            .unwrap()
            .complete(receipt, u32::MAX)
            .is_err()
    );
    let mut overflow = cpu.clone();
    overflow.cycles = u64::MAX;
    assert!(
        overflow
            .next_indexed_memory_cycle(&state)
            .unwrap()
            .complete(receipt, 0)
            .is_err()
    );
    for changed in 0..3 {
        let completion = cpu
            .next_indexed_memory_cycle(&state)
            .unwrap()
            .complete(receipt, 0)
            .unwrap();
        let (mut stale, mut after) = (cpu.clone(), state);
        match changed {
            0 => stale.regs.f ^= 1,
            1 => after.transfer.as_mut().unwrap().pc ^= 1,
            _ => {
                stale.reset();
                after = BusState::default();
            }
        }
        let before = (stale.clone(), after);
        assert_eq!(
            stale.retire_indexed_memory_cycle(&mut after, completion),
            Err(CycleError::StaleContinuation)
        );
        assert_eq!((stale, after), before);
    }
    if phase >= 2 {
        let mut forged = cpu.clone();
        forged.memptr = Some(kind.address() ^ 1);
        assert!(forged.next_indexed_memory_cycle(&state).is_err());
        forged_rmw(cpu, state, kind, phase);
    }
}

fn forged_rmw(cpu: &Cpu, state: BusState, kind: IndexedMemoryKind, phase: usize) {
    for mutation in 0..5 {
        let mut writer = crate::state::Writer::new();
        writer.write_u8(4);
        writer.write_u16(kind.address() ^ u16::from(mutation == 0));
        crate::state::write_option_u8((mutation == 1).then_some(6), &mut writer);
        if phase == 2 {
            writer.write_u8(0);
            if mutation == 2 {
                writer.write_u8(2);
                writer.write_u8(0x06);
            } else {
                let opcode = cpu.instruction_bytes[1];
                writer.write_u8(u8::from(opcode == 0x35) ^ u8::from(mutation >= 3));
            }
        } else {
            let IndexedMemoryKind::Write { value, .. } = kind else {
                unreachable!()
            };
            let opcode = cpu.instruction_bytes[1];
            let flags = if opcode == 0x34 {
                alu::inc_flags(value.wrapping_sub(1), value, cpu.regs.f)
            } else {
                alu::dec_flags(value.wrapping_add(1), value, cpu.regs.f)
            };
            writer.write_u8(2);
            writer.write_u8(value ^ u8::from(mutation == 2));
            writer.write_u8(flags ^ u8::from(mutation == 3));
            crate::state::write_option_u8((mutation == 4).then_some(0), &mut writer);
        }
        let bytes = writer.into_bytes();
        let operation = indexed_memory::IndexedMemory::read_continuation(
            &mut crate::state::Reader::new(&bytes),
        )
        .unwrap();
        let mut forged = state;
        forged.transfer.as_mut().unwrap().operation = Operation::IndexedMemory(operation);
        assert!(forged.indexed_memory_pending());
        assert!(cpu.next_indexed_memory_cycle(&forged).is_err());
    }
}
