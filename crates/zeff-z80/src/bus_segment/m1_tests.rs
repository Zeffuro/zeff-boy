use super::prefix_tests::{Bus, restored, sample, stale};
use super::*;

#[path = "m1_tests/guards.rs"]
mod guards;

pub(super) fn physical_m1_retirement() {
    let mut count = 0;
    for opcode in u8::MIN..=u8::MAX {
        if !m1_register::admitted(opcode) && !stack::Stack::prepares(opcode) {
            continue;
        }
        for prefix in [None, Some(Z80_PREFIX_DD), Some(Z80_PREFIX_FD)] {
            count += 1;
            for flags in [0, 0xff] {
                for waits in [0, 2] {
                    qualified(prefix, opcode, flags, waits);
                }
            }
        }
    }
    assert_eq!(count, 87);
    guards::selector();
}

fn m1(address: u16, refresh_address: u16) -> CycleKind {
    CycleKind::M1 {
        address,
        halted: false,
        refresh_address,
    }
}

fn pair(cpu: &Cpu, register: u8, prefix: Option<u8>) -> u16 {
    match (register, prefix) {
        (2, Some(Z80_PREFIX_DD)) => cpu.regs.ix,
        (2, Some(Z80_PREFIX_FD)) => cpu.regs.iy,
        _ => cpu.read_reg16(register),
    }
}

fn qualified(prefix: Option<u8>, opcode: u8, flags: u8, waits: u32) {
    let mut cpu = Cpu::new();
    cpu.regs.pc = u16::MAX;
    cpu.regs.sp = u16::MAX;
    cpu.regs.set_bc(0x1234);
    cpu.regs.set_de(0x5678);
    cpu.regs.set_hl(0x9abc);
    cpu.regs.ix = 0xcafe;
    cpu.regs.iy = 0xbeef;
    cpu.regs.a = 0x80;
    cpu.regs.f = flags;
    cpu.regs.i = 0x37;
    cpu.regs.r = 0xff;
    cpu.enable_interrupts_delay = 1;
    let six = m1_register::admitted(opcode);
    let conditional = opcode & 0xc7 == 0xc0;
    let restart = opcode & 0xc7 == 0xc7;
    let push = !six && !conditional;
    if push {
        cpu.regs.sp = 1;
    }
    let register = if opcode == 0xf9 { 2 } else { (opcode >> 4) & 3 };
    if six && opcode != 0xf9 {
        let value = if flags == 0 { u16::MAX } else { 0 };
        match (register, prefix) {
            (2, Some(Z80_PREFIX_DD)) => cpu.regs.ix = value,
            (2, Some(Z80_PREFIX_FD)) => cpu.regs.iy = value,
            _ => cpu.write_reg16(register, value),
        }
    }
    let before = cpu.clone();
    let sequential = u16::from(prefix.is_some());
    let taken = conditional && cpu.condition_is_true((opcode >> 3) & 7);
    let mut phases = vec![(
        m1(u16::MAX, 0x37ff),
        prefix.unwrap_or(opcode),
        if prefix.is_some() {
            4
        } else if six {
            6
        } else {
            5
        },
    )];
    if let Some(prefix) = prefix {
        assert!(matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD));
        phases.push((m1(0, 0x3780), opcode, if six { 6 } else { 5 }));
    }
    if push {
        let word = if restart {
            sequential
        } else if register == 2 && prefix.is_some() {
            pair(&cpu, register, prefix)
        } else {
            cpu.read_stack_reg16(register)
        };
        let [low, high] = word.to_le_bytes();
        phases.extend([
            (
                CycleKind::MemoryWrite {
                    address: 0,
                    value: high,
                },
                high,
                3,
            ),
            (
                CycleKind::MemoryWrite {
                    address: u16::MAX,
                    value: low,
                },
                low,
                3,
            ),
        ]);
    } else if taken {
        phases.extend([
            (CycleKind::MemoryRead { address: u16::MAX }, 0x34, 3),
            (CycleKind::MemoryRead { address: 0 }, 0x12, 3),
        ]);
    }
    let phase_count = phases.len();
    let mut state = BusState::default();
    let mut legacy = cpu.clone();
    let mut legacy_state = state;
    let mut total = 0;
    for (index, (kind, value, base)) in phases.into_iter().enumerate() {
        let inputs = CycleInterrupts {
            maskable_pending: index != 0,
            nmi_pending: index != 0,
        };
        let (mut clone, mut clone_state) = restored(&cpu, &state);
        let saved = cpu.clone();
        let saved_state = state;
        let request = cpu.next_physical_cycle(&state).unwrap();
        assert_eq!(request.kind(), kind);
        assert_eq!(kind.t_states(), if kind.is_m1() { None } else { Some(3) });
        assert_eq!(kind.minimum_t_states(), if kind.is_m1() { 4 } else { 3 });
        let sample = sample(kind, value);
        assert_eq!(request.sampled_t_states(sample), Ok(base));
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
            assert_eq!(request.sampled_t_states(bad), Err(error));
            assert!(
                matches!(cpu.next_physical_cycle(&state).unwrap().complete(bad, waits, inputs), Err(actual) if actual == error)
            );
        }
        if kind.is_write() {
            let bad = CycleSample {
                value: value ^ 1,
                ..sample
            };
            assert_eq!(
                request.sampled_t_states(bad),
                Err(CycleError::SampleMismatch)
            );
        }
        guards::pending(&cpu, state, prefix, opcode);
        stale(&cpu, &state, sample, inputs);
        let completion = request.complete(sample, waits, inputs).unwrap();
        assert_eq!(completion.t_states(), base);
        assert_eq!(completion.wait_states(), waits);
        assert_eq!(cpu, saved);
        assert_eq!(state, saved_state);
        let mut bus = Bus {
            sample,
            interrupts: inputs,
            calls: std::cell::Cell::new(0),
        };
        let mut expected = legacy
            .step_bus_segment(&mut bus, &mut legacy_state)
            .unwrap();
        if base == 5 {
            assert_eq!(expected.instruction.cycles, 4);
            assert!(!expected.complete);
            let prepare = legacy
                .step_bus_segment(&mut bus, &mut legacy_state)
                .unwrap();
            assert_eq!(prepare.instruction.cycles, 1);
            expected.instruction.cycles += prepare.instruction.cycles;
            expected.complete = prepare.complete;
        }
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
        assert_eq!(cpu.regs.f, flags);
        assert_eq!(
            cpu.enable_interrupts_delay,
            u8::from(index + 1 != phase_count)
        );
        assert_eq!(cpu.last_opcode, prefix.unwrap_or(opcode));
        assert_eq!(cpu.last_opcode_pc, u16::MAX);
        if index + 1 != phase_count && kind.is_m1() && base == 5 {
            assert_eq!(cpu.regs.sp, if push { 0 } else { u16::MAX });
        }
        if push && index + 1 != phase_count {
            assert_eq!(
                cpu.regs.pc,
                if prefix.is_some() && index == 0 {
                    0
                } else {
                    sequential
                }
            );
        }
    }
    restored(&cpu, &state);
    assert!(!state.pending());
    assert!(cpu.interrupt_flip_flop_1 && cpu.interrupt_flip_flop_2);
    assert_eq!(cpu.regs.r, if prefix.is_some() { 0x81 } else { 0x80 });
    assert_eq!(
        cpu.last_m1_fetch_count,
        if prefix.is_some() { 2 } else { 1 }
    );
    assert_eq!(
        cpu.instruction_byte_count,
        if prefix.is_some() { 2 } else { 1 }
    );
    if six {
        let old = pair(&before, register, prefix);
        if opcode == 0xf9 {
            assert_eq!(cpu.regs.sp, old);
        } else {
            assert_eq!(
                pair(&cpu, register, prefix),
                if opcode & 8 == 0 {
                    old.wrapping_add(1)
                } else {
                    old.wrapping_sub(1)
                }
            );
        }
        if prefix.is_some() && register == 2 {
            assert_eq!(cpu.regs.hl(), before.regs.hl());
        }
        assert_eq!(cpu.regs.pc, sequential);
    } else if push {
        assert_eq!(cpu.regs.sp, u16::MAX);
        assert_eq!(
            cpu.regs.pc,
            if restart {
                u16::from(opcode & 0x38)
            } else {
                sequential
            }
        );
    } else {
        assert_eq!(cpu.regs.sp, if taken { 1 } else { u16::MAX });
        assert_eq!(cpu.regs.pc, if taken { 0x1234 } else { sequential });
    }
}
