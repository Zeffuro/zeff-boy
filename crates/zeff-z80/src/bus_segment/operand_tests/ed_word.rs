use super::super::prefix_tests::{Bus, restored, sample};
use super::*;
use std::cell::Cell;

pub(super) fn retirement() {
    for opcode in [0x42, 0x52, 0x62, 0x72, 0x4a, 0x5a, 0x6a, 0x7a] {
        for initial_r in [0x7e, 0xfe, 0xff] {
            for carry in [0, Z80_FLAG_CARRY] {
                let (mut cpu, mut state) = start(opcode, initial_r, carry);
                let mut expected = (cpu.clone(), state);
                let initial = cpu.clone();
                for phase in [EdWordPhase::Internal4, EdWordPhase::Internal3] {
                    let (mut resumed, mut resumed_state) = restored(&cpu, &state);
                    let request = cpu.next_ed_word_cycle(&state).unwrap();
                    let kind = request.kind();
                    assert_eq!(kind.phase, phase);
                    let terminal_r = (initial_r & 0x80) | (initial_r.wrapping_add(1) & 0x7f);
                    assert_eq!(
                        kind.retained_address,
                        u16::from_be_bytes([0x5a, terminal_r])
                    );
                    assert_eq!(kind.t_states(), if kind.finishes() { 3 } else { 4 });
                    assert!(matches!(
                        cpu.next_ed_word_cycle(&state).unwrap().complete(1),
                        Err(CycleError::SampleMismatch)
                    ));
                    reject_stale(&cpu, state);
                    reject_context(&cpu, state);
                    reject_overflow(&cpu, state, kind.t_states());
                    let mut bus = Bus {
                        sample: sample(CycleKind::MemoryRead { address: 0xdead }, 0xa5),
                        interrupts: CycleInterrupts {
                            maskable_pending: true,
                            nmi_pending: true,
                        },
                        calls: Cell::new(0),
                    };
                    let default = expected
                        .0
                        .step_bus_segment(&mut bus, &mut expected.1)
                        .unwrap();
                    assert_eq!(bus.calls.get(), 0);
                    let completion = request.complete(0).unwrap();
                    let typed = cpu.retire_ed_word_cycle(&mut state, completion).unwrap();
                    let completion = resumed
                        .next_ed_word_cycle(&resumed_state)
                        .unwrap()
                        .complete(0)
                        .unwrap();
                    assert_eq!(
                        resumed
                            .retire_ed_word_cycle(&mut resumed_state, completion)
                            .unwrap(),
                        typed
                    );
                    assert_eq!(typed, default);
                    assert_eq!((resumed, resumed_state), (cpu.clone(), state));
                    assert_eq!((cpu.clone(), state), expected);
                    assert_eq!(cpu.regs.r, initial.regs.r);
                    assert_eq!(cpu.regs.pc, 1);
                    if phase == EdWordPhase::Internal4 {
                        assert_eq!(cpu.regs, initial.regs);
                        assert_eq!(cpu.memptr(), Some(0x2800));
                        assert_eq!(cpu.enable_interrupts_delay, 1);
                    } else {
                        assert_eq!(cpu.memptr(), Some(initial.regs.hl().wrapping_add(1)));
                        assert_eq!(cpu.enable_interrupts_delay, 0);
                        assert!(cpu.interrupt_flip_flop_1 && cpu.interrupt_flip_flop_2);
                        assert!(matches!(
                            cpu.next_physical_cycle(&state)
                                .unwrap()
                                .validate_interrupts(bus.interrupts),
                            Err(CycleError::UnsupportedInterrupt)
                        ));
                    }
                }
            }
        }
    }
}

fn start(opcode: u8, initial_r: u8, carry: u8) -> (Cpu, BusState) {
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0, 0x8000), MemptrProfile::Nmos);
    let mut state = BusState::default();
    let mut bus = Bus {
        sample: sample(CycleKind::MemoryRead { address: 0 }, 0),
        interrupts: CycleInterrupts::default(),
        calls: Cell::new(0),
    };
    for (address, value) in [(0, 0x3a), (1, 0xff), (2, 0x27), (0x27ff, 0)] {
        bus.sample = sample(CycleKind::MemoryRead { address }, value);
        cpu.step_bus_segment(&mut bus, &mut state).unwrap();
    }
    assert_eq!(cpu.memptr(), Some(0x2800));
    cpu.regs.pc = 0xffff;
    cpu.regs.i = 0x5a;
    cpu.regs.r = initial_r;
    cpu.regs.f = carry;
    cpu.regs.set_bc(0x1234);
    cpu.regs.set_de(0x89ab);
    cpu.regs.set_hl(0x7fff);
    cpu.regs.sp = 0x8000;
    cpu.interrupt_mode = InterruptMode::Im1;
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    cpu.enable_interrupts_delay = 1;
    for (address, value) in [(0xffff, Z80_PREFIX_ED), (0, opcode)] {
        bus.sample = sample(CycleKind::MemoryRead { address }, value);
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    }
    (cpu, state)
}

fn reject_stale(cpu: &Cpu, state: BusState) {
    for variant in 0..4 {
        let completion = cpu.next_ed_word_cycle(&state).unwrap().complete(0).unwrap();
        let mut changed = cpu.clone();
        let mut changed_state = state;
        match variant {
            0 => changed.regs.f ^= Z80_FLAG_CARRY,
            1 => changed.memptr = Some(cpu.memptr().unwrap() ^ 0x2000),
            2 => changed.memptr_profile = MemptrProfile::Legacy,
            _ => changed_state.transfer = None,
        }
        let before = (changed.clone(), changed_state);
        assert!(matches!(
            changed.retire_ed_word_cycle(&mut changed_state, completion),
            Err(CycleError::StaleContinuation)
        ));
        assert_eq!((changed, changed_state), before);
    }
}

fn reject_context(cpu: &Cpu, state: BusState) {
    for variant in 0..13 {
        let mut changed = cpu.clone();
        let mut changed_state = state;
        match variant {
            0 => changed.instruction_bytes[0] = Z80_PREFIX_DD,
            1 => changed.instruction_bytes[1] ^= 0x10,
            2 => changed.instruction_bytes[2] = 1,
            3 => changed.instruction_byte_count = 3,
            4 => changed.last_m1_fetch_count = 3,
            5 => changed.last_opcode = Z80_PREFIX_DD,
            6 => changed.last_opcode_pc ^= 1,
            7 => changed.regs.pc = changed.regs.pc.wrapping_add(1),
            8 => changed_state.prefix = PrefixState::index(cpu.last_opcode_pc, Z80_PREFIX_DD),
            9 => changed_state.transfer.as_mut().unwrap().pc ^= 1,
            10 => changed.state = CpuState::Suspended,
            11 => changed.last_step_was_interrupt = true,
            _ => changed.enable_interrupts_delay = 3,
        }
        let before = (changed.clone(), changed_state);
        assert!(changed.next_ed_word_cycle(&changed_state).is_err());
        assert_eq!((changed, changed_state), before);
    }
    for opcode in [0x47, 0x4f, 0x57, 0x5f] {
        let mut changed = cpu.clone();
        changed.instruction_bytes[1] = opcode;
        let mut changed_state = state;
        changed_state.transfer.as_mut().unwrap().operation =
            Operation::EdInternal(ed::Internal::decode(opcode).unwrap());
        assert!(matches!(
            changed.next_ed_word_cycle(&changed_state),
            Err(CycleError::UnsupportedCycle)
        ));
    }
}

fn reject_overflow(cpu: &Cpu, state: BusState, clocks: u32) {
    let mut changed = cpu.clone();
    changed.cycles = u64::MAX - u64::from(clocks) + 1;
    let before = (changed.clone(), state);
    assert!(matches!(
        changed.next_ed_word_cycle(&state).unwrap().complete(0),
        Err(CycleError::ClockOverflow)
    ));
    assert_eq!((changed.clone(), state), before);
    changed.cycles = u64::MAX - u64::from(clocks);
    let mut changed_state = state;
    let completion = changed
        .next_ed_word_cycle(&state)
        .unwrap()
        .complete(0)
        .unwrap();
    changed
        .retire_ed_word_cycle(&mut changed_state, completion)
        .unwrap();
    assert_eq!(changed.cycles, u64::MAX);
}
