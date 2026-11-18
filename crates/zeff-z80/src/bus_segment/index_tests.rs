use super::prefix_tests::{Bus, restored, sample, stale};
use super::*;
use crate::index_cycle as indexed;

#[path = "index_tests/guards.rs"]
mod guards;

pub(super) fn physical_index_retirement() {
    for prefix in [Z80_PREFIX_DD, Z80_PREFIX_FD] {
        let mut count = 0;
        for opcode in u8::MIN..=u8::MAX {
            if m1_register::admitted(opcode)
                || stack::Stack::prepares(opcode)
                || stack::Stack::is_call_exchange(opcode)
            {
                continue;
            } else if indexed::admitted(opcode) {
                count += 1;
                for (flags, waits) in [(0, 0), (0xff, 2)] {
                    qualified(prefix, opcode, flags, waits);
                }
            } else {
                guards::unsupported(prefix, opcode);
            }
        }
        assert_eq!(count, 175);
        guards::repeated(prefix);
    }
}

fn m1(address: u16, refresh_address: u16) -> CycleKind {
    CycleKind::M1 {
        address,
        halted: false,
        refresh_address,
    }
}

fn read(address: u16, value: u8) -> (CycleKind, u8) {
    (CycleKind::MemoryRead { address }, value)
}

fn write(address: u16, value: u8) -> (CycleKind, u8) {
    (CycleKind::MemoryWrite { address, value }, value)
}

fn qualified(prefix: u8, opcode: u8, flags: u8, waits: u32) {
    let mut cpu = Cpu::new();
    cpu.regs.pc = u16::MAX;
    cpu.regs.sp = u16::MAX;
    cpu.regs.a = 0x80;
    cpu.regs.f = flags;
    cpu.regs.set_bc(0xcafe);
    cpu.regs.set_de(0xbeef);
    cpu.regs.set_hl(0x1357);
    cpu.regs.ix = 0x2468;
    cpu.regs.iy = 0x9abc;
    cpu.regs.i = 0x37;
    cpu.regs.r = 0xff;
    cpu.shadow = ShadowRegisters {
        a: 0x12,
        f: 0x34,
        b: 0x56,
        c: 0x78,
        d: 0x9a,
        e: 0xbc,
        h: 0xde,
        l: 0xf0,
    };
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    cpu.enable_interrupts_delay = 1;
    let selected = if prefix == Z80_PREFIX_FD {
        cpu.regs.iy
    } else {
        cpu.regs.ix
    };
    let mut phases = vec![(m1(u16::MAX, 0x37ff), prefix), (m1(0, 0x3780), opcode)];
    match opcode {
        0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e | 0xc6 | 0xce | 0xd6 | 0xde | 0xe6
        | 0xee | 0xf6 | 0xfe => {
            phases.push(read(1, 0x34));
        }
        0x01 | 0x11 | 0x21 | 0x31 => phases.extend([read(1, 0x34), read(2, 0x12)]),
        0x22 | 0x2a | 0x32 | 0x3a => {
            phases.extend([read(1, 0xff), read(2, 0xff)]);
            match opcode {
                0x22 => {
                    let [low, high] = selected.to_le_bytes();
                    phases.extend([write(u16::MAX, low), write(0, high)]);
                }
                0x2a => phases.extend([read(u16::MAX, 0x34), read(0, 0x12)]),
                0x32 => phases.push(write(u16::MAX, 0x80)),
                0x3a => phases.push(read(u16::MAX, 0x5a)),
                _ => unreachable!(),
            }
        }
        0x02 => phases.push(write(0xcafe, 0x80)),
        0x12 => phases.push(write(0xbeef, 0x80)),
        0x0a => phases.push(read(0xcafe, 0x5a)),
        0x1a => phases.push(read(0xbeef, 0x5a)),
        _ if stack::Stack::is_jump(opcode) => phases.extend([read(1, 0x34), read(2, 0x12)]),
        _ if stack::Stack::is_pop(opcode) => phases.extend([read(u16::MAX, 0x34), read(0, 0x12)]),
        _ => {}
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
        assert_eq!(kind.t_states(), if kind.is_m1() { None } else { Some(3) });
        assert_eq!(base, if kind.is_m1() { 4 } else { 3 });
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
        assert_eq!(cpu.last_opcode, prefix);
        assert_eq!(cpu.last_opcode_pc, u16::MAX);
        assert_eq!(cpu.last_m1_fetch_count, if index == 0 { 1 } else { 2 });
        assert_eq!(
            cpu.enable_interrupts_delay,
            u8::from(index + 1 != phase_count)
        );
        if index + 1 != phase_count {
            assert!(state.prefix_pending());
        }
    }
    assert!(!state.pending());
    assert_eq!(cpu.regs.r, 0x81);
    assert_eq!(cpu.interrupt_flip_flop_1, opcode != 0xf3);
    assert_eq!(cpu.interrupt_flip_flop_2, opcode != 0xf3);
    assert_eq!(cpu.is_halted(), opcode == 0x76);
    assert_eq!(cpu.instruction_bytes[0..2], [prefix, opcode]);
    match opcode {
        0xeb => {
            assert_eq!(cpu.regs.hl(), 0xbeef);
            assert_eq!(cpu.regs.de(), 0x1357);
            assert_eq!((cpu.regs.ix, cpu.regs.iy), (0x2468, 0x9abc));
        }
        0xe9 => assert_eq!(cpu.regs.pc, selected),
        0xe1 | 0x21 | 0x2a => {
            assert_eq!(
                if prefix == Z80_PREFIX_FD {
                    cpu.regs.iy
                } else {
                    cpu.regs.ix
                },
                0x1234
            );
            assert_eq!(
                if prefix == Z80_PREFIX_FD {
                    cpu.regs.ix
                } else {
                    cpu.regs.iy
                },
                if prefix == Z80_PREFIX_FD {
                    0x2468
                } else {
                    0x9abc
                }
            );
        }
        0x26 => assert_eq!(
            if prefix == Z80_PREFIX_FD {
                cpu.regs.iy
            } else {
                cpu.regs.ix
            },
            (selected & 0xff) | 0x3400
        ),
        0x2e => assert_eq!(
            if prefix == Z80_PREFIX_FD {
                cpu.regs.iy
            } else {
                cpu.regs.ix
            },
            (selected & 0xff00) | 0x34
        ),
        0xc9 => assert_eq!(cpu.regs.pc, 0x1234),
        _ => {}
    }
    if stack::Stack::is_pop(opcode) {
        assert_eq!(cpu.regs.sp, 1);
        if matches!(opcode, 0xc1 | 0xd1 | 0xf1) {
            assert_eq!(cpu.read_stack_reg16((opcode >> 4) & 3), 0x1234);
        }
    }
}
