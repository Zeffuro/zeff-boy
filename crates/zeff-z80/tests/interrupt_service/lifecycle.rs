use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub fn interrupt_pending_clone_suspend_reset() {
    let mut boundaries = 0;
    for service in SERVICES {
        for count in 1..service.clocks().len() {
            let (mut cpu, mut bus) = service_fixture(service, 0x1234, 1, 0xff, true);
            let mut state = BusState::default();
            for &clock in &service.clocks()[..count] {
                phase(&mut cpu, &mut bus, &mut state, clock, false);
            }
            cpu.suspend();
            let saved = (cpu.clone(), bus.clone(), state);
            assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
            assert_eq!((cpu.clone(), bus.clone(), state), saved);
            cpu.resume();
            bus.memory.irq = !bus.memory.irq;
            bus.memory.nmi = true;
            bus.byte = 0;
            let polls = bus.memory.polls.get();
            let acknowledgements = (bus.irq_acks, bus.memory.acknowledgements);
            let mut cloned = (cpu.clone(), bus.clone(), state);
            finish(&mut cpu, &mut bus, &mut state, service, count);
            finish(&mut cloned.0, &mut cloned.1, &mut cloned.2, service, count);
            assert_eq!(bus.memory.polls.get(), polls);
            assert_eq!(
                (bus.irq_acks, bus.memory.acknowledgements),
                acknowledgements
            );
            assert_eq!((cpu, bus, state), cloned);
            let (mut cpu, mut bus, _) = saved;
            let prior_memory = bus.memory.bytes.clone();
            cpu.reset();
            bus.memory.irq = false;
            bus.memory.nmi = false;
            bus.memory.bytes[0x100] = 0;
            bus.memory.clear_trace();
            let acks = (bus.irq_acks, bus.memory.acknowledgements);
            let mut reset_state = BusState::default();
            phase(&mut cpu, &mut bus, &mut reset_state, 4, true);
            assert_eq!(
                cpu.regs(),
                Registers {
                    pc: 0x101,
                    sp: 0x9876,
                    r: 1,
                    ..Registers::default()
                }
            );
            assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);
            assert_eq!(trace(&bus), vec![Some(Access::Read(0x100, 0))]);
            assert_eq!((bus.irq_acks, bus.memory.acknowledgements), acks);
            let mut expected_memory = prior_memory;
            expected_memory[0x100] = 0;
            assert_eq!(bus.memory.bytes, expected_memory);
            boundaries += 1;
        }
    }
    assert_eq!(boundaries, 24);
}

pub fn interrupt_priority_iff_and_ei_boundaries() {
    for (iff1, iff2) in [(false, false), (false, true), (true, false), (true, true)] {
        let (mut cpu, mut bus) = service_fixture(Service::Nmi, 0x4000, 0x8000, 0x7f, iff2);
        cpu.finish_guest_call(iff1, iff2, 0);
        cpu.resume();
        bus.nmi_result = false;
        let mut state = BusState::default();
        finish(&mut cpu, &mut bus, &mut state, Service::Nmi, 0);
        assert_eq!((cpu.regs().pc, cpu.regs().r), (0x66, 0));
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, iff2)
        );
        assert_eq!(
            (
                bus.irq_acks,
                bus.memory.acknowledgements,
                bus.memory.polls.get()
            ),
            (0, 1, 1)
        );
        bus.memory.nmi = true;
        finish(&mut cpu, &mut bus, &mut state, Service::Nmi, 0);
        assert_eq!(
            (cpu.regs().pc, cpu.regs().sp, cpu.regs().r),
            (0x66, 0x7ffc, 1)
        );
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, iff2)
        );
        assert_eq!(
            (
                bus.irq_acks,
                bus.memory.acknowledgements,
                bus.memory.polls.get()
            ),
            (0, 2, 2)
        );
        bus.memory.clear_trace();
        phase(&mut cpu, &mut bus, &mut state, 4, true);
        assert_eq!(cpu.regs().pc, 0x67);
        assert!(!cpu.last_step_was_interrupt());
        assert_eq!(bus.irq_acks, 0);

        let (mut cpu, mut bus) = fixture(0x56, 0xaa, 0x4000, 0x8000, 0xff, 0x12);
        cpu.finish_guest_call(iff1, iff2, 0);
        cpu.resume();
        bus.memory.irq = false;
        bus.memory.bytes[0x4000..0x4004].copy_from_slice(&[0xfb, 0xdd, 0xfd, 0]);
        cpu.step_with_bus(&mut bus).unwrap();
        bus.memory.clear_trace();
        bus.memory.irq = true;
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 4, false);
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (iff1, iff2)
        );
        let polls = bus.memory.polls.get();
        phase(&mut cpu, &mut bus, &mut state, 4, false);
        phase(&mut cpu, &mut bus, &mut state, 4, true);
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (true, true)
        );
        assert_eq!(bus.memory.polls.get(), polls);
        assert_eq!(bus.irq_acks, 0);
        assert_eq!(cpu.regs().pc, 0x4004);
        assert!(!cpu.last_step_was_interrupt());
        finish(&mut cpu, &mut bus, &mut state, Service::Im1, 0);
        assert_eq!((cpu.regs().pc, cpu.regs().sp), (0x38, 0x7ffe));
        assert_eq!(bus.memory.bytes[0x7ffe], 4);
        assert_eq!(bus.irq_acks, 1);
    }
    for next in [0xf3, 0xfb] {
        let (mut cpu, mut bus) = fixture(0x56, 0xaa, 0x4000, 0x8000, 0, 0x12);
        cpu.finish_guest_call(false, false, 0);
        cpu.resume();
        bus.memory.irq = false;
        bus.memory.bytes[0x4000..0x4003].copy_from_slice(&[0xfb, next, 0]);
        cpu.step_with_bus(&mut bus).unwrap();
        bus.memory.irq = true;
        cpu.step_with_bus(&mut bus).unwrap();
        assert!(!cpu.interrupts_enabled());
        assert_eq!(bus.irq_acks, 0);
        cpu.step_with_bus(&mut bus).unwrap();
        assert_eq!(cpu.interrupts_enabled(), next == 0xfb);
        assert_eq!(bus.irq_acks, 0);
    }
}

fn codec(cpu: &Cpu) {
    let mut writer = StateWriter::new();
    cpu.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut decoded = Cpu::new_with_reset(ResetState::new(0x100, 0x9876));
    let mut reader = StateReader::new(&bytes);
    decoded.read_state(&mut reader).unwrap();
    assert_eq!(decoded.regs(), cpu.regs());
    assert_eq!(
        (decoded.state(), decoded.interrupt_mode()),
        (cpu.state(), cpu.interrupt_mode())
    );
    assert_eq!(
        (
            decoded.interrupts_enabled(),
            decoded.saved_interrupts_enabled()
        ),
        (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled())
    );
    assert_eq!(
        (
            decoded.cycles(),
            decoded.last_opcode_pc(),
            decoded.last_opcode(),
            decoded.trap()
        ),
        (
            cpu.cycles(),
            cpu.last_opcode_pc(),
            cpu.last_opcode(),
            cpu.trap()
        )
    );
    let mut writer = StateWriter::new();
    decoded.write_state(&mut writer);
    assert_eq!(writer.into_bytes(), bytes);
}

fn single(cpu: &mut Cpu, bus: &mut IrqBus, api: u8, clocks: u32) {
    let instruction = match api {
        0 => cpu.step_with_bus(bus).unwrap(),
        1 => {
            let step = cpu.step_bounded(bus, &mut PrefixState::default()).unwrap();
            assert!(step.complete);
            step.instruction
        }
        _ => {
            let step = cpu.step_bus_segment(bus, &mut BusState::default()).unwrap();
            assert!(step.complete);
            step.instruction
        }
    };
    assert_eq!(instruction.cycles, clocks);
}

fn halt(api: u8, enabled: bool, nmi: bool) {
    let (mut cpu, mut bus) = fixture(0x56, 0xaa, 0xffff, 0x8000, 0xff, 0x12);
    cpu.finish_guest_call(enabled, true, 0);
    cpu.resume();
    bus.memory.irq = false;
    bus.memory.bytes[0xffff] = 0x76;
    bus.memory.bytes[0] = 0xdd;
    single(&mut cpu, &mut bus, api, 4);
    let mut expected = cpu.regs();
    assert_eq!((expected.pc, expected.r), (0, 0x80));
    assert!(cpu.is_halted());
    bus.memory.clear_trace();
    bus.memory.irq = !enabled;
    for r in [0x81, 0x82, 0x83] {
        single(&mut cpu, &mut bus, api, 4);
        expected.r = r;
        assert_eq!(cpu.regs(), expected);
        assert!(cpu.is_halted());
        assert!(!cpu.last_step_was_interrupt());
        assert_eq!(cpu.last_m1_fetch_count(), 1);
        assert!(cpu.instruction_bytes().is_empty());
    }
    assert_eq!(trace(&bus), vec![Some(Access::Read(0, 0xdd)); 3]);
    assert_eq!(bus.irq_acks, 0);
    if enabled || nmi {
        bus.memory.irq = true;
        bus.memory.nmi = nmi;
        bus.memory.clear_trace();
        let service = if nmi { Service::Nmi } else { Service::Im1 };
        if api == 2 {
            finish(&mut cpu, &mut bus, &mut BusState::default(), service, 0);
        } else {
            single(&mut cpu, &mut bus, api, if nmi { 11 } else { 13 });
        }
        expected.pc = service.target();
        expected.sp = 0x7ffe;
        expected.r = 0x84;
        assert_eq!(cpu.regs(), expected);
        assert_eq!(cpu.state(), CpuState::Running);
        assert_eq!((bus.memory.bytes[0x7fff], bus.memory.bytes[0x7ffe]), (0, 0));
        metadata(&cpu, 0);
    }
    cpu.reset();
    assert!(!cpu.is_halted());
    assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);
}

pub fn interrupt_reset_halt_and_im0_guards() {
    for reset in [ResetState::default(), ResetState::new(0x2345, 0x6789)] {
        let cpu = Cpu::new_with_reset(reset);
        assert_eq!(
            cpu.regs(),
            Registers {
                pc: reset.pc,
                sp: reset.sp,
                ..Registers::default()
            }
        );
        assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, false)
        );
        codec(&cpu);
    }
    assert_eq!(Cpu::new().interrupt_mode(), InterruptMode::Im0);
    for service in SERVICES {
        let (mut cpu, mut bus) = service_fixture(service, 0x1234, 1, 0xff, true);
        cpu.step_with_bus(&mut bus).unwrap();
        codec(&cpu);
        cpu.reset();
        assert_eq!(
            cpu.regs(),
            Registers {
                pc: 0x100,
                sp: 0x9876,
                ..Registers::default()
            }
        );
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, false)
        );
        assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);
        assert_eq!(
            (
                cpu.cycles(),
                cpu.last_opcode_pc(),
                cpu.last_opcode(),
                cpu.last_m1_fetch_count()
            ),
            (0, 0x100, 0, 0)
        );
        assert!(!cpu.last_step_was_interrupt());
        assert!(cpu.instruction_bytes().is_empty());
        assert_eq!(cpu.trap(), None);
        codec(&cpu);
    }
    for api in 0..3 {
        halt(api, false, false);
        halt(api, true, false);
        halt(api, false, true);
    }
    let mut guards = 0;
    for byte in 0..=255u8 {
        if [
            0x00, 0x01, 0x06, 0x0e, 0x11, 0x16, 0x1e, 0x21, 0x26, 0x2e, 0x31, 0x3e, 0xc2, 0xc3,
            0xc4, 0xc7, 0xca, 0xcc, 0xcd, 0xcf, 0xd2, 0xd4, 0xd7, 0xda, 0xdc, 0xdf, 0xe2, 0xe4,
            0xe7, 0xea, 0xec, 0xef, 0xf2, 0xf4, 0xf7, 0xfa, 0xfc, 0xff,
        ]
        .contains(&byte)
        {
            continue;
        }
        let (cpu, bus) = fixture(0x46, byte, 0x4000, 0x8000, 0xff, 0x12);
        let initial = cpu.regs();
        let mut expected = initial;
        expected.r = 0x80;
        let mut aggregate = (cpu.clone(), bus.clone());
        let instruction = aggregate.0.step_with_bus(&mut aggregate.1).unwrap();
        assert_eq!(
            (instruction.pc, instruction.opcode, instruction.cycles),
            (0x4000, 0xff, 6)
        );
        assert_eq!(aggregate.0.regs(), expected);
        assert_eq!(aggregate.0.state(), CpuState::Suspended);
        assert_eq!(
            aggregate.0.trap(),
            Some(CpuTrap::UnsupportedOpcode {
                pc: 0x4000,
                opcode: byte
            })
        );
        assert_eq!(
            (
                aggregate.0.interrupts_enabled(),
                aggregate.0.saved_interrupts_enabled()
            ),
            (false, false)
        );
        assert_eq!(aggregate.1.memory.bytes, bus.memory.bytes);
        assert!(trace(&aggregate.1).is_empty());
        assert_eq!(aggregate.1.irq_acks, 1);
        metadata(&aggregate.0, 0x4000);
        let mut bounded = (cpu.clone(), bus.clone());
        let mut prefix = PrefixState::default();
        let step = bounded.0.step_bounded(&mut bounded.1, &mut prefix).unwrap();
        assert_eq!(
            (step.instruction, step.complete, prefix.pending()),
            (instruction, true, false)
        );
        assert_eq!(bounded, aggregate);
        let mut segmented = (cpu, bus);
        phase(
            &mut segmented.0,
            &mut segmented.1,
            &mut BusState::default(),
            6,
            true,
        );
        assert_eq!(segmented, aggregate);
        let saved = aggregate.clone();
        assert!(aggregate.0.step_with_bus(&mut aggregate.1).is_none());
        assert_eq!(aggregate, saved);
        assert!(
            bounded
                .0
                .step_bounded(&mut bounded.1, &mut prefix)
                .is_none()
        );
        assert_eq!(bounded, saved);
        assert!(
            segmented
                .0
                .step_bus_segment(&mut segmented.1, &mut BusState::default())
                .is_none()
        );
        assert_eq!(segmented, saved);
        guards += 1;
    }
    assert_eq!(guards, 218);
}
