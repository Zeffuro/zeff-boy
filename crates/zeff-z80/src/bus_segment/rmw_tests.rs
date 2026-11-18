use super::prefix_tests::{Bus, restored, sample, stale};
use super::*;

#[path = "rmw_tests/guards.rs"]
mod guards;

pub(super) fn physical_memory_retirement() {
    let mut counts = [0; 4];
    for opcode in u8::MIN..=u8::MAX {
        if opcode & 7 == REGISTER_MEMORY_INDEX && opcode >> 6 != 1 {
            counts[0] += 1;
            for (value, flags, waits) in [(0, 0, 0), (0x7f, Z80_FLAG_CARRY, 2), (0x80, 0xff, 5)] {
                qualified(Some(Z80_PREFIX_CB), opcode, value, flags, waits);
            }
        } else if opcode & 7 == REGISTER_MEMORY_INDEX {
            guards::bit_closed(opcode);
        }
        if matches!(opcode, 0x34 | 0x35) {
            counts[1] += 1;
            for (value, flags, waits) in [(0, 0, 0), (0x7f, Z80_FLAG_CARRY, 2), (0x80, 0xff, 5)] {
                qualified(None, opcode, value, flags, waits);
            }
        }
        if stack::Stack::is_pop(opcode) {
            counts[2] += 1;
            for flags in [0, 0xff] {
                qualified(None, opcode, 0, flags, 2);
            }
        }
        if stack::Stack::is_ed_return(opcode) {
            counts[3] += 1;
            qualified(Some(Z80_PREFIX_ED), opcode, 0, 0xff, 5);
            guards::ed_return_admission(opcode);
            legacy_interrupt_return(opcode);
        }
    }
    for bit in 0..8 {
        for history in [0x0800, 0x2800] {
            for carry in [0, 1] {
                qualified_profile(Some(0xcb), 0x46 | bit << 3, 0x81, carry, 2, Some(history));
            }
        }
    }
    assert_eq!(counts, [24, 2, 5, 8]);
}

fn m1(address: u16, refresh_address: u16) -> CycleKind {
    CycleKind::M1 {
        address,
        halted: false,
        refresh_address,
    }
}

fn qualified(prefix: Option<u8>, opcode: u8, value: u8, flags: u8, waits: u32) {
    qualified_profile(prefix, opcode, value, flags, waits, None);
}
fn qualified_profile(
    prefix: Option<u8>,
    opcode: u8,
    value: u8,
    flags: u8,
    waits: u32,
    history: Option<u16>,
) {
    let mut cpu = if history.is_some() {
        Cpu::new_with_memptr_profile(
            ResetState::new(Z80_RESET_PC, Z80_RESET_SP),
            MemptrProfile::Nmos,
        )
    } else {
        Cpu::new()
    };
    cpu.memptr = history;
    cpu.regs.pc = 0x2000;
    cpu.regs.sp = u16::MAX;
    cpu.regs.set_hl(0x8123);
    cpu.regs.a = 0xa5;
    cpu.regs.f = flags;
    cpu.regs.i = 0x37;
    cpu.regs.r = 0xff;
    let rmw = prefix == Some(Z80_PREFIX_CB) || matches!(opcode, 0x34 | 0x35);
    cpu.enable_interrupts_delay = u8::from(rmw);
    let ed_return = prefix == Some(Z80_PREFIX_ED);
    cpu.interrupt_flip_flop_1 = !ed_return && flags != 0;
    cpu.interrupt_flip_flop_2 = !ed_return && flags == 0;
    let before_iff = (cpu.interrupt_flip_flop_1, cpu.interrupt_flip_flop_2);
    let mut phases = vec![(m1(0x2000, 0x37ff), prefix.unwrap_or(opcode))];
    if prefix.is_some() {
        phases.push((m1(0x2001, 0x3780), opcode));
    }
    if rmw {
        phases.push((CycleKind::MemoryReadExtended { address: 0x8123 }, value));
        let output = match prefix {
            Some(Z80_PREFIX_CB) => {
                let (result, _, write) = cb::effect(opcode, value, flags);
                write.then_some(result)
            }
            _ => Some(if opcode == 0x34 {
                value.wrapping_add(1)
            } else {
                value.wrapping_sub(1)
            }),
        };
        if let Some(value) = output {
            phases.push((
                CycleKind::MemoryWrite {
                    address: 0x8123,
                    value,
                },
                value,
            ));
        }
    } else {
        phases.extend([
            (CycleKind::MemoryRead { address: u16::MAX }, 0x34),
            (CycleKind::MemoryRead { address: 0 }, 0x12),
        ]);
    }
    let phase_count = phases.len();
    let mut state = BusState::default();
    let mut legacy = cpu.clone();
    let mut legacy_state = state;
    let mut total = 0;
    for (index, (kind, value)) in phases.into_iter().enumerate() {
        let inputs = CycleInterrupts {
            maskable_pending: index != 0,
            nmi_pending: index != 0,
        };
        let (mut clone, mut clone_state) = restored(&cpu, &state);
        let request = cpu.next_physical_cycle(&state).unwrap();
        assert_eq!(request.kind(), kind);
        let sample = sample(kind, value);
        let base = request.sampled_t_states(sample).unwrap();
        assert_eq!(
            kind.t_states(),
            match kind {
                CycleKind::M1 { .. } => None,
                CycleKind::MemoryReadExtended { .. } => Some(4),
                _ => Some(3),
            }
        );
        assert_eq!(
            base,
            if kind.is_m1() || matches!(kind, CycleKind::MemoryReadExtended { .. }) {
                4
            } else {
                3
            }
        );
        if let CycleKind::MemoryReadExtended { .. } = kind {
            assert!(!kind.is_m1() && !kind.is_write());
            assert_eq!(state.next_memory_cycle(), None);
        }
        if ed_return && !kind.is_m1() {
            assert_eq!(state.next_memory_cycle(), None);
        }
        guards::pending(&cpu, state);
        for (bad, error) in [
            (
                CycleSample {
                    known_mask: 0xfe,
                    ..sample
                },
                CycleError::UnknownData,
            ),
            (
                CycleSample {
                    address: sample.address ^ 1,
                    ..sample
                },
                CycleError::SampleMismatch,
            ),
            (
                CycleSample {
                    m1: !sample.m1,
                    ..sample
                },
                CycleError::SampleMismatch,
            ),
            (
                CycleSample {
                    write: !sample.write,
                    ..sample
                },
                CycleError::SampleMismatch,
            ),
        ] {
            assert!(
                matches!(cpu.next_physical_cycle(&state).unwrap().complete(bad, waits, inputs), Err(actual) if actual == error)
            );
        }
        if kind.is_write() {
            assert!(matches!(
                cpu.next_physical_cycle(&state).unwrap().complete(
                    CycleSample {
                        value: value ^ 1,
                        ..sample
                    },
                    waits,
                    inputs
                ),
                Err(CycleError::SampleMismatch)
            ));
        }
        stale(&cpu, &state, sample, inputs);
        if let Some(history) = history {
            for changed in [None, Some(history ^ 0x2000)] {
                let completion = cpu
                    .next_physical_cycle(&state)
                    .unwrap()
                    .complete(sample, waits, inputs)
                    .unwrap();
                let mut candidate = cpu.clone();
                candidate.memptr = changed;
                let before = candidate.clone();
                let mut saved = state;
                assert!(matches!(
                    candidate.retire_physical_cycle(&mut saved, completion),
                    Err(CycleError::StaleContinuation)
                ));
                assert!(candidate == before && saved == state);
            }
        }
        let before = cpu.clone();
        let before_state = state;
        let completion = request.complete(sample, waits, inputs).unwrap();
        assert_eq!(cpu, before);
        assert_eq!(state, before_state);
        let mut bus = Bus {
            sample,
            interrupts: inputs,
            calls: std::cell::Cell::new(0),
        };
        let mut expected = legacy
            .step_bus_segment(&mut bus, &mut legacy_state)
            .unwrap();
        assert_eq!(bus.calls.get(), 1);
        assert_eq!(expected.instruction.cycles, base);
        assert_eq!(expected.complete, index + 1 == phase_count);
        assert!(legacy.extend_bus_segment(&mut expected.instruction, waits));
        assert_eq!(
            cpu.retire_physical_cycle(&mut state, completion).unwrap(),
            expected
        );
        assert_eq!(cpu, legacy);
        assert_eq!(state, legacy_state);
        let completion = clone
            .next_physical_cycle(&clone_state)
            .unwrap()
            .complete(sample, waits, inputs)
            .unwrap();
        assert_eq!(
            clone
                .retire_physical_cycle(&mut clone_state, completion)
                .unwrap(),
            expected
        );
        assert_eq!(clone, cpu);
        assert_eq!(clone_state, state);
        total += u64::from(base + waits);
        assert_eq!(cpu.cycles(), total);
        assert_eq!(
            cpu.enable_interrupts_delay,
            u8::from(rmw && index + 1 != phase_count)
        );
        assert_eq!(cpu.last_opcode, prefix.unwrap_or(opcode));
        assert_eq!(cpu.last_opcode_pc, 0x2000);
        if !rmw && !kind.is_m1() && !expected.complete {
            assert_eq!(cpu.regs.sp, 0);
            assert_eq!(cpu.regs.pc, if ed_return { 0x2002 } else { 0x2001 });
        }
        if rmw && matches!(kind, CycleKind::MemoryReadExtended { .. }) && state.pending() {
            assert_eq!(cpu.regs.f, flags);
        }
    }
    assert!(!state.pending());
    assert_eq!(cpu.regs.r, if prefix.is_some() { 0x81 } else { 0x80 });
    assert_eq!(
        cpu.last_m1_fetch_count,
        if prefix.is_some() { 2 } else { 1 }
    );
    assert_eq!(
        cpu.instruction_byte_count,
        if prefix.is_some() { 2 } else { 1 }
    );
    assert_eq!(
        cpu.instruction_bytes,
        [prefix.unwrap_or(opcode), prefix.map_or(0, |_| opcode), 0, 0]
    );
    if rmw {
        assert_eq!(cpu.regs.hl(), 0x8123);
        assert_eq!(cpu.regs.a, 0xa5);
        assert_eq!(cpu.regs.sp, u16::MAX);
        assert_eq!(cpu.regs.pc, if prefix.is_some() { 0x2002 } else { 0x2001 });
        if prefix.is_none() {
            assert_eq!(cpu.regs.f & Z80_FLAG_CARRY, flags & Z80_FLAG_CARRY);
        }
        if prefix.is_some() && opcode >> 6 >= 2 {
            assert_eq!(cpu.regs.f, flags);
        }
    } else {
        assert_eq!(cpu.regs.sp, 1);
        if ed_return || opcode == 0xc9 {
            assert_eq!(cpu.regs.pc, 0x1234);
        } else {
            assert_eq!(cpu.regs.pc, 0x2001);
            assert_eq!(cpu.read_stack_reg16((opcode >> 4) & 3), 0x1234);
        }
        if opcode != 0xf1 {
            assert_eq!(cpu.regs.f, flags);
        }
    }
    assert_eq!(
        (cpu.interrupt_flip_flop_1, cpu.interrupt_flip_flop_2),
        if rmw { (true, true) } else { before_iff }
    );
}

fn legacy_interrupt_return(opcode: u8) {
    for iff2 in [false, true] {
        let mut cpu = Cpu::new();
        cpu.regs.sp = u16::MAX;
        cpu.regs.f = 0xa5;
        cpu.interrupt_flip_flop_1 = !iff2;
        cpu.interrupt_flip_flop_2 = iff2;
        let operation = stack::Stack::decode_ed(&cpu, opcode).unwrap();
        let mut bus = Bus {
            sample: sample(CycleKind::MemoryRead { address: u16::MAX }, 0x34),
            interrupts: CycleInterrupts::default(),
            calls: std::cell::Cell::new(0),
        };
        let operation = operation.step(&mut cpu, &mut bus).0.unwrap();
        assert_eq!(cpu.interrupt_flip_flop_1, !iff2);
        bus.sample = sample(CycleKind::MemoryRead { address: 0 }, 0x12);
        assert!(operation.step(&mut cpu, &mut bus).0.is_none());
        assert_eq!(cpu.regs.pc, 0x1234);
        assert_eq!(cpu.regs.sp, 1);
        assert_eq!(cpu.regs.f, 0xa5);
        assert_eq!(cpu.interrupt_flip_flop_1, iff2);
        assert_eq!(cpu.interrupt_flip_flop_2, iff2);
        assert_eq!(bus.calls.get(), 2);
    }
}
