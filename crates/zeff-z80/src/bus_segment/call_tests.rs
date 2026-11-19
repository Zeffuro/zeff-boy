use super::prefix_tests::{Bus, restored, sample, stale};
use super::*;

#[path = "call_tests/guards.rs"]
mod guards;

pub(super) fn physical_call_retirement() {
    let mut count = 0;
    for opcode in u8::MIN..=u8::MAX {
        if !stack::Stack::is_call_exchange(opcode) {
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
    assert_eq!(count, 30);
    guards::excluded();
}

fn m1(address: u16, refresh_address: u16) -> CycleKind {
    CycleKind::M1 {
        address,
        halted: false,
        refresh_address,
    }
}

fn selected(cpu: &Cpu, prefix: Option<u8>) -> u16 {
    match prefix {
        Some(Z80_PREFIX_DD) => cpu.regs.ix,
        Some(Z80_PREFIX_FD) => cpu.regs.iy,
        _ => cpu.regs.hl(),
    }
}

fn qualified(prefix: Option<u8>, opcode: u8, flags: u8, waits: u32) {
    let mut cpu = Cpu::new();
    cpu.regs.pc = u16::MAX;
    cpu.regs.sp = if opcode == 0xe3 { u16::MAX } else { 1 };
    cpu.regs.set_bc(0x1234);
    cpu.regs.set_de(0x5678);
    cpu.regs.set_hl(0x9abc);
    cpu.regs.ix = 0xcafe;
    cpu.regs.iy = 0x1357;
    cpu.regs.a = 0x80;
    cpu.regs.f = flags;
    cpu.regs.i = 0x37;
    cpu.regs.r = 0xff;
    cpu.enable_interrupts_delay = 1;
    let before = cpu.clone();
    let exchange = opcode == 0xe3;
    let taken = !exchange && (opcode == 0xcd || cpu.condition_is_true((opcode >> 3) & 7));
    let terminal = u16::from(prefix.is_some());
    let sequential = terminal + if exchange { 0 } else { 2 };
    let original = selected(&cpu, prefix);
    let mut phases = vec![(m1(u16::MAX, 0x37ff), prefix.unwrap_or(opcode), 4)];
    if prefix.is_some() {
        phases.push((m1(0, 0x3780), opcode, 4));
    }
    if exchange {
        let [low, high] = original.to_le_bytes();
        phases.extend([
            (CycleKind::MemoryRead { address: u16::MAX }, 0x34, 3),
            (CycleKind::MemoryReadExtended { address: 0 }, 0x12, 4),
            (
                CycleKind::MemoryWrite {
                    address: 0,
                    value: high,
                },
                high,
                3,
            ),
            (
                CycleKind::MemoryWriteExtended {
                    address: u16::MAX,
                    value: low,
                },
                low,
                5,
            ),
        ]);
    } else {
        phases.push((CycleKind::MemoryRead { address: terminal }, 0x78, 3));
        phases.push((
            if taken {
                CycleKind::MemoryReadExtended {
                    address: terminal + 1,
                }
            } else {
                CycleKind::MemoryRead {
                    address: terminal + 1,
                }
            },
            0x56,
            if taken { 4 } else { 3 },
        ));
        if taken {
            let [low, high] = sequential.to_le_bytes();
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
        }
    }
    let phase_count = phases.len();
    let mut state = BusState::default();
    let mut legacy = cpu.clone();
    let mut legacy_state = state;
    let mut total = 0;
    let mut future = restored(&cpu, &state);
    for (index, (kind, value, base)) in phases.into_iter().enumerate() {
        let inputs = CycleInterrupts {
            maskable_pending: index != 0,
            nmi_pending: index != 0,
        };
        let saved = cpu.clone();
        let saved_state = state;
        let request = cpu.next_physical_cycle(&state).unwrap();
        assert_eq!(request.kind(), kind);
        assert_eq!(
            kind.t_states(),
            if kind.is_m1() { None } else { Some(base) }
        );
        assert_eq!(kind.minimum_t_states(), base);
        let receipt = sample(kind, value);
        guards::sample_checks(&cpu, &state, receipt, inputs, waits);
        guards::pending(&cpu, state, prefix, opcode);
        guards::overflow(&cpu, state, receipt);
        stale(&cpu, &state, receipt, inputs);
        let completion = request.complete(receipt, waits, inputs).unwrap();
        assert_eq!(completion.t_states(), base);
        assert_eq!(completion.wait_states(), waits);
        assert_eq!(cpu, saved);
        assert_eq!(state, saved_state);
        let classifier = legacy_state.next_memory_cycle();
        if !kind.is_m1() {
            assert_eq!(
                classifier,
                (base == 3 || (!exchange && base == 4)).then_some(3)
            );
        }
        let mut bus = Bus {
            sample: receipt,
            interrupts: inputs,
            calls: std::cell::Cell::new(0),
        };
        let mut expected = legacy
            .step_bus_segment(&mut bus, &mut legacy_state)
            .unwrap();
        if !exchange && taken && matches!(kind, CycleKind::MemoryReadExtended { .. }) {
            assert_eq!(expected.instruction.cycles, 3);
            let prepare = legacy
                .step_bus_segment(&mut bus, &mut legacy_state)
                .unwrap();
            assert_eq!(prepare.instruction.cycles, 1);
            assert!(!prepare.complete);
            expected.instruction.cycles += 1;
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
        let completion = future
            .0
            .next_physical_cycle(&future.1)
            .unwrap()
            .complete(receipt, waits, inputs)
            .unwrap();
        assert_eq!(
            future
                .0
                .retire_physical_cycle(&mut future.1, completion)
                .unwrap(),
            expected
        );
        assert_eq!(future, (cpu.clone(), state));
        future = restored(&future.0, &future.1);
        total += u64::from(base + waits);
        assert_eq!(cpu.cycles(), total);
        assert_eq!(cpu.regs.f, flags);
        assert_eq!(cpu.last_opcode, prefix.unwrap_or(opcode));
        assert_eq!(cpu.last_opcode_pc, u16::MAX);
        assert_eq!(
            cpu.enable_interrupts_delay,
            u8::from(index + 1 != phase_count)
        );
        if exchange {
            assert_eq!(cpu.regs.sp, u16::MAX);
            assert_eq!(
                selected(&cpu, prefix),
                if expected.complete { 0x1234 } else { original }
            );
        }
    }
    assert!(!state.pending());
    assert!(cpu.interrupt_flip_flop_1 && cpu.interrupt_flip_flop_2);
    assert_eq!(cpu.last_m1_fetch_count, 1 + u8::from(prefix.is_some()));
    assert_eq!(cpu.regs.r, 0x80 + u8::from(prefix.is_some()));
    assert_eq!(
        cpu.instruction_byte_count,
        1 + u8::from(prefix.is_some()) + if exchange { 0 } else { 2 }
    );
    assert_eq!(cpu.regs.pc, if taken { 0x5678 } else { sequential });
    assert_eq!(cpu.regs.sp, if exchange || taken { u16::MAX } else { 1 });
    if prefix.is_some() {
        assert_eq!(cpu.regs.hl(), before.regs.hl());
    }
    assert_eq!(cpu.regs.bc(), before.regs.bc());
    assert_eq!(cpu.regs.de(), before.regs.de());
}
