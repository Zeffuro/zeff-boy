#[path = "../multi_load/support.rs"]
mod support;

use support::{Access, Bus, phase, seeded};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::{BusState, Cpu, FetchedInstruction, PrefixState, ResetState};

fn inspection(cpu: &Cpu) -> impl std::fmt::Debug + PartialEq + use<> {
    (
        cpu.regs(),
        (cpu.state(), cpu.interrupt_mode()),
        (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
        (cpu.last_opcode_pc(), cpu.last_opcode()),
        (cpu.last_step_was_interrupt(), cpu.last_m1_fetch_count()),
        cpu.instruction_bytes().to_vec(),
        cpu.trap(),
    )
}

fn account(
    cpu: &mut Cpu,
    bus: &Bus,
    state: &BusState,
    report: &mut FetchedInstruction,
    waits: u32,
) {
    let before = inspection(cpu);
    let old_cycles = cpu.cycles();
    let old_report = *report;
    let saved_bus = bus.clone();
    let saved_state = *state;
    assert!(cpu.extend_bus_segment(report, waits));
    assert_eq!(cpu.cycles(), old_cycles + u64::from(waits));
    assert_eq!(report.cycles, old_report.cycles + waits);
    assert_eq!(
        (report.pc, report.opcode),
        (old_report.pc, old_report.opcode)
    );
    assert_eq!(inspection(cpu), before);
    assert_eq!(bus, &saved_bus);
    assert_eq!(*state, saved_state);
}

pub fn checked_segment_wait_counts() {
    for waits in [0, 2, u32::MAX - 4, u32::MAX - 3] {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new();
        let mut state = BusState::default();
        let mut segment = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert!(segment.complete);
        assert_eq!(segment.instruction.cycles, 4);
        assert_eq!(bus.trace.borrow().events[0], Some(Access::Read(0, 0)));
        let saved = (cpu.clone(), bus.clone(), state, segment);
        if waits == u32::MAX - 3 {
            assert!(!cpu.extend_bus_segment(&mut segment.instruction, waits));
            assert_eq!((cpu, bus, state, segment), saved);
        } else {
            account(&mut cpu, &bus, &state, &mut segment.instruction, waits);
            assert_eq!(cpu.cycles(), 4 + u64::from(waits));
            if waits == 0 {
                assert_eq!((cpu, bus, state, segment), saved);
            }
        }
    }
}

pub fn push_waits_preserve_owned_phases() {
    for delayed_phase in [2, 3] {
        let (mut cpu, mut bus) = seeded(0x9c);
        let pc = cpu.regs().pc;
        let sp = cpu.regs().sp;
        bus.bytes[usize::from(pc)] = 0xe5;
        bus.bytes[usize::from(sp - 1)] = 0xa7;
        bus.bytes[usize::from(sp - 2)] = 0xb8;
        let initial_cycles = cpu.cycles();
        let mut state = BusState::default();
        let mut copied = None;
        let mut reports = 0;
        for (index, clocks) in [4, 1, 3, 3].into_iter().enumerate() {
            let mut segment = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(segment.instruction.cycles, clocks);
            assert_eq!(segment.complete, index == 3);
            assert_eq!(state.pending(), index != 3);
            if index == 1 {
                assert_eq!(bus.trace.borrow().count, 1);
                assert_eq!(cpu.regs().sp, sp - 1);
            }
            if index == delayed_phase {
                account(&mut cpu, &bus, &state, &mut segment.instruction, 2);
                assert_eq!(
                    bus.bytes[usize::from(sp - 1)],
                    if index == 2 { 0x34 } else { 0xc9 }
                );
                assert_eq!(
                    bus.bytes[usize::from(sp - 2)],
                    if index == 2 { 0xb8 } else { 0x56 }
                );
            }
            reports += segment.instruction.cycles;
            if index == 2 {
                bus.bytes[usize::from(pc)] = 0;
                bus.bytes[usize::from(sp - 1)] = 0xc9;
                copied = Some((cpu.clone(), bus.clone(), state));
            }
        }
        let (mut copy_cpu, mut copy_bus, mut copy_state) = copied.unwrap();
        let mut final_segment = copy_cpu
            .step_bus_segment(&mut copy_bus, &mut copy_state)
            .unwrap();
        if delayed_phase == 3 {
            account(
                &mut copy_cpu,
                &copy_bus,
                &copy_state,
                &mut final_segment.instruction,
                2,
            );
        }
        assert!(final_segment.complete);
        assert_eq!(
            (cpu.clone(), bus.clone(), state),
            (copy_cpu, copy_bus, copy_state)
        );
        assert_eq!(reports, 13);
        assert_eq!(cpu.cycles() - initial_cycles, 13);
        assert_eq!(
            (cpu.regs().pc, cpu.regs().sp, cpu.regs().hl()),
            (pc + 1, sp - 2, 0x3456)
        );
        assert_eq!(
            (
                bus.bytes[usize::from(sp - 1)],
                bus.bytes[usize::from(sp - 2)]
            ),
            (0xc9, 0x56)
        );
        assert_eq!(bus.trace.borrow().count, 3);
        assert_eq!(
            &bus.trace.borrow().events[..3],
            &[
                Some(Access::Read(pc, 0xe5)),
                Some(Access::Write(sp - 1, 0x34)),
                Some(Access::Write(sp - 2, 0x56))
            ]
        );
    }
}

pub fn accounting_preserves_ei_interrupt_halt_and_inspection() {
    for nmi in [false, true] {
        let mut cpu = Cpu::new_with_reset(ResetState::new(0x100, 0x8000));
        let mut bus = Bus::new();
        bus.bytes[0x100..0x103].copy_from_slice(&[0xfb, 0x76, 0]);
        let mut state = BusState::default();
        let mut ei = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert!(ei.complete);
        assert!(!cpu.interrupts_enabled());
        account(&mut cpu, &bus, &state, &mut ei.instruction, 9);
        assert!(!cpu.interrupts_enabled());
        let mut halt = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert!(halt.complete);
        assert!(cpu.is_halted());
        assert!(cpu.interrupts_enabled());
        account(&mut cpu, &bus, &state, &mut halt.instruction, 7);
        let mut idle = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert!(idle.complete);
        assert_eq!(idle.instruction.cycles, 4);
        assert!(cpu.is_halted());
        assert!(cpu.instruction_bytes().is_empty());
        account(&mut cpu, &bus, &state, &mut idle.instruction, 5);
        bus.clear_trace();
        bus.nmi = nmi;
        bus.irq = !nmi;
        let mut service = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        assert_eq!(service.instruction.cycles, if nmi { 5 } else { 7 });
        assert!(!service.complete);
        assert!(state.pending());
        assert!(cpu.last_step_was_interrupt());
        assert!(!cpu.is_halted());
        assert_eq!(bus.acknowledgements, u32::from(nmi));
        account(&mut cpu, &bus, &state, &mut service.instruction, 3);
        let polls = bus.polls.get();
        phase(&mut cpu, &mut bus, &mut state, 3, false);
        phase(&mut cpu, &mut bus, &mut state, 3, true);
        assert_eq!(bus.polls.get(), polls);
        assert_eq!(bus.acknowledgements, u32::from(nmi));
        assert_eq!(cpu.regs().pc, if nmi { 0x66 } else { 0x38 });
        assert_eq!(cpu.regs().sp, 0x7ffe);
    }
}

pub fn waits_keep_continuation_clone_reset_and_default_apis() {
    let (cpu, mut bus) = seeded(0x9c);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 4].copy_from_slice(&[0xe5, 0, 0x3e, 0x72]);
    let mut legacy = (cpu.clone(), bus.clone());
    let mut bounded = legacy.clone();
    let mut zero = legacy.clone();
    let mut state = BusState::default();
    let mut prefix = PrefixState::default();
    for expected_clocks in [11, 4, 7] {
        assert_eq!(
            legacy.0.step_with_bus(&mut legacy.1).unwrap().cycles,
            expected_clocks
        );
        let mut bounded_clocks = 0;
        loop {
            let segment = bounded.0.step_bounded(&mut bounded.1, &mut prefix).unwrap();
            bounded_clocks += segment.instruction.cycles;
            if segment.complete {
                break;
            }
        }
        let mut zero_clocks = 0;
        loop {
            let mut segment = zero.0.step_bus_segment(&mut zero.1, &mut state).unwrap();
            account(&mut zero.0, &zero.1, &state, &mut segment.instruction, 0);
            zero_clocks += segment.instruction.cycles;
            if segment.complete {
                break;
            }
        }
        assert_eq!(
            (bounded_clocks, zero_clocks),
            (expected_clocks, expected_clocks)
        );
        assert_eq!(bounded, legacy);
        assert_eq!(zero, legacy);
    }
    let mut delayed = (cpu, bus, BusState::default());
    let mut first = delayed
        .0
        .step_bus_segment(&mut delayed.1, &mut delayed.2)
        .unwrap();
    let mut copied = delayed.clone();
    let mut copy_report = first.instruction;
    account(
        &mut delayed.0,
        &delayed.1,
        &delayed.2,
        &mut first.instruction,
        3,
    );
    account(&mut copied.0, &copied.1, &copied.2, &mut copy_report, 8);
    for clocks in [1, 3, 3] {
        let a = delayed
            .0
            .step_bus_segment(&mut delayed.1, &mut delayed.2)
            .unwrap();
        let b = copied
            .0
            .step_bus_segment(&mut copied.1, &mut copied.2)
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.instruction.cycles, clocks);
    }
    assert!(!delayed.2.pending());
    assert!(!copied.2.pending());
    assert_eq!(inspection(&delayed.0), inspection(&copied.0));
    assert_eq!(delayed.1, copied.1);
    assert_eq!(copied.0.cycles(), delayed.0.cycles() + 5);
    let mut writer = StateWriter::new();
    delayed.0.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut decoded = Cpu::new_with_reset(ResetState::new(0x100, 0x9876));
    decoded.read_state(&mut StateReader::new(&bytes)).unwrap();
    let mut writer = StateWriter::new();
    decoded.write_state(&mut writer);
    assert_eq!(writer.into_bytes(), bytes);
    let mut decoded_bus = delayed.1.clone();
    let a = delayed
        .0
        .step_bus_segment(&mut delayed.1, &mut delayed.2)
        .unwrap();
    let b = decoded
        .step_bus_segment(&mut decoded_bus, &mut BusState::default())
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(
        (delayed.0.clone(), delayed.1.clone()),
        (decoded, decoded_bus)
    );
    let prior_memory = delayed.1.bytes.clone();
    delayed.0.reset();
    delayed.2 = BusState::default();
    assert_eq!(
        delayed.0,
        Cpu::new_with_reset(ResetState::new(0x100, 0x9876))
    );
    assert_eq!(delayed.1.bytes, prior_memory);
    delayed.1.bytes[0x100] = 0;
    delayed.1.clear_trace();
    phase(&mut delayed.0, &mut delayed.1, &mut delayed.2, 4, true);
    assert_eq!(delayed.0.cycles(), 4);
    assert_eq!(delayed.0.regs().pc, 0x101);
}
