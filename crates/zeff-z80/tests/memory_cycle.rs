#[allow(dead_code)]
#[path = "multi_load/support.rs"]
mod support;

use zeff_z80::BusState;

pub fn memory_cycle_classification() {
    for (program, phases) in [
        (&[0x06, 0x55][..], &[false, true][..]),
        (&[0x21, 0x34, 0x12][..], &[false, true, true][..]),
        (&[0x22, 0, 0x80][..], &[false, true, true, true, true][..]),
        (&[0x2a, 0, 0x80][..], &[false, true, true, true, true][..]),
        (&[0x32, 0, 0x80][..], &[false, true, true, true][..]),
        (&[0x3a, 0, 0x80][..], &[false, true, true, true][..]),
        (&[0x36, 0x55][..], &[false, true, true][..]),
        (
            &[0xdd, 0x21, 0x34, 0x12][..],
            &[false, false, true, true][..],
        ),
        (
            &[0xfd, 0x22, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (
            &[0xed, 0x43, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (
            &[0xed, 0x4b, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (&[0xe5][..], &[false, false, true, true][..]),
        (&[0xe1][..], &[false, true, true][..]),
        (&[0xc3, 0x34, 0x12][..], &[false, true, true][..]),
        (
            &[0xcd, 0x34, 0x12][..],
            &[false, true, true, false, true, true][..],
        ),
        (&[0xc9][..], &[false, true, true][..]),
        (&[0xcf][..], &[false, false, true, true][..]),
        (&[0xe3][..], &[false, true, false, true, false][..]),
        (&[0xdd, 0xe5][..], &[false, false, false, true, true][..]),
        (&[0xfd, 0xe1][..], &[false, false, true, true][..]),
        (&[0xed, 0x45][..], &[false, false, false, false][..]),
        (&[0x77][..], &[false, false][..]),
    ] {
        let (mut cpu, mut bus) = support::seeded(program[0]);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + program.len()].copy_from_slice(program);
        let mut state = BusState::default();
        for &memory in phases {
            let classified = state.next_memory_cycle();
            assert_eq!(classified, memory.then_some(3));
            bus.clear_trace();
            let segment = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            if memory {
                assert_eq!(segment.instruction.cycles, 3);
                assert_eq!(bus.trace.borrow().count, 1);
            }
        }
        assert!(!state.pending());
        assert_eq!(state.next_memory_cycle(), None);
    }
}

#[test]
fn next_memory_cycle_classifies_owned_memory_phases() {
    memory_cycle_classification();
}

pub fn memory_cycle_excludes_interrupt_origin() {
    for nmi in [false, true] {
        let mut cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x100, 0x8000));
        let mut bus = support::Bus::new();
        bus.bytes[0x100..0x102].copy_from_slice(&[0xfb, 0x76]);
        let mut state = BusState::default();
        for _ in 0..2 {
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        }
        bus.nmi = nmi;
        bus.irq = !nmi;
        for (index, clocks) in [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate() {
            assert_eq!(state.next_memory_cycle(), None);
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, clocks);
            assert_eq!(step.complete, index == 2);
        }
        assert_eq!(cpu.regs().pc, if nmi { 0x66 } else { 0x38 });
        assert_eq!(cpu.regs().sp, 0x7ffe);
        assert_eq!(state.next_memory_cycle(), None);
    }
}

#[test]
fn interrupt_stack_phases_remain_unclassified() {
    memory_cycle_excludes_interrupt_origin();
}
