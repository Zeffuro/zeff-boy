use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::{InterruptMode, ResetState};

pub fn im0_pending_clone_suspend_reset() {
    let mut boundaries = 0;
    for case in cases() {
        for count in 1..case.clocks().len() {
            let (mut cpu, mut bus) = fixture_flags(case.opcode, 0xffff, 1, 0xff, case.flags);
            let mut state = BusState::default();
            for &clock in &case.clocks()[..count] {
                phase(&mut cpu, &mut bus, &mut state, clock, false);
            }
            cpu.suspend();
            let saved = (cpu.clone(), bus.clone(), state);
            assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
            assert_eq!((cpu.clone(), bus.clone(), state), saved);
            cpu.resume();
            bus.memory.irq = false;
            bus.memory.nmi = true;
            bus.opcode = 0xdd;
            bus.operands = [0xa5, 0x5a];
            let polls = bus.memory.polls.get();
            let acknowledgements = (bus.acknowledgements, bus.memory.acknowledgements);
            let mut cloned = (cpu.clone(), bus.clone(), state);
            finish(&mut cpu, &mut bus, &mut state, case, count);
            finish(&mut cloned.0, &mut cloned.1, &mut cloned.2, case, count);
            assert_eq!(bus.memory.polls.get(), polls);
            assert_eq!(
                (bus.acknowledgements, bus.memory.acknowledgements),
                acknowledgements
            );
            assert_eq!((cpu, bus, state), cloned);

            let (mut cpu, mut bus, _) = saved;
            let reads = (bus.read_count, bus.reads);
            cpu.reset();
            bus.memory.irq = false;
            bus.memory.nmi = false;
            bus.memory.bytes[0x100] = 0;
            let memory = bus.memory.bytes.clone();
            bus.memory.clear_trace();
            let mut fresh_state = BusState::default();
            let step = cpu.step_bus_segment(&mut bus, &mut fresh_state).unwrap();
            assert_eq!(
                (
                    step.instruction.pc,
                    step.instruction.opcode,
                    step.instruction.cycles
                ),
                (0x100, 0, 4)
            );
            assert!(step.complete);
            assert!(!fresh_state.pending());
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
            assert_eq!(
                (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
                (false, false)
            );
            assert_eq!(cpu.trap(), None);
            assert!(!cpu.last_step_was_interrupt());
            assert_eq!((bus.read_count, bus.reads), reads);
            assert_eq!(
                (bus.acknowledgements, bus.memory.acknowledgements),
                acknowledgements
            );
            assert_eq!(trace(&bus), vec![Some(Access::Read(0x100, 0))]);
            assert_eq!(bus.memory.bytes, memory);
            boundaries += 1;
        }
    }
    assert_eq!(boundaries, 126);
}

fn encoded(cpu: &Cpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_state(&mut writer);
    writer.into_bytes()
}

fn codec(cpu: &Cpu, trapped: bool) {
    let bytes = encoded(cpu);
    assert_eq!(bytes.len(), if trapped { 46 } else { 43 });
    let regs = cpu.regs();
    assert_eq!(
        &bytes[..8],
        &[
            regs.a, regs.f, regs.b, regs.c, regs.d, regs.e, regs.h, regs.l
        ]
    );
    for (offset, value) in [(8, regs.ix), (10, regs.iy), (12, regs.sp), (14, regs.pc)] {
        assert_eq!(&bytes[offset..offset + 2], &value.to_le_bytes());
    }
    assert_eq!(&bytes[16..26], &[regs.i, regs.r, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&bytes[26..31], &[if trapped { 2 } else { 0 }, 0, 0, 0, 0]);
    assert_eq!(&bytes[31..39], &cpu.cycles().to_le_bytes());
    assert_eq!(
        &bytes[39..42],
        &[
            cpu.last_opcode_pc() as u8,
            (cpu.last_opcode_pc() >> 8) as u8,
            0xff
        ]
    );
    assert_eq!(bytes[42], u8::from(trapped));
    if let Some(CpuTrap::UnsupportedOpcode { pc, opcode }) = cpu.trap() {
        assert_eq!(&bytes[43..], &[pc as u8, (pc >> 8) as u8, opcode]);
    }
    let mut decoded = Cpu::new_with_reset(ResetState::new(0x100, 0x9876));
    decoded.read_state(&mut StateReader::new(&bytes)).unwrap();
    assert_eq!(decoded.regs(), cpu.regs());
    assert_eq!(
        (decoded.state(), decoded.interrupt_mode(), decoded.trap()),
        (cpu.state(), InterruptMode::Im0, cpu.trap())
    );
    assert_eq!(
        (
            decoded.interrupts_enabled(),
            decoded.saved_interrupts_enabled()
        ),
        (false, false)
    );
    assert_eq!(
        (
            decoded.cycles(),
            decoded.last_opcode_pc(),
            decoded.last_opcode()
        ),
        (cpu.cycles(), cpu.last_opcode_pc(), 0xff)
    );
    assert!(!decoded.last_step_was_interrupt());
    assert_eq!(decoded.last_m1_fetch_count(), 0);
    assert!(decoded.instruction_bytes().is_empty());
    assert_eq!(encoded(&decoded), bytes);
}

pub fn im0_completed_codec_layout_and_tags() {
    for case in cases() {
        let (mut cpu, mut bus) = fixture_flags(case.opcode, 0xffff, 1, 0xff, case.flags);
        cpu.step_with_bus(&mut bus).unwrap();
        codec(&cpu, false);
    }
    let (mut cpu, mut bus) = fixture(0xdd, 0xffff, 1, 0xff);
    cpu.step_with_bus(&mut bus).unwrap();
    codec(&cpu, true);
    let bytes = encoded(&cpu);
    for (offset, value) in [(26, 3), (27, 3), (30, 3), (42, 4)] {
        let mut invalid = bytes.clone();
        invalid[offset] = value;
        let mut decoded = Cpu::default();
        assert!(decoded.read_state(&mut StateReader::new(&invalid)).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoryIrq(IrqBus);

impl Z80Bus for MemoryIrq {
    fn cpu_read(&self, address: u16) -> u8 {
        self.0.cpu_read(address)
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.0.cpu_write(address, value);
    }
    fn io_read(&mut self, port: u8) -> u8 {
        self.0.io_read(port)
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.0.io_write(port, value);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.0.maskable_interrupt_pending()
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.0.non_maskable_interrupt_pending()
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.0.acknowledge_maskable_interrupt()
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.0.acknowledge_non_maskable_interrupt()
    }
}

pub fn im0_default_reads_and_remaining_guards() {
    for opcode in [0x3e, 0x21, 0xc3, 0xcd] {
        for pc in [0x1234, 0xfffe, 0xffff] {
            let (cpu, mut bus) = fixture(opcode, pc, 1, 0xff);
            bus.memory.bytes[usize::from(pc)] = 0x78;
            bus.memory.bytes[usize::from(pc.wrapping_add(1))] = 0x56;
            for api in 0..3 {
                let mut cpu = cpu.clone();
                let mut memory = MemoryIrq(bus.clone());
                let before = cpu.cycles();
                let instruction = if api == 0 {
                    cpu.step_with_bus(&mut memory).unwrap()
                } else if api == 1 {
                    let step = cpu
                        .step_bounded(&mut memory, &mut PrefixState::default())
                        .unwrap();
                    assert!(step.complete);
                    step.instruction
                } else {
                    let mut state = BusState::default();
                    let first = cpu.step_bus_segment(&mut memory, &mut state).unwrap();
                    assert_eq!((first.instruction.cycles, first.complete), (6, false));
                    assert_eq!(trace(&memory.0), vec![]);
                    memory.0.memory.bytes[usize::from(pc)] = 0x78;
                    let low = cpu.step_bus_segment(&mut memory, &mut state).unwrap();
                    assert_eq!((low.instruction.cycles, low.complete), (3, opcode == 0x3e));
                    if opcode != 0x3e {
                        memory.0.memory.bytes[usize::from(pc)] = 0x56;
                        let high = cpu.step_bus_segment(&mut memory, &mut state).unwrap();
                        assert_eq!(
                            (high.instruction.cycles, high.complete),
                            (3, opcode != 0xcd)
                        );
                        if opcode == 0xcd {
                            for (clock, complete) in [(1, false), (3, false), (3, true)] {
                                let step = cpu.step_bus_segment(&mut memory, &mut state).unwrap();
                                assert_eq!(
                                    (step.instruction.cycles, step.complete),
                                    (clock, complete)
                                );
                            }
                        }
                    }
                    assert!(!state.pending());
                    first.instruction
                };
                let target = if api == 2 { 0x5678 } else { 0x7878 };
                let mut expected_trace = vec![Some(Access::Read(pc, 0x78))];
                if opcode != 0x3e {
                    expected_trace.push(Some(Access::Read(pc, if api == 2 { 0x56 } else { 0x78 })));
                }
                if opcode == 0xcd {
                    expected_trace.extend([
                        Some(Access::Write(0, (pc >> 8) as u8)),
                        Some(Access::Write(0xffff, pc as u8)),
                    ]);
                }
                assert_eq!(trace(&memory.0), expected_trace);
                assert_eq!(memory.0.read_count, 0);
                assert_eq!(
                    (memory.0.acknowledgements, memory.0.memory.polls.get()),
                    (1, 2)
                );
                assert_eq!(
                    cpu.cycles() - before,
                    match opcode {
                        0x3e => 9,
                        0xcd => 19,
                        _ => 12,
                    }
                );
                if api != 2 {
                    assert_eq!(u64::from(instruction.cycles), cpu.cycles() - before);
                }
                match opcode {
                    0x3e => assert_eq!((cpu.regs().a, cpu.regs().pc), (0x78, pc)),
                    0x21 => assert_eq!((cpu.regs().hl(), cpu.regs().pc), (target, pc)),
                    _ => assert_eq!(cpu.regs().pc, target),
                }
                metadata(&cpu, pc);
            }
        }
    }
    let supported = cases();
    let mut guards = 0;
    for opcode in 0..=255 {
        if supported.iter().any(|case| case.opcode == opcode) {
            continue;
        }
        let (cpu, bus) = fixture(opcode, 0xfffe, 0, 0x7f);
        let mut expected = cpu.regs();
        expected.r = 0;
        let mut aggregate = (cpu.clone(), bus.clone());
        let instruction = aggregate.0.step_with_bus(&mut aggregate.1).unwrap();
        assert_eq!(
            (instruction.pc, instruction.opcode, instruction.cycles),
            (0xfffe, 0xff, 6)
        );
        assert_eq!(aggregate.0.regs(), expected);
        assert_eq!(aggregate.0.state(), CpuState::Suspended);
        assert_eq!(
            aggregate.0.trap(),
            Some(CpuTrap::UnsupportedOpcode { pc: 0xfffe, opcode })
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
        assert_eq!(
            (
                aggregate.1.acknowledgements,
                aggregate.1.read_count,
                aggregate.1.memory.polls.get()
            ),
            (1, 0, 2)
        );
        metadata(&aggregate.0, 0xfffe);
        let mut bounded = (cpu.clone(), bus.clone());
        let mut prefix = PrefixState::default();
        let step = bounded.0.step_bounded(&mut bounded.1, &mut prefix).unwrap();
        assert_eq!(
            (step.instruction, step.complete, prefix.pending()),
            (instruction, true, false)
        );
        assert_eq!(bounded, aggregate);
        let mut segmented = (cpu, bus);
        let mut state = BusState::default();
        phase(&mut segmented.0, &mut segmented.1, &mut state, 6, true);
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
                .step_bus_segment(&mut segmented.1, &mut state)
                .is_none()
        );
        assert_eq!(segmented, saved);
        codec(&saved.0, true);
        guards += 1;
    }
    assert_eq!(guards, 218);
}
