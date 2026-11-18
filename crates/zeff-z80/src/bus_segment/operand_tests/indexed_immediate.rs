use super::super::prefix_tests::{Bus, restored};
use super::*;

fn bus(address: u16, value: u8, write: bool) -> Bus {
    Bus {
        sample: CycleSample {
            address,
            value,
            known_mask: 255,
            m1: false,
            write,
        },
        interrupts: CycleInterrupts::default(),
        calls: std::cell::Cell::new(0),
    }
}

pub(super) fn retirement() {
    for prefix in [0xdd, 0xfd] {
        for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
            for (base, displacement, value, waits) in [
                (0, 0x80, 0, 0),
                (0xffff, 0x7f, 0xff, 2),
                (0x1cc0, 0x0b, 0, 0),
                (0x000a, 0, 0, 2),
            ] {
                case(prefix, profile, base, displacement, value, waits);
            }
        }
    }
}

fn case(prefix: u8, profile: MemptrProfile, base: u16, displacement: u8, value: u8, waits: u32) {
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0xfffe, 0x2345), profile);
    cpu.regs.ix = base;
    cpu.regs.iy = base;
    cpu.regs.f = 0xa5;
    let mut state = BusState::default();
    for byte in [prefix, 0x36] {
        assert!(cpu.next_indexed_memory_cycle(&state).is_err());
        let mut source = bus(cpu.regs.pc, byte, false);
        let step = cpu.step_bus_segment(&mut source, &mut state).unwrap();
        assert_eq!(source.calls.get(), 1);
        assert_eq!(step.instruction.cycles, 4);
        assert!(!step.complete);
    }
    let address = base.wrapping_add_signed(i16::from(displacement as i8));
    cpu.enable_interrupts_delay = 1;
    for (phase, receipt) in [
        IndexedMemoryReceipt::Read {
            address: 0,
            value: displacement,
            known_mask: 255,
        },
        IndexedMemoryReceipt::Read {
            address: 1,
            value,
            known_mask: 255,
        },
        IndexedMemoryReceipt::Write { address, value },
    ]
    .into_iter()
    .enumerate()
    {
        assert!(state.indexed_memory_pending());
        let request = cpu.next_indexed_memory_cycle(&state).unwrap();
        let kind = request.kind();
        assert_eq!(kind.t_states(), [3, 5, 3][phase]);
        assert_eq!(kind.finishes(), phase == 2);
        match phase {
            0 => assert!(matches!(
                kind,
                IndexedMemoryKind::Displacement { address: 0, .. }
            )),
            1 => {
                assert_eq!(kind, IndexedMemoryKind::Immediate { address: 1 });
                assert_eq!(request.preceding_address(), 0);
            }
            _ => {
                assert_eq!(kind, IndexedMemoryKind::Write { address, value });
                assert_eq!(request.preceding_address(), 1);
                guards(&cpu, state, address, value);
            }
        }
        phase_guards(&cpu, state, receipt);
        retire(&mut cpu, &mut state, receipt, waits);
        assert_eq!(cpu.regs.f, 0xa5);
        assert_eq!(cpu.enable_interrupts_delay, u8::from(phase != 2));
        assert!(!cpu.last_step_was_interrupt);
    }
    assert_eq!(cpu.cycles(), 19 + 3 * u64::from(waits));
    assert_eq!(cpu.regs.pc, 2);
    assert_eq!(cpu.regs.f, 0xa5);
    assert_eq!(cpu.last_m1_fetch_count, 2);
    assert_eq!(cpu.regs.r, 2);
    assert!(!state.pending());
    assert_eq!(
        cpu.memptr(),
        (profile == MemptrProfile::Nmos).then_some(address)
    );
}

fn retire(cpu: &mut Cpu, state: &mut BusState, receipt: IndexedMemoryReceipt, waits: u32) {
    let request = cpu.next_indexed_memory_cycle(state).unwrap();
    let (address, value, write) = match receipt {
        IndexedMemoryReceipt::Read { address, value, .. } => (address, value, false),
        IndexedMemoryReceipt::Write { address, value } => (address, value, true),
        _ => unreachable!(),
    };
    let (mut expected, mut expected_state) = (cpu.clone(), *state);
    let mut target = bus(address, value, write);
    target.interrupts = CycleInterrupts {
        maskable_pending: true,
        nmi_pending: true,
    };
    let mut step = expected
        .step_bus_segment(&mut target, &mut expected_state)
        .unwrap();
    assert_eq!(target.calls.get(), 1);
    assert_eq!(step.complete, request.kind().finishes());
    assert_eq!(step.instruction.cycles, request.kind().t_states());
    assert!(expected.extend_bus_segment(&mut step.instruction, waits));
    let (mut clone, mut clone_state) = restored(cpu, state);
    let completion = request.complete(receipt, waits).unwrap();
    assert_eq!(
        cpu.retire_indexed_memory_cycle(state, completion).unwrap(),
        step
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
        step
    );
    assert_eq!((&*cpu, *state), (&expected, expected_state));
    assert_eq!((&clone, clone_state), (&*cpu, *state));
}

fn phase_guards(cpu: &Cpu, state: BusState, receipt: IndexedMemoryReceipt) {
    for field in 0..7 {
        let mut forged = cpu.clone();
        match field {
            0 => {
                forged.instruction_byte_count = if cpu.instruction_byte_count == 4 {
                    3
                } else {
                    4
                }
            }
            1 => forged.instruction_bytes[0] = 0xed,
            2 => forged.regs.pc ^= 1,
            3 => forged.last_m1_fetch_count = 3,
            4 => forged.last_opcode_pc ^= 1,
            5 => {
                if cpu.instruction_bytes[0] == 0xfd {
                    forged.regs.iy ^= 1
                } else {
                    forged.regs.ix ^= 1
                }
            }
            _ => forged.instruction_bytes[2] ^= 1,
        }
        assert!(forged.next_indexed_memory_cycle(&state).is_err());
    }
    if let IndexedMemoryReceipt::Read { address, value, .. } = receipt {
        for malformed in [
            IndexedMemoryReceipt::Internal,
            IndexedMemoryReceipt::Write { address, value },
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
        ] {
            assert!(
                cpu.next_indexed_memory_cycle(&state)
                    .unwrap()
                    .complete(malformed, 0)
                    .is_err()
            );
        }
        let phase = u8::from(cpu.instruction_byte_count == 3);
        let mut writer = crate::state::Writer::new();
        if phase == 0 {
            writer.write_u8(0);
            writer.write_u8(4);
            let base = if cpu.instruction_bytes[0] == 0xfd {
                cpu.regs.iy
            } else {
                cpu.regs.ix
            };
            writer.write_u16(base ^ 1);
        } else {
            writer.write_u8(2);
            let base = if cpu.instruction_bytes[0] == 0xfd {
                cpu.regs.iy
            } else {
                cpu.regs.ix
            };
            writer
                .write_u16(base.wrapping_add_signed(i16::from(cpu.instruction_bytes[2] as i8)) ^ 1);
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
}

fn guards(cpu: &Cpu, state: BusState, address: u16, value: u8) {
    for field in 0..8 {
        let mut forged = cpu.clone();
        match field {
            0 => forged.instruction_byte_count = 3,
            1 => forged.instruction_bytes[3] ^= 1,
            2 => forged.instruction_bytes[2] ^= 1,
            3 => forged.regs.pc ^= 1,
            4 => forged.last_m1_fetch_count = 3,
            5 => forged.instruction_bytes[0] ^= 1,
            6 => forged.last_opcode_pc ^= 1,
            _ => forged.instruction_bytes[1] = 0x70,
        }
        assert!(forged.next_indexed_memory_cycle(&state).is_err());
    }
    for (phase, byte, captured) in [
        (0, value, address),
        (1, value, address),
        (3, value ^ 1, address),
        (3, value, address ^ 1),
    ] {
        let mut writer = crate::state::Writer::new();
        writer.write_u8(phase);
        writer.write_u8(1);
        writer.write_u8(byte);
        writer.write_u16(captured);
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
    for malformed in [
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
    ] {
        assert!(
            cpu.next_indexed_memory_cycle(&state)
                .unwrap()
                .complete(malformed, 0)
                .is_err()
        );
    }
}
