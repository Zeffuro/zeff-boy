use super::*;

pub(super) struct Bus {
    pub(super) sample: CycleSample,
    pub(super) interrupts: CycleInterrupts,
    pub(super) calls: std::cell::Cell<u8>,
}

impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        assert!(!self.sample.write);
        assert_eq!(address, self.sample.address);
        self.calls.set(self.calls.get() + 1);
        self.sample.value
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        assert!(self.sample.write);
        assert_eq!((address, value), (self.sample.address, self.sample.value));
        self.calls.set(self.calls.get() + 1);
    }
    fn io_read(&mut self, _: u8) -> u8 {
        panic!("unexpected prefix I/O read");
    }
    fn io_write(&mut self, _: u8, _: u8) {
        panic!("unexpected prefix I/O write");
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.interrupts.maskable_pending
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.interrupts.nmi_pending
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        panic!("prefix transfer must not acknowledge NMI");
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        panic!("prefix transfer must not acknowledge IRQ");
    }
}

pub(super) fn physical_prefix_retirement() {
    let mut counts = [0; 3];
    for opcode in u8::MIN..=u8::MAX {
        if opcode & 7 != REGISTER_MEMORY_INDEX {
            counts[0] += 1;
            for (value, flags) in [(0x80, 0), (0xff, 0xff), (0, Z80_FLAG_CARRY)] {
                qualified(Z80_PREFIX_CB, opcode, value, flags);
            }
        }
        if ed::is_neg(opcode) {
            counts[1] += 1;
            for value in [0, 1, 0x80, 0xff] {
                qualified(Z80_PREFIX_ED, opcode, value, 0xff);
            }
        } else if load::Load::decode_ed(&Cpu::new(), opcode).is_some() {
            counts[2] += 1;
            qualified(Z80_PREFIX_ED, opcode, 0x80, Z80_FLAG_CARRY);
        } else if !stack::Stack::is_ed_return(opcode) {
            unsupported(Z80_PREFIX_ED, opcode);
        }
    }
    assert_eq!(counts, [224, 8, 8]);
    malformed_prefixes();
}

pub(super) fn sample(kind: CycleKind, value: u8) -> CycleSample {
    CycleSample {
        address: kind.address(),
        m1: kind.is_m1(),
        write: kind.is_write(),
        value,
        known_mask: u8::MAX,
    }
}

pub(super) fn restored(cpu: &Cpu, state: &BusState) -> (Cpu, BusState) {
    let mut writer = crate::state::Writer::new();
    cpu.write_continuation(state, &mut writer);
    let bytes = writer.into_bytes();
    let mut result = Cpu::new();
    let mut bus = BusState::default();
    result
        .read_continuation(&mut bus, &mut crate::state::Reader::new(&bytes))
        .unwrap();
    assert_eq!(result, *cpu);
    assert_eq!(bus, *state);
    (result, bus)
}

pub(super) fn start(prefix: u8) -> (Cpu, BusState) {
    let mut cpu = Cpu::new();
    cpu.interrupt_flip_flop_1 = true;
    let mut state = BusState::default();
    let request = cpu.next_physical_cycle(&state).unwrap();
    for inputs in [
        CycleInterrupts {
            maskable_pending: true,
            nmi_pending: false,
        },
        CycleInterrupts {
            maskable_pending: false,
            nmi_pending: true,
        },
    ] {
        assert_eq!(
            request.validate_interrupts(inputs),
            Err(CycleError::UnsupportedInterrupt)
        );
    }
    let kind = request.kind();
    let completion = request
        .complete(sample(kind, prefix), 0, CycleInterrupts::default())
        .unwrap();
    cpu.retire_physical_cycle(&mut state, completion).unwrap();
    assert!(state.pending());
    assert_eq!(cpu.last_opcode, prefix);
    (cpu, state)
}

fn qualified(prefix: u8, opcode: u8, value: u8, flags: u8) {
    let mut cpu = Cpu::new();
    cpu.regs.a = value;
    cpu.regs.b = value;
    cpu.regs.c = value;
    cpu.regs.d = value;
    cpu.regs.e = value;
    cpu.regs.h = value;
    cpu.regs.l = value;
    cpu.regs.pc = u16::MAX;
    cpu.regs.i = 0x5a;
    cpu.regs.r = 0xff;
    cpu.regs.f = flags;
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    cpu.enable_interrupts_delay = 1;
    let load = prefix == Z80_PREFIX_ED && !ed::is_neg(opcode);
    if load {
        cpu.regs.set_bc(0x1234);
        cpu.regs.set_de(0x5678);
        cpu.regs.set_hl(0x9abc);
        cpu.regs.sp = 0xdef0;
    }
    let mut phases = vec![(u16::MAX, prefix, true, false), (0, opcode, true, false)];
    if load {
        phases.extend([(1, 0xff, false, false), (2, 0xff, false, false)]);
        if opcode & 8 == 0 {
            let [low, high] = cpu.read_reg16((opcode >> 4) & 3).to_le_bytes();
            phases.extend([(0xffff, low, false, true), (0, high, false, true)]);
        } else {
            phases.extend([(0xffff, 0x34, false, false), (0, 0x12, false, false)]);
        }
    }
    let mut state = BusState::default();
    let mut legacy = cpu.clone();
    let mut legacy_state = state;
    let phase_count = phases.len();
    let mut total = 0;
    for (index, (address, value, m1, write)) in phases.into_iter().enumerate() {
        let inputs = CycleInterrupts {
            maskable_pending: index != 0,
            nmi_pending: index != 0,
        };
        let (mut clone, mut cloned_state) = restored(&cpu, &state);
        let before = cpu.clone();
        let before_state = state;
        let request = cpu.next_physical_cycle(&state).unwrap();
        let kind = request.kind();
        assert_eq!(
            (kind.address(), kind.is_m1(), kind.is_write()),
            (address, m1, write)
        );
        if let CycleKind::M1 {
            refresh_address, ..
        } = kind
        {
            assert_eq!(refresh_address, if index == 0 { 0x5aff } else { 0x5a80 });
        }
        if write {
            assert_eq!(kind, CycleKind::MemoryWrite { address, value });
        }
        let sample = sample(kind, value);
        request.validate_interrupts(inputs).unwrap();
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
        stale(&cpu, &state, sample, inputs);
        let waits = index as u32 % 3;
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
        assert_eq!(expected.complete, index + 1 == phase_count);
        assert_eq!(expected.instruction.cycles, if m1 { 4 } else { 3 });
        assert!(legacy.extend_bus_segment(&mut expected.instruction, waits));
        assert_eq!(
            cpu.retire_physical_cycle(&mut state, completion).unwrap(),
            expected
        );
        assert_eq!(cpu, legacy);
        assert_eq!(state, legacy_state);
        let cloned_completion = clone
            .next_physical_cycle(&cloned_state)
            .unwrap()
            .complete(sample, waits, inputs)
            .unwrap();
        assert_eq!(
            clone
                .retire_physical_cycle(&mut cloned_state, cloned_completion)
                .unwrap(),
            expected
        );
        assert_eq!(clone, cpu);
        assert_eq!(cloned_state, state);
        assert_eq!(cpu.last_opcode, prefix);
        assert_eq!(cpu.last_opcode_pc, u16::MAX);
        assert_eq!(
            cpu.enable_interrupts_delay,
            u8::from(index + 1 != phase_count)
        );
        assert_eq!(cpu.last_m1_fetch_count, if index == 0 { 1 } else { 2 });
        total += u64::from(if m1 { 4 } else { 3 } + waits);
        assert_eq!(cpu.cycles(), total);
        if index == 0 {
            assert_eq!(cpu.instruction_bytes, [prefix, 0, 0, 0]);
            assert_eq!(cpu.instruction_byte_count, 1);
        }
        if load && state.pending() && index >= 1 {
            malformed_load_context(&cpu, state);
        }
    }
    assert_eq!(cpu.regs.r, 0x81);
    assert_eq!(cpu.regs.pc, if load { 3 } else { 1 });
    assert_eq!(cpu.instruction_byte_count, if load { 4 } else { 2 });
    assert_eq!(
        cpu.instruction_bytes,
        [
            prefix,
            opcode,
            if load { 0xff } else { 0 },
            if load { 0xff } else { 0 }
        ]
    );
    assert!(cpu.interrupt_flip_flop_1 && cpu.interrupt_flip_flop_2);
    assert!(!state.pending());
    assert_eq!(
        cpu.next_physical_cycle(&state)
            .unwrap()
            .validate_interrupts(CycleInterrupts {
                maskable_pending: true,
                nmi_pending: false
            }),
        Err(CycleError::UnsupportedInterrupt)
    );
    if load {
        assert_eq!(cpu.regs.f, flags);
    }
    if load && opcode & 8 != 0 {
        assert_eq!(cpu.read_reg16((opcode >> 4) & 3), 0x1234);
    } else if !load && prefix == Z80_PREFIX_ED {
        assert_eq!(cpu.regs.a, value.wrapping_neg());
        assert_eq!(cpu.regs.f & Z80_FLAG_CARRY != 0, value != 0);
        assert_eq!(cpu.regs.f & Z80_FLAG_ZERO != 0, value == 0);
        assert_eq!(cpu.regs.f & Z80_FLAG_PARITY_OVERFLOW != 0, value == 0x80);
    }
}

pub(super) fn stale(cpu: &Cpu, state: &BusState, sample: CycleSample, inputs: CycleInterrupts) {
    for change_cpu in [false, true] {
        let mut candidate = cpu.clone();
        let mut candidate_state = *state;
        let completion = candidate
            .next_physical_cycle(&candidate_state)
            .unwrap()
            .complete(sample, 0, inputs)
            .unwrap();
        if change_cpu {
            candidate.regs.i ^= 1;
        } else {
            candidate_state.transfer = if candidate_state.transfer.is_some() {
                None
            } else {
                Some(Transfer {
                    pc: candidate.regs.pc,
                    operation: Operation::CbFetch,
                })
            };
        }
        let before = candidate.clone();
        let before_state = candidate_state;
        assert_eq!(
            candidate.retire_physical_cycle(&mut candidate_state, completion),
            Err(CycleError::StaleContinuation)
        );
        assert_eq!(candidate, before);
        assert_eq!(candidate_state, before_state);
    }
}

fn unsupported(prefix: u8, opcode: u8) {
    let (cpu, state) = start(prefix);
    let before = cpu.clone();
    let before_state = state;
    let request = cpu.next_physical_cycle(&state).unwrap();
    let kind = request.kind();
    let completion = request.complete(
        sample(kind, opcode),
        3,
        CycleInterrupts {
            maskable_pending: true,
            nmi_pending: true,
        },
    );
    assert!(
        matches!(completion, Err(CycleError::UnsupportedOpcode { pc: 1, opcode: actual }) if actual == opcode)
    );
    assert_eq!(cpu, before);
    assert_eq!(state, before_state);
}

fn rejects(cpu: &Cpu, state: &BusState) {
    assert!(matches!(
        cpu.next_physical_cycle(state),
        Err(CycleError::UnsupportedCycle)
    ));
}

fn malformed_prefixes() {
    for prefix in [Z80_PREFIX_CB, Z80_PREFIX_ED] {
        let (cpu, state) = start(prefix);
        for variant in 0..9 {
            let mut candidate = cpu.clone();
            let mut candidate_state = state;
            match variant {
                0 => candidate_state.transfer.as_mut().unwrap().pc ^= 1,
                1 => candidate.last_opcode_pc ^= 1,
                2 => candidate.last_opcode = 0x00,
                3 => candidate.instruction_bytes[0] ^= 1,
                4 => candidate.instruction_byte_count = 2,
                5 => candidate.last_m1_fetch_count = 2,
                6 => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
                7 => {
                    candidate_state.transfer.as_mut().unwrap().operation =
                        if prefix == Z80_PREFIX_CB {
                            Operation::EdFetch { pc: 0 }
                        } else {
                            Operation::CbFetch
                        }
                }
                _ => {
                    candidate_state.transfer.as_mut().unwrap().operation =
                        Operation::EdFetch { pc: 1 }
                }
            }
            rejects(&candidate, &candidate_state);
        }
    }
}

fn malformed_load_context(cpu: &Cpu, state: BusState) {
    for variant in 0..10 {
        let mut candidate = cpu.clone();
        let mut candidate_state = state;
        match variant {
            0 => candidate_state.transfer.as_mut().unwrap().pc ^= 1,
            1 => candidate.last_opcode_pc ^= 1,
            2 => candidate.last_opcode = Z80_PREFIX_CB,
            3 => candidate.instruction_bytes[0] = Z80_PREFIX_CB,
            4 => candidate.instruction_bytes[1] = 0x3a,
            5 => candidate.instruction_byte_count ^= 1,
            6 => candidate.last_m1_fetch_count = 1,
            7 => candidate.regs.pc = candidate.regs.pc.wrapping_add(1),
            8 => {
                candidate.instruction_bytes[1] ^= if cpu.instruction_bytes[1] & 8 != 0 {
                    0x10
                } else {
                    8
                }
            }
            _ => candidate.instruction_bytes[1] ^= 0x30,
        }
        rejects(&candidate, &candidate_state);
    }
    for byte in 2..usize::from(cpu.instruction_byte_count) {
        let mut candidate = cpu.clone();
        candidate.instruction_bytes[byte] ^= 1;
        rejects(&candidate, &state);
    }
    if cpu.instruction_bytes[1] & 8 == 0 {
        let mut candidate = cpu.clone();
        let pair = (cpu.instruction_bytes[1] >> 4) & 3;
        candidate.write_reg16(pair, candidate.read_reg16(pair) ^ 0x100);
        rejects(&candidate, &state);
    }
    for payload in [&[0, 0, 0][..], &[0, 3, 2][..], &[0, 3, 0, 1, 0][..]] {
        let mut candidate_state = state;
        let load = load::Load::read_continuation(&mut crate::state::Reader::new(payload)).unwrap();
        candidate_state.transfer.as_mut().unwrap().operation = Operation::Load(load);
        rejects(cpu, &candidate_state);
    }
}
