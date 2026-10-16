use zeff_z80::{Cpu, CpuState, IoWriteCycle, PrefixState, Z80Bus};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bus {
    bytes: Vec<u8>,
    irq: bool,
    nmi: bool,
    io: Vec<IoWriteCycle>,
}
impl Bus {
    fn new(program: &[u8]) -> Self {
        let mut bytes = vec![0; 65536];
        bytes[..program.len()].copy_from_slice(program);
        Self {
            bytes,
            irq: false,
            nmi: false,
            io: Vec::new(),
        }
    }
}
impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        self.bytes[usize::from(address)]
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.bytes[usize::from(address)] = value;
    }
    fn io_read(&mut self, _: u8) -> u8 {
        0x55
    }
    fn io_write(&mut self, _: u8, _: u8) {}
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.io.push(cycle);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        let pending = self.nmi;
        self.nmi = false;
        pending
    }
}

pub fn finite_streams_match_legacy() {
    for program in [
        &[0xdd, 0x21, 0x34, 0x12][..],
        &[0xfd, 0x21, 0x34, 0x12],
        &[0xdd, 0xfd, 0xdd, 0x21, 0x34, 0x12],
        &[0xdd, 0x00],
        &[0xfd, 0xed, 0x44],
        &[0xdd, 0xcb, 0, 0],
        &[0xfd, 0x76],
    ] {
        let mut legacy_bus = Bus::new(program);
        let mut bounded_bus = legacy_bus.clone();
        let mut legacy = Cpu::new();
        let mut bounded = legacy.clone();
        let reference = legacy.step_with_bus(&mut legacy_bus).unwrap();
        let mut state = PrefixState::default();
        let mut total = 0;
        let mut finished = false;
        for _ in 0..program.len() {
            let slice = bounded.step_bounded(&mut bounded_bus, &mut state).unwrap();
            total += slice.instruction.cycles;
            if slice.complete {
                finished = true;
                break;
            }
            assert_eq!(slice.instruction.cycles, 4);
            assert!(state.pending());
        }
        assert!(finished && !state.pending());
        assert_eq!(total, reference.cycles);
        assert_eq!(bounded, legacy);
        assert_eq!(bounded_bus, legacy_bus);
    }
}

pub fn long_prefix_stream_is_bounded() {
    let mut bus = Bus::new(&[]);
    bus.bytes.fill(0xdd);
    let mut cpu = Cpu::new();
    let mut state = PrefixState::default();
    for _ in 0..65537 {
        let step = cpu.step_bounded(&mut bus, &mut state).unwrap();
        assert!(!step.complete);
        assert_eq!(step.instruction.cycles, 4);
    }
    assert_eq!(cpu.regs().pc, 1);
    assert_eq!(cpu.regs().r, 1);
    assert_eq!(cpu.cycles(), 65537 * 4);
    assert_eq!(cpu.last_m1_fetch_count(), 255);
    assert_eq!(cpu.instruction_bytes(), &[0xdd; 4]);
    let mut copied_cpu = cpu.clone();
    let mut copied_state = state;
    let mut copied_bus = bus.clone();
    bus.bytes[1] = 0;
    copied_bus.bytes[1] = 0;
    assert!(cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    assert!(
        copied_cpu
            .step_bounded(&mut copied_bus, &mut copied_state)
            .unwrap()
            .complete
    );
    assert_eq!(cpu, copied_cpu);
    assert_eq!(state, copied_state);
    assert!(!state.pending());
    cpu.reset();
    state = PrefixState::default();
    assert_eq!(cpu, Cpu::new());
    assert!(!state.pending());
}

pub fn interrupt_and_suspend_deferral() {
    let mut bus = Bus::new(&[0xfb, 0xdd, 0xfd, 0, 0]);
    let mut cpu = Cpu::new();
    let mut state = PrefixState::default();
    assert!(cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    bus.irq = true;
    for _ in 0..2 {
        assert!(!cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
        assert!(!cpu.interrupts_enabled());
    }
    cpu.suspend();
    let saved = cpu.clone();
    let saved_prefix = state;
    assert!(cpu.step_bounded(&mut bus, &mut state).is_none());
    assert_eq!(cpu, saved);
    assert_eq!(state, saved_prefix);
    cpu.resume();
    assert!(cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    assert!(cpu.interrupts_enabled());
    let interrupt = cpu.step_bounded(&mut bus, &mut state).unwrap();
    assert!(interrupt.complete && cpu.last_step_was_interrupt());
    assert_eq!(cpu.regs().pc, 0x38);
    let mut cpu = Cpu::new();
    let mut state = PrefixState::default();
    let mut bus = Bus::new(&[0xdd, 0xfd, 0]);
    assert!(!cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    bus.nmi = true;
    assert!(!cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    assert!(bus.nmi);
    assert!(cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    assert!(bus.nmi);
    assert_eq!(
        cpu.step_bounded(&mut bus, &mut state)
            .unwrap()
            .instruction
            .cycles,
        11
    );
    assert!(!bus.nmi);
    assert_eq!(cpu.regs().pc, 0x66);
}

pub fn ignored_index_ed_repeats_at_ed() {
    let mut bus = Bus::new(&[0x21, 0, 1, 0x11, 0, 2, 0x01, 2, 0, 0xdd, 0xed, 0xb0, 0x76]);
    bus.bytes[0x100] = 0x55;
    bus.bytes[0x101] = 0x66;
    let mut cpu = Cpu::new();
    let mut state = PrefixState::default();
    for _ in 0..3 {
        assert!(cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
    }
    assert_eq!(
        cpu.step_bounded(&mut bus, &mut state)
            .unwrap()
            .instruction
            .cycles,
        4
    );
    assert_eq!(
        cpu.step_bounded(&mut bus, &mut state)
            .unwrap()
            .instruction
            .cycles,
        21
    );
    assert_eq!(cpu.regs().pc, 10);
    assert_eq!(cpu.regs().bc(), 1);
    assert_eq!(
        cpu.step_bounded(&mut bus, &mut state)
            .unwrap()
            .instruction
            .cycles,
        16
    );
    assert_eq!(cpu.regs().pc, 12);
    assert_eq!(&bus.bytes[0x200..0x202], &[0x55, 0x66]);
}

pub fn io_offsets_are_segment_relative() {
    for count in [1usize, 3, 300] {
        let mut program = vec![0xdd; count];
        program.extend_from_slice(&[0xd3, 0x7f]);
        let mut bus = Bus::new(&program);
        let mut cpu = Cpu::new();
        let mut state = PrefixState::default();
        for _ in 0..count {
            assert!(!cpu.step_bounded(&mut bus, &mut state).unwrap().complete);
        }
        let terminal = cpu.step_bounded(&mut bus, &mut state).unwrap();
        assert!(terminal.complete);
        assert_eq!(terminal.instruction.cycles, 11);
        assert_eq!(
            bus.io[0],
            IoWriteCycle {
                port: 0x7f,
                value: 0,
                t_states_before: 7,
                t_states: 4
            }
        );
        assert_eq!(cpu.cycles(), (count * 4 + 11) as u64);
        assert_eq!(cpu.state(), CpuState::Running);
    }
}
