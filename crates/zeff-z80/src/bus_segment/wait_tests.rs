use super::*;

#[test]
fn wait_report_and_total_overflow_are_atomic() {
    physical_read_retirement();
    super::operand_tests::physical_operand_retirement();
    for (total, cycles, waits) in [(u64::MAX, 4, 1), (u64::MAX - 1, 4, 2), (8, u32::MAX, 1)] {
        let mut cpu = Cpu::new();
        cpu.cycles = total;
        let mut report = FetchedInstruction {
            pc: 0x1234,
            opcode: 0xe5,
            cycles,
        };
        let saved = cpu.clone();
        let original = report;
        assert!(!cpu.extend_bus_segment(&mut report, waits));
        assert_eq!(cpu, saved);
        assert_eq!(report, original);
    }
}

fn physical_read_retirement() {
    struct ReadBus {
        address: u16,
        value: u8,
        write: bool,
        reads: std::cell::Cell<u8>,
        writes: u8,
    }
    impl Z80Bus for ReadBus {
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
            true
        }
        fn non_maskable_interrupt_pending(&self) -> bool {
            false
        }
        fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
            panic!("unexpected NMI acknowledge");
        }
    }
    let inputs = CycleInterrupts {
        maskable_pending: true,
        nmi_pending: false,
    };
    let mut cpu = Cpu::new();
    let mut state = BusState::default();
    let mut legacy = cpu.clone();
    let mut legacy_state = state;
    for (address, value, m1, halted, waits) in [
        (0, 0x3a, true, false, 0),
        (1, 0x01, false, false, 1),
        (2, 0x80, false, false, 2),
        (0x8001, 0x3c, false, false, 5),
        (3, 0x76, true, false, 0),
        (4, 0xa5, true, true, 3),
    ] {
        let saved = cpu.clone();
        let saved_state = state;
        let request = cpu.next_physical_cycle(&state).unwrap();
        assert_eq!(request.kind().address(), address);
        assert_eq!(request.kind().is_m1(), m1);
        if let CycleKind::M1 {
            refresh_address,
            halted: actual,
            ..
        } = request.kind()
        {
            assert_eq!(actual, halted);
            assert_eq!(
                refresh_address,
                u16::from_be_bytes([cpu.regs.i, cpu.regs.r])
            );
        }
        request.validate_interrupts(inputs).unwrap();
        let sample = CycleSample {
            address,
            m1,
            write: false,
            value,
            known_mask: u8::MAX,
        };
        let completion = request.complete(sample, waits, inputs).unwrap();
        assert_eq!(cpu, saved);
        assert_eq!(state, saved_state);
        let mut bus = ReadBus {
            address,
            value,
            write: false,
            reads: std::cell::Cell::new(0),
            writes: 0,
        };
        let mut expected = legacy
            .step_bus_segment(&mut bus, &mut legacy_state)
            .unwrap();
        assert_eq!(bus.reads.get(), 1);
        assert!(legacy.extend_bus_segment(&mut expected.instruction, waits));
        assert_eq!(
            cpu.retire_physical_cycle(&mut state, completion).unwrap(),
            expected
        );
        assert_eq!(cpu, legacy);
        assert_eq!(state, legacy_state);
    }
    assert_eq!(cpu.regs.a, 0x3c);
    assert_eq!(cpu.regs.pc, 4);
    assert_eq!(cpu.regs.r, 3);
    assert!(cpu.is_halted());

    for opcode in [
        0x06, 0x0e, 0x16, 0x1e, 0x26, 0x2e, 0x3e, 0x01, 0x11, 0x21, 0x31, 0x22, 0x2a, 0x32, 0x3a,
        0x36,
    ] {
        let mut cpu = Cpu::new();
        cpu.regs.a = 0xa5;
        cpu.regs.set_hl(0x9abc);
        let mut state = BusState::default();
        let mut legacy = cpu.clone();
        let mut legacy_state = state;
        let mut cycles = vec![(0, opcode, true, false)];
        match opcode {
            0x01 | 0x11 | 0x21 | 0x31 => {
                cycles.extend([(1, 0x34, false, false), (2, 0x12, false, false)]);
            }
            0x22 | 0x2a | 0x32 | 0x3a => {
                cycles.extend([(1, 0xff, false, false), (2, 0xff, false, false)]);
                match opcode {
                    0x22 => cycles.extend([(0xffff, 0xbc, false, true), (0, 0x9a, false, true)]),
                    0x2a => cycles.extend([(0xffff, 0x34, false, false), (0, 0x12, false, false)]),
                    0x32 => cycles.push((0xffff, 0xa5, false, true)),
                    0x3a => cycles.push((0xffff, 0x34, false, false)),
                    _ => unreachable!(),
                }
            }
            0x36 => cycles.extend([(1, 0x34, false, false), (0x9abc, 0x34, false, true)]),
            _ => cycles.push((1, 0x34, false, false)),
        }
        for (address, value, m1, write) in cycles {
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
                m1,
                write,
                value,
                known_mask: u8::MAX,
            };
            if write {
                for (bad, error) in [
                    (
                        CycleSample {
                            write: false,
                            ..sample
                        },
                        CycleError::SampleMismatch,
                    ),
                    (
                        CycleSample {
                            value: value ^ 1,
                            ..sample
                        },
                        CycleError::SampleMismatch,
                    ),
                    (
                        CycleSample {
                            known_mask: 0xfe,
                            ..sample
                        },
                        CycleError::UnknownData,
                    ),
                ] {
                    assert!(matches!(
                        cpu.next_physical_cycle(&state).unwrap().complete(bad, 0, inputs),
                        Err(actual) if actual == error
                    ));
                }
            }
            let completion = request.complete(sample, 2, inputs).unwrap();
            assert_eq!(cpu, saved);
            assert_eq!(state, saved_state);
            let mut bus = ReadBus {
                address,
                value,
                write,
                reads: std::cell::Cell::new(0),
                writes: 0,
            };
            let mut expected = legacy
                .step_bus_segment(&mut bus, &mut legacy_state)
                .unwrap();
            assert_eq!(
                (bus.reads.get(), bus.writes),
                (u8::from(!write), u8::from(write))
            );
            assert!(legacy.extend_bus_segment(&mut expected.instruction, 2));
            assert_eq!(
                cpu.retire_physical_cycle(&mut state, completion).unwrap(),
                expected
            );
            assert_eq!(cpu, legacy);
            assert_eq!(state, legacy_state);
        }
        assert!(!state.pending());
        match opcode {
            0x01 | 0x11 | 0x21 | 0x31 => {
                assert_eq!(cpu.read_reg16((opcode >> 4) & 3), 0x1234);
            }
            0x2a => assert_eq!(cpu.regs.hl(), 0x1234),
            0x3a => assert_eq!(cpu.regs.a, 0x34),
            0x22 | 0x32 | 0x36 => {}
            _ => assert_eq!(cpu.read_register8((opcode >> 3) & 7), 0x34),
        }
        assert_eq!(cpu.regs.pc, if opcode & 0xc7 == 0x06 { 2 } else { 3 });
        assert_eq!(cpu.regs.r, 1);
        if let Some(load) = load::Load::decode_interrupt(&cpu, opcode) {
            state.transfer = Some(Transfer {
                pc: 0,
                operation: Operation::Load(load),
            });
            assert!(matches!(
                cpu.next_physical_cycle(&state),
                Err(CycleError::UnsupportedCycle)
            ));
            let saved_pc = cpu.regs.pc;
            let saved_bytes = (cpu.instruction_bytes, cpu.instruction_byte_count);
            let mut bus = ReadBus {
                address: saved_pc,
                value: 0x34,
                write: false,
                reads: std::cell::Cell::new(0),
                writes: 0,
            };
            let mut pending = Some(load);
            while let Some(load) = pending {
                pending = load.step(&mut cpu, &mut bus);
                assert_eq!(cpu.regs.pc, saved_pc);
                assert_eq!(
                    (cpu.instruction_bytes, cpu.instruction_byte_count),
                    saved_bytes
                );
            }
            assert_eq!(bus.reads.get(), if opcode & 0x0f == 1 { 2 } else { 1 });
        }
    }

    let mut admitted = 0;
    for opcode in u8::MIN..=u8::MAX {
        let expected = matches!(
            opcode,
            0x00 | 0x07
                | 0x08
                | 0x0f
                | 0x17
                | 0x1f
                | 0x27
                | 0x2f
                | 0x37
                | 0x3f
                | 0xd9
                | 0xe9
                | 0xeb
                | 0xf3
        ) || (opcode < 0x40
            && matches!(opcode & 7, 4 | 5)
            && ((opcode >> 3) & 7) != 6)
            || ((0x40..=0x7f).contains(&opcode) && (opcode & 7) != 6 && ((opcode >> 3) & 7) != 6)
            || ((0x80..=0xbf).contains(&opcode) && (opcode & 7) != 6);
        assert_eq!(
            register_cycle::decode(opcode).is_some(),
            expected,
            "{opcode:02x}"
        );
        if !expected {
            continue;
        }
        admitted += 1;
        for (seed, flags, waits) in [(0x8f_u8, 0xd7, 0), (0xff, 0xff, 3), (0x7f, 0, 5)] {
            let mut cpu = Cpu::new();
            cpu.regs = Registers {
                a: seed,
                f: flags,
                b: seed,
                c: seed.rotate_left(1),
                d: seed ^ 0x80,
                e: seed.wrapping_sub(1),
                h: 0x9a,
                l: 0xbc,
                sp: 0x4567,
                pc: u16::MAX,
                ix: 0xabcd,
                iy: 0x0102,
                i: 0x5a,
                r: 0xff,
            };
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
            cpu.cycles = 0x1234;
            cpu.interrupt_flip_flop_1 = true;
            cpu.interrupt_flip_flop_2 = false;
            cpu.enable_interrupts_delay = 1;
            cpu.instruction_bytes = [0xa5; 4];
            cpu.instruction_byte_count = 4;
            cpu.last_m1_fetch_count = 9;
            cpu.last_step_was_interrupt = true;
            let mut state = BusState::default();
            let saved = cpu.clone();
            let mut legacy = cpu.clone();
            let mut legacy_state = state;
            let request = cpu.next_physical_cycle(&state).unwrap();
            assert_eq!(
                request.kind(),
                CycleKind::M1 {
                    address: u16::MAX,
                    halted: false,
                    refresh_address: 0x5aff,
                }
            );
            let completion = request
                .complete(
                    CycleSample {
                        address: u16::MAX,
                        m1: true,
                        write: false,
                        value: opcode,
                        known_mask: u8::MAX,
                    },
                    waits,
                    inputs,
                )
                .unwrap();
            assert_eq!(cpu, saved);
            assert_eq!(state, BusState::default());
            let mut bus = ReadBus {
                address: u16::MAX,
                value: opcode,
                write: false,
                reads: std::cell::Cell::new(0),
                writes: 0,
            };
            let mut expected = legacy
                .step_bus_segment(&mut bus, &mut legacy_state)
                .unwrap();
            assert_eq!((bus.reads.get(), bus.writes), (1, 0));
            assert!(expected.complete);
            assert_eq!(expected.instruction.cycles, CYCLES_M1);
            assert!(legacy.extend_bus_segment(&mut expected.instruction, waits));
            let actual = cpu.retire_physical_cycle(&mut state, completion).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.instruction.pc, u16::MAX);
            assert_eq!(actual.instruction.opcode, opcode);
            assert_eq!(actual.instruction.cycles, CYCLES_M1 + waits);
            assert_eq!(cpu, legacy);
            assert_eq!(state, legacy_state);
            assert_eq!(state, BusState::default());
            assert_eq!(cpu.regs.pc, if opcode == 0xe9 { 0x9abc } else { 0 });
            assert_eq!(cpu.regs.r, 0x80);
            assert_eq!(cpu.instruction_bytes, [opcode, 0, 0, 0]);
            assert_eq!(
                (cpu.instruction_byte_count, cpu.last_m1_fetch_count),
                (1, 1)
            );
            assert_eq!(cpu.cycles, 0x1234 + u64::from(CYCLES_M1 + waits));
            assert_eq!(cpu.enable_interrupts_delay, 0);
            assert_eq!(
                (cpu.interrupt_flip_flop_1, cpu.interrupt_flip_flop_2),
                (opcode != 0xf3, opcode != 0xf3)
            );
        }
    }
    assert_eq!(admitted, 133);

    let malformed: &[(u8, &[u8])] = &[
        (0x3a, &[0, 5, 3, 0xa5, 0, 0x60]),
        (0x06, &[0, 0, 7]),
        (0x06, &[0, 7, 0, 0x60, 0xa5]),
        (0x01, &[0, 1, 0, 1]),
        (0x2a, &[0, 6, 0, 1, 0, 0x60, 0x34]),
        (0x32, &[0, 3, 2]),
        (0x22, &[0, 5, 3, 0xa5, 0, 0x60]),
        (0x36, &[0, 5, 3, 0xa5, 0, 0x60]),
    ];
    for &(opcode, payload) in malformed {
        let mut cpu = Cpu::new();
        cpu.last_opcode = opcode;
        let load = load::Load::read_continuation(&mut crate::state::Reader::new(payload)).unwrap();
        let state = BusState {
            transfer: Some(Transfer {
                pc: 0,
                operation: Operation::Load(load),
            }),
            ..BusState::default()
        };
        let saved = cpu.clone();
        let saved_state = state;
        assert!(matches!(
            cpu.next_physical_cycle(&state),
            Err(CycleError::UnsupportedCycle)
        ));
        assert_eq!(cpu, saved);
        assert_eq!(state, saved_state);
    }

    let captured: &[(u8, &[u8], u16, u8)] = &[
        (0x32, &[0, 5, 3, 0xa5, 0, 0x60], 0x6000, 0xa5),
        (0x22, &[0, 5, 1, 0x34, 0x12, 0xff, 0xff], 0xffff, 0x34),
        (0x22, &[0, 7, 0, 0, 0x12], 0, 0x12),
        (0x36, &[0, 9, 0, 0x60, 0xa5], 0x6000, 0xa5),
    ];
    for &(opcode, payload, address, value) in captured {
        let mut cpu = Cpu::new();
        cpu.last_opcode = opcode;
        let load = load::Load::read_continuation(&mut crate::state::Reader::new(payload)).unwrap();
        let state = BusState {
            transfer: Some(Transfer {
                pc: 0,
                operation: Operation::Load(load),
            }),
            ..BusState::default()
        };
        assert_eq!(
            cpu.next_physical_cycle(&state).unwrap().kind(),
            CycleKind::MemoryWrite { address, value }
        );
    }

    for opcode in [0xd3, 0xfb] {
        let cpu = Cpu::new();
        let state = BusState::default();
        let saved = cpu.clone();
        let sample = CycleSample {
            address: 0,
            m1: true,
            write: false,
            value: opcode,
            known_mask: u8::MAX,
        };
        let completion = cpu
            .next_physical_cycle(&state)
            .unwrap()
            .complete(sample, 0, inputs);
        assert!(
            matches!(completion, Err(CycleError::UnsupportedOpcode { pc: 0, opcode: actual }) if actual == opcode)
        );
        assert_eq!(cpu, saved);
        assert_eq!(state, BusState::default());
    }
    let sample = CycleSample {
        address: 0,
        m1: true,
        write: false,
        value: 0x3a,
        known_mask: u8::MAX,
    };
    let mut cpu = Cpu::new();
    let mut state = BusState::default();
    let request = cpu.next_physical_cycle(&state).unwrap();
    assert!(matches!(
        request.complete(
            CycleSample {
                known_mask: 0xfe,
                ..sample
            },
            0,
            inputs
        ),
        Err(CycleError::UnknownData)
    ));
    let completion = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .complete(sample, 0, inputs)
        .unwrap();
    cpu.regs.a = 1;
    let saved = cpu.clone();
    assert_eq!(
        cpu.retire_physical_cycle(&mut state, completion),
        Err(CycleError::StaleContinuation)
    );
    assert_eq!(cpu, saved);
    assert_eq!(state, BusState::default());
    cpu.cycles = u64::MAX;
    let completion = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .complete(sample, 0, inputs)
        .unwrap();
    let saved = cpu.clone();
    assert_eq!(
        cpu.retire_physical_cycle(&mut state, completion),
        Err(CycleError::ClockOverflow)
    );
    assert_eq!(cpu, saved);
    assert_eq!(state, BusState::default());
    let mut cpu = Cpu::new();
    let first = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .complete(sample, 0, inputs)
        .unwrap();
    let duplicate = cpu
        .next_physical_cycle(&state)
        .unwrap()
        .complete(sample, 0, inputs)
        .unwrap();
    cpu.retire_physical_cycle(&mut state, first).unwrap();
    let saved = cpu.clone();
    let saved_state = state;
    assert_eq!(
        cpu.retire_physical_cycle(&mut state, duplicate),
        Err(CycleError::StaleContinuation)
    );
    assert_eq!(cpu, saved);
    assert_eq!(state, saved_state);
    let request = cpu.next_physical_cycle(&state).unwrap();
    state.transfer = None;
    let sample = CycleSample {
        address: 1,
        m1: false,
        write: false,
        value: 1,
        known_mask: u8::MAX,
    };
    let completion = request.complete(sample, 0, inputs).unwrap();
    let saved = cpu.clone();
    assert_eq!(
        cpu.retire_physical_cycle(&mut state, completion),
        Err(CycleError::StaleContinuation)
    );
    assert_eq!(cpu, saved);
    assert_eq!(state, BusState::default());
    let request = cpu.next_physical_cycle(&state).unwrap();
    assert_eq!(
        request.validate_interrupts(CycleInterrupts {
            nmi_pending: true,
            ..inputs
        }),
        Err(CycleError::UnsupportedInterrupt)
    );
    cpu.interrupt_flip_flop_1 = true;
    assert_eq!(
        cpu.next_physical_cycle(&state)
            .unwrap()
            .validate_interrupts(inputs),
        Err(CycleError::UnsupportedInterrupt)
    );
}

#[test]
fn maximum_wait_accounting_preserves_nonclock_state() {
    let mut cpu = Cpu::new();
    cpu.cycles = u64::MAX - u64::from(u32::MAX);
    let mut report = FetchedInstruction {
        pc: 0x1234,
        opcode: 0xe5,
        cycles: 0,
    };
    let mut expected = cpu.clone();
    assert!(cpu.extend_bus_segment(&mut report, u32::MAX));
    expected.cycles = u64::MAX;
    assert_eq!(cpu, expected);
    assert_eq!(report.cycles, u32::MAX);
    let saved = cpu.clone();
    let original = report;
    assert!(cpu.extend_bus_segment(&mut report, 0));
    assert_eq!(cpu, saved);
    assert_eq!(report, original);
}
