use super::*;
#[path = "operand_tests/ed_word.rs"]
mod ed_word;
#[path = "operand_tests/indexed_bit.rs"]
mod indexed_bit;
#[path = "operand_tests/indexed_immediate.rs"]
mod indexed_immediate;
#[path = "operand_tests/indexed_inc_dec.rs"]
mod indexed_inc_dec;

struct Bus {
    address: u16,
    value: u8,
    write: bool,
    reads: std::cell::Cell<u8>,
    writes: u8,
}

impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        assert!(!self.write);
        assert_eq!(address, self.address);
        self.reads.set(self.reads.get() + 1);
        self.value
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        assert!(self.write);
        assert_eq!((address, value), (self.address, self.value));
        self.writes += 1;
    }
    fn io_read(&mut self, _: u8) -> u8 {
        panic!("unexpected I/O read");
    }
    fn io_write(&mut self, _: u8, _: u8) {
        panic!("unexpected I/O write");
    }
    fn maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        panic!("unexpected NMI acknowledge");
    }
}

pub(super) fn physical_operand_retirement() {
    ed_word::retirement();
    indexed_bit::retirement();
    indexed_immediate::retirement();
    indexed_inc_dec::retirement();
    super::prefix_tests::physical_prefix_retirement();
    super::rmw_tests::physical_memory_retirement();
    super::index_tests::physical_index_retirement();
    super::m1_tests::physical_m1_retirement();
    super::call_tests::physical_call_retirement();
    let opcodes = [
        0x02, 0x12, 0x0a, 0x1a, 0x46, 0x4e, 0x56, 0x5e, 0x66, 0x6e, 0x7e, 0x70, 0x71, 0x72, 0x73,
        0x74, 0x75, 0x77, 0x86, 0x8e, 0x96, 0x9e, 0xa6, 0xae, 0xb6, 0xbe, 0xc6, 0xce, 0xd6, 0xde,
        0xe6, 0xee, 0xf6, 0xfe, 0xc3, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa,
    ];
    let inputs = CycleInterrupts::default();
    for opcode in opcodes {
        for flags in [0, u8::MAX] {
            let mut cpu = Cpu::new();
            cpu.regs.a = 0x80;
            cpu.regs.f = flags;
            cpu.regs.set_bc(0x1234);
            cpu.regs.set_de(0x5678);
            cpu.regs.set_hl(0x9abc);
            cpu.regs.pc = u16::MAX;
            cpu.regs.r = 0xff;
            let mut state = BusState::default();
            let mut legacy = cpu.clone();
            let mut legacy_state = state;
            let mut phases = vec![(u16::MAX, opcode, true, false)];
            match opcode {
                0x02 => phases.push((0x1234, 0x80, false, true)),
                0x12 => phases.push((0x5678, 0x80, false, true)),
                0x0a => phases.push((0x1234, 0x7f, false, false)),
                0x1a => phases.push((0x5678, 0x7f, false, false)),
                0x70..=0x75 | 0x77 => {
                    phases.push((0x9abc, cpu.read_register8(opcode & 7), false, true));
                }
                0xc6 | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe => {
                    phases.push((0, 0x7f, false, false));
                }
                _ if stack::Stack::is_jump(opcode) => {
                    phases.extend([(0, 0x34, false, false), (1, 0x12, false, false)]);
                }
                _ => phases.push((0x9abc, 0x7f, false, false)),
            }
            let phase_count = phases.len();
            for (index, (address, value, m1, write)) in phases.into_iter().enumerate() {
                let saved = cpu.clone();
                let saved_state = state;
                let request = cpu.next_physical_cycle(&state).unwrap();
                let kind = request.kind();
                assert_eq!(
                    (kind.address(), kind.is_m1(), kind.is_write()),
                    (address, m1, write)
                );
                if write {
                    assert_eq!(kind, CycleKind::MemoryWrite { address, value });
                }
                let sample = CycleSample {
                    address,
                    value,
                    m1,
                    write,
                    known_mask: u8::MAX,
                };
                for bad in [
                    CycleSample {
                        address: address ^ 1,
                        ..sample
                    },
                    CycleSample {
                        write: !write,
                        ..sample
                    },
                    CycleSample { m1: !m1, ..sample },
                ] {
                    assert!(matches!(
                        cpu.next_physical_cycle(&state)
                            .unwrap()
                            .complete(bad, 0, inputs),
                        Err(CycleError::SampleMismatch)
                    ));
                }
                if write {
                    assert!(matches!(
                        cpu.next_physical_cycle(&state).unwrap().complete(
                            CycleSample {
                                value: value ^ 1,
                                ..sample
                            },
                            0,
                            inputs
                        ),
                        Err(CycleError::SampleMismatch)
                    ));
                }
                let completion = request.complete(sample, 2, inputs).unwrap();
                assert_eq!(cpu, saved);
                assert_eq!(state, saved_state);
                let mut memory = Bus {
                    address,
                    value,
                    write,
                    reads: std::cell::Cell::new(0),
                    writes: 0,
                };
                let mut expected = legacy
                    .step_bus_segment(&mut memory, &mut legacy_state)
                    .unwrap();
                assert_eq!(
                    (memory.reads.get(), memory.writes),
                    (u8::from(!write), u8::from(write))
                );
                assert_eq!(expected.complete, index + 1 == phase_count);
                assert_eq!(expected.instruction.cycles, if m1 { 4 } else { 3 });
                assert!(legacy.extend_bus_segment(&mut expected.instruction, 2));
                assert_eq!(
                    cpu.retire_physical_cycle(&mut state, completion).unwrap(),
                    expected
                );
                assert_eq!(cpu, legacy);
                assert_eq!(state, legacy_state);
                let mut writer = crate::state::Writer::new();
                cpu.write_continuation(&state, &mut writer);
                let bytes = writer.into_bytes();
                let mut restored = Cpu::new();
                let mut restored_state = BusState::default();
                restored
                    .read_continuation(&mut restored_state, &mut crate::state::Reader::new(&bytes))
                    .unwrap();
                assert_eq!(restored, cpu);
                assert_eq!(restored_state, state);
                if state.pending() {
                    assert_eq!(
                        restored
                            .next_physical_cycle(&restored_state)
                            .unwrap()
                            .kind(),
                        cpu.next_physical_cycle(&state).unwrap().kind()
                    );
                }
            }
            assert!(!state.pending());
            assert_eq!(cpu.regs.r, 0x80);
            assert_eq!(cpu.cycles, if phase_count == 3 { 16 } else { 11 });
            if stack::Stack::is_jump(opcode) {
                let taken = opcode == 0xc3 || cpu.condition_is_true((opcode >> 3) & 7);
                assert_eq!(cpu.regs.pc, if taken { 0x1234 } else { 2 });
                assert_eq!(cpu.instruction_bytes, [opcode, 0x34, 0x12, 0]);
                assert_eq!(cpu.instruction_byte_count, 3);
            } else {
                let immediate = opcode & 0xc7 == 0xc6;
                assert_eq!(cpu.regs.pc, u16::from(immediate));
                assert_eq!(cpu.instruction_byte_count, if immediate { 2 } else { 1 });
            }
        }
    }
    operand_continuation_guards();
}

fn operand_continuation_guards() {
    let mut cpu = Cpu::new();
    cpu.last_opcode = 0x77;
    cpu.regs.set_hl(0x1234);
    cpu.regs.a = 0xa5;
    let mut state = BusState {
        transfer: Some(Transfer {
            pc: 0,
            operation: Operation::Single {
                opcode: 0x77,
                index: None,
            },
        }),
        ..BusState::default()
    };
    cpu.regs.set_hl(0x5678);
    cpu.regs.a = 0x3c;
    let request = cpu.next_physical_cycle(&state).unwrap();
    assert_eq!(
        request.kind(),
        CycleKind::MemoryWrite {
            address: 0x5678,
            value: 0x3c
        }
    );
    let completion = request
        .complete(
            CycleSample {
                address: 0x5678,
                m1: false,
                write: true,
                value: 0x3c,
                known_mask: u8::MAX,
            },
            0,
            CycleInterrupts::default(),
        )
        .unwrap();
    cpu.regs.a ^= 1;
    let saved = cpu.clone();
    let saved_state = state;
    assert_eq!(
        cpu.retire_physical_cycle(&mut state, completion),
        Err(CycleError::StaleContinuation)
    );
    assert_eq!(cpu, saved);
    assert_eq!(state, saved_state);
    for (opcode, index) in [(0x70, None), (0x77, Some(Z80_PREFIX_DD)), (0x34, None)] {
        state.transfer.as_mut().unwrap().operation = Operation::Single { opcode, index };
        assert!(matches!(
            cpu.next_physical_cycle(&state),
            Err(CycleError::UnsupportedCycle)
        ));
    }
    for payload in [
        &[0, 1, 0, 0x60, 0, 0, 0, 0][..],
        &[0, 0, 0, 0x60, 0, 0, 0, 1, 0][..],
        &[0, 0, 0, 0x60, 0, 0, 2, 0][..],
        &[0, 0, 0, 0x60, 0, 1, 0, 0][..],
    ] {
        cpu.last_opcode = 0xc3;
        let operation =
            stack::Stack::read_continuation(&mut crate::state::Reader::new(payload)).unwrap();
        state.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
        assert!(matches!(
            cpu.next_physical_cycle(&state),
            Err(CycleError::UnsupportedCycle)
        ));
    }
    for opcode in [0xc3, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa] {
        let mut operation = stack::Stack::decode_interrupt(&cpu, opcode).unwrap();
        state.transfer.as_mut().unwrap().operation = Operation::Stack(operation);
        cpu.last_opcode = opcode;
        assert!(matches!(
            cpu.next_physical_cycle(&state),
            Err(CycleError::UnsupportedCycle)
        ));
        let saved_pc = cpu.regs.pc;
        let saved_bytes = (cpu.instruction_bytes, cpu.instruction_byte_count);
        let mut memory = Bus {
            address: saved_pc,
            value: 0x34,
            write: false,
            reads: std::cell::Cell::new(0),
            writes: 0,
        };
        operation = operation.step(&mut cpu, &mut memory).0.unwrap();
        assert_eq!(cpu.regs.pc, saved_pc);
        assert!(operation.step(&mut cpu, &mut memory).0.is_none());
        assert_eq!(
            (cpu.instruction_bytes, cpu.instruction_byte_count),
            saved_bytes
        );
        assert_eq!(memory.reads.get(), 2);
    }
}
