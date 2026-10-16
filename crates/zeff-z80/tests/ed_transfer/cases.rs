#[path = "../multi_load/support.rs"]
mod support;

use support::{Access, Bus, phase, seeded};
use zeff_z80::{BusState, Cpu, IoWriteCycle, PrefixState, Registers, Z80Bus};

const OPCODES: [u8; 16] = [
    0x43, 0x53, 0x63, 0x73, 0x4b, 0x5b, 0x6b, 0x7b, 0x45, 0x4d, 0x55, 0x5d, 0x65, 0x6d, 0x75, 0x7d,
];

fn ret(opcode: u8) -> bool {
    opcode & 7 == 5
}

fn schedule(opcode: u8) -> &'static [u32] {
    if opcode & 0xe4 == 0xa0 {
        match (opcode & 3, opcode & 0x10 != 0) {
            (0, false) => &[4, 4, 3, 3, 2],
            (0, true) => &[4, 4, 3, 3, 2, 5],
            (1, false) => &[4, 4, 3, 5],
            (1, true) => &[4, 4, 3, 5, 5],
            (2, false) => &[4, 4, 1, 4, 3],
            (2, true) => &[4, 4, 1, 4, 3, 5],
            (_, false) => &[4, 4, 1, 3, 4],
            (_, true) => &[4, 4, 1, 3, 4, 5],
        }
    } else if opcode & 0xc6 == 0x40 {
        &[4, 4, 4]
    } else if matches!(
        opcode,
        0x42 | 0x52 | 0x62 | 0x72 | 0x4a | 0x5a | 0x6a | 0x7a
    ) {
        &[4, 4, 4, 3]
    } else if matches!(opcode, 0x47 | 0x4f | 0x57 | 0x5f) {
        &[4, 4, 1]
    } else if matches!(opcode, 0x67 | 0x6f) {
        &[4, 4, 3, 4, 3]
    } else if ret(opcode) {
        &[4, 4, 3, 3]
    } else {
        &[4, 4, 3, 3, 3, 3]
    }
}

fn pair(regs: Registers, opcode: u8) -> u16 {
    match opcode & 0x30 {
        0 => regs.bc(),
        0x10 => regs.de(),
        0x20 => regs.hl(),
        _ => regs.sp,
    }
}

fn set_pair(regs: &mut Registers, opcode: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    match opcode & 0x30 {
        0 => (regs.b, regs.c) = (high, low),
        0x10 => (regs.d, regs.e) = (high, low),
        0x20 => (regs.h, regs.l) = (high, low),
        _ => regs.sp = value,
    }
}

fn fixture(prefix: Option<u8>, opcode: u8, address: u16, pc: u16) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x9c);
    let setup = usize::from(cpu.regs().pc);
    let sp = if ret(opcode) { address } else { 0x7000 };
    let [low, high] = sp.to_le_bytes();
    let [pc_low, pc_high] = pc.to_le_bytes();
    bus.bytes[setup..setup + 6].copy_from_slice(&[0x31, low, high, 0xc3, pc_low, pc_high]);
    for _ in 0..2 {
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
    }
    bus.bytes[usize::from(address)] = 0x53;
    bus.bytes[usize::from(address.wrapping_add(1))] = 0xa7;
    let mut at = pc;
    for byte in prefix.into_iter().chain([0xed, opcode]).chain(
        (!ret(opcode))
            .then_some(address.to_le_bytes())
            .into_iter()
            .flatten(),
    ) {
        bus.bytes[usize::from(at)] = byte;
        at = at.wrapping_add(1);
    }
    (cpu, bus)
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, opcode: u8, from: usize) {
    let clocks = schedule(opcode);
    for (i, &clock) in clocks.iter().enumerate().skip(from) {
        phase(cpu, bus, state, clock, i + 1 == clocks.len());
    }
}

fn literal_case(prefix: Option<u8>, opcode: u8, address: u16, pc: u16) {
    let (mut cpu, mut bus) = fixture(prefix, opcode, address, pc);
    let mut regs = cpu.regs();
    let mut memory = bus.bytes.clone();
    let ed = pc.wrapping_add(u16::from(prefix.is_some()));
    let sequential = ed.wrapping_add(if ret(opcode) { 2 } else { 4 });
    let mut accesses = Vec::new();
    let bytes = if ret(opcode) { 2 } else { 4 };
    for offset in 0..bytes + u16::from(prefix.is_some()) {
        let at = pc.wrapping_add(offset);
        accesses.push(Some(Access::Read(at, memory[usize::from(at)])));
    }
    regs.pc = sequential;
    regs.r += 2 + u8::from(prefix.is_some());
    let high_address = address.wrapping_add(1);
    if ret(opcode) || opcode & 8 != 0 {
        let low = memory[usize::from(address)];
        let high = memory[usize::from(high_address)];
        accesses.extend([
            Some(Access::Read(address, low)),
            Some(Access::Read(high_address, high)),
        ]);
        let value = u16::from_le_bytes([low, high]);
        if ret(opcode) {
            regs.pc = value;
            regs.sp = address.wrapping_add(2);
        } else {
            set_pair(&mut regs, opcode, value);
        }
    } else {
        let [low, high] = pair(regs, opcode).to_le_bytes();
        accesses.extend([
            Some(Access::Write(address, low)),
            Some(Access::Write(high_address, high)),
        ]);
        memory[usize::from(address)] = low;
        memory[usize::from(high_address)] = high;
    }
    let mut legacy = (cpu.clone(), bus.clone());
    let mut bounded = legacy.clone();
    let total = legacy.0.step_with_bus(&mut legacy.1).unwrap().cycles;
    let mut prefix_state = PrefixState::default();
    let mut bounded_clocks = 0;
    loop {
        let step = bounded
            .0
            .step_bounded(&mut bounded.1, &mut prefix_state)
            .unwrap();
        bounded_clocks += step.instruction.cycles;
        if step.complete {
            break;
        }
    }
    assert_eq!(bounded, legacy);
    assert_eq!(bounded_clocks, total);
    let start_cycles = cpu.cycles();
    let mut state = BusState::default();
    if prefix.is_some() {
        phase(&mut cpu, &mut bus, &mut state, 4, false);
    }
    for (i, &clock) in schedule(opcode).iter().enumerate() {
        let complete = i + 1 == schedule(opcode).len();
        phase(&mut cpu, &mut bus, &mut state, clock, complete);
        let phase_pc = if complete {
            regs.pc
        } else {
            ed.wrapping_add(if ret(opcode) {
                (i + 1).min(2)
            } else {
                (i + 1).min(4)
            } as u16)
        };
        assert_eq!(cpu.regs().pc, phase_pc);
        if ret(opcode) {
            assert_eq!(
                cpu.regs().sp,
                address.wrapping_add(i.saturating_sub(1) as u16)
            );
        }
        assert_eq!(state.prefix_pending(), prefix.is_some() && !complete);
    }
    assert_eq!(
        total,
        if ret(opcode) { 14 } else { 20 } + 4 * u32::from(prefix.is_some())
    );
    assert_eq!(cpu.cycles() - start_cycles, u64::from(total));
    assert_eq!(
        cpu.regs(),
        regs,
        "{prefix:?} {opcode:02x} {address:04x} {pc:04x}"
    );
    assert_eq!(bus.bytes, memory);
    assert_eq!(bus.trace.borrow().count, accesses.len());
    assert_eq!(
        &bus.trace.borrow().events[..accesses.len()],
        accesses.as_slice()
    );
    assert_eq!((cpu, bus), legacy);
}

pub fn all_ed_transfer_forms_match_literal_effects_and_steppers() {
    let mut forms = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            forms += 1;
            for address in [0, 0x8000, 0xffff] {
                literal_case(prefix, opcode, address, 0x4000);
            }
        }
    }
    assert_eq!(forms, 48);
}

pub fn ed_transfer_live_reads_latches_and_wraps() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            for pc in [0x4000u16, 0xfffe, 0xffff] {
                let ed = pc.wrapping_add(u16::from(prefix.is_some()));
                for address in [ed, ed.wrapping_add(2), ed.wrapping_add(3)] {
                    literal_case(prefix, opcode, address, pc);
                }
            }
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 0x4000);
            let source = pair(cpu.regs(), opcode);
            let ed = 0x4000 + usize::from(prefix.is_some());
            let mut state = BusState::default();
            if prefix.is_some() {
                phase(&mut cpu, &mut bus, &mut state, 4, false);
            }
            phase(&mut cpu, &mut bus, &mut state, 4, false);
            phase(&mut cpu, &mut bus, &mut state, 4, false);
            bus.bytes[ed + 1] = 0;
            let address = if ret(opcode) {
                0xffff
            } else {
                phase(&mut cpu, &mut bus, &mut state, 3, false);
                bus.bytes[ed + 2] = 0x12;
                bus.bytes[ed + 3] = 0x80;
                phase(&mut cpu, &mut bus, &mut state, 3, false);
                bus.bytes[ed + 3] = 0x90;
                0x80ff
            };
            bus.bytes[address] = 0x62;
            let high = (address + 1) & 0xffff;
            bus.bytes[high] = 0x73;
            phase(&mut cpu, &mut bus, &mut state, 3, false);
            if !ret(opcode) && opcode & 8 == 0 {
                assert_eq!(bus.bytes[address], source as u8);
            }
            bus.bytes[address] = 0x19;
            bus.bytes[high] = 0x84;
            phase(&mut cpu, &mut bus, &mut state, 3, true);
            assert_eq!(bus.bytes[address], 0x19);
            if ret(opcode) {
                assert_eq!(cpu.regs().pc, 0x8462);
                assert_eq!(cpu.regs().sp, 1);
            } else if opcode & 8 != 0 {
                assert_eq!(pair(cpu.regs(), opcode), 0x8462);
            } else {
                assert_eq!(bus.bytes[high], (source >> 8) as u8);
            }
        }
    }
}

pub fn ed_transfer_pending_clone_suspend_reset() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let mut clocks = Vec::new();
            if prefix.is_some() {
                clocks.push(4);
            }
            clocks.extend_from_slice(schedule(opcode));
            for count in 1..clocks.len() {
                let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 0x4000);
                let mut state = BusState::default();
                for &clock in &clocks[..count] {
                    phase(&mut cpu, &mut bus, &mut state, clock, false);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut copied = (cpu.clone(), bus.clone(), state);
                for (i, &clock) in clocks.iter().enumerate().skip(count) {
                    phase(&mut cpu, &mut bus, &mut state, clock, i + 1 == clocks.len());
                    phase(
                        &mut copied.0,
                        &mut copied.1,
                        &mut copied.2,
                        clock,
                        i + 1 == clocks.len(),
                    );
                }
                assert_eq!((cpu, bus, state), copied);
                let (mut cpu, mut bus, _) = saved;
                cpu.reset();
                bus.clear_trace();
                bus.bytes[0x100] = 0;
                let memory = bus.bytes.clone();
                phase(&mut cpu, &mut bus, &mut BusState::default(), 4, true);
                assert_eq!(bus.bytes, memory);
                assert_eq!(cpu.regs().pc, 0x101);
                assert_eq!(cpu.regs().sp, 0x9876);
            }
        }
    }
}

pub fn ed_transfer_interrupt_boundaries() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let mut clocks = Vec::new();
            if prefix.is_some() {
                clocks.push(4);
            }
            clocks.extend_from_slice(schedule(opcode));
            for count in 1..clocks.len() {
                for (iff1, iff2) in [(false, false), (false, true), (true, false), (true, true)] {
                    for nmi in [false, true] {
                        let (mut cpu, mut bus) = fixture(prefix, opcode, 0x7000, 0x4000);
                        cpu.finish_guest_call(iff1, iff2, 0);
                        cpu.resume();
                        let mut state = BusState::default();
                        for &clock in &clocks[..count] {
                            phase(&mut cpu, &mut bus, &mut state, clock, false);
                            assert_eq!(cpu.interrupts_enabled(), iff1);
                        }
                        let polls = bus.polls.get();
                        bus.nmi = nmi;
                        bus.irq = !nmi;
                        for (i, &clock) in clocks.iter().enumerate().skip(count) {
                            let complete = i + 1 == clocks.len();
                            phase(&mut cpu, &mut bus, &mut state, clock, complete);
                            assert_eq!(
                                cpu.interrupts_enabled(),
                                if complete && ret(opcode) { iff2 } else { iff1 }
                            );
                            assert_eq!(cpu.saved_interrupts_enabled(), iff2);
                            assert_eq!(bus.polls.get(), polls);
                        }
                        bus.clear_trace();
                        cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                        assert_eq!(
                            cpu.last_step_was_interrupt(),
                            nmi || if ret(opcode) { iff2 } else { iff1 }
                        );
                    }
                }
            }
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x7000, 0x4000);
            bus.bytes.copy_within(0x4000..0x4006, 0x4001);
            bus.bytes[0x4000] = 0xfb;
            cpu.step_with_bus(&mut bus).unwrap();
            bus.clear_trace();
            bus.irq = true;
            let mut state = BusState::default();
            for (i, &clock) in clocks.iter().enumerate() {
                let complete = i + 1 == clocks.len();
                phase(&mut cpu, &mut bus, &mut state, clock, complete);
                assert_eq!(cpu.interrupts_enabled(), complete);
                assert!(!cpu.last_step_was_interrupt());
            }
            bus.clear_trace();
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert!(cpu.last_step_was_interrupt());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IoBus {
    inner: Bus,
    output: Option<IoWriteCycle>,
}

impl Z80Bus for IoBus {
    fn cpu_read(&self, address: u16) -> u8 {
        self.inner.cpu_read(address)
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.inner.cpu_write(address, value);
    }
    fn io_read(&mut self, port: u8) -> u8 {
        self.inner.io_read(port)
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.inner.io_write(port, value);
    }
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.output = Some(cycle);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.inner.maskable_interrupt_pending()
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.inner.non_maskable_interrupt_pending()
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.inner.acknowledge_non_maskable_interrupt()
    }
}

pub fn ed_terminal_fallback_fetch_repeat_and_io() {
    for prefixes in [&[][..], &[0xdd][..], &[0xfd][..], &[0xdd, 0xfd, 0xdd][..]] {
        for opcode in 0..=255u8 {
            let (mut cpu, mut inner) = fixture(None, opcode, 0x8000, 0x4000);
            let ed = 0x4000 + prefixes.len();
            inner.bytes[0x4000..ed].copy_from_slice(prefixes);
            inner.bytes[ed..ed + 4].copy_from_slice(&[0xed, opcode, 0x00, 0x80]);
            let mut bus = IoBus {
                inner,
                output: None,
            };
            let mut legacy = (cpu.clone(), bus.clone());
            let mut bounded = legacy.clone();
            let total = legacy.0.step_with_bus(&mut legacy.1).unwrap().cycles;
            let mut prefix_state = PrefixState::default();
            let mut bounded_total = 0;
            loop {
                let step = bounded
                    .0
                    .step_bounded(&mut bounded.1, &mut prefix_state)
                    .unwrap();
                bounded_total += step.instruction.cycles;
                if step.complete {
                    break;
                }
            }
            assert_eq!(bounded_total, total);
            assert_eq!(bounded.0, legacy.0);
            assert_eq!(bounded.1.inner, legacy.1.inner);
            let mut state = BusState::default();
            for _ in 0..=prefixes.len() {
                let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                assert_eq!(step.instruction.cycles, 4);
                assert!(!step.complete);
            }
            assert_eq!(cpu.regs().pc, ed as u16 + 1);
            assert_eq!(bus.inner.trace.borrow().count, prefixes.len() + 1);
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            let selected = OPCODES.contains(&opcode)
                || opcode & 0xe4 == 0xa0
                || opcode & 0xc6 == 0x40
                || matches!(
                    opcode,
                    0x42 | 0x52
                        | 0x62
                        | 0x72
                        | 0x4a
                        | 0x5a
                        | 0x6a
                        | 0x7a
                        | 0x47
                        | 0x4f
                        | 0x57
                        | 0x5f
                        | 0x67
                        | 0x6f
                );
            assert_eq!(step.complete, !selected);
            assert_eq!(
                step.instruction.cycles,
                if selected {
                    4
                } else {
                    total - 4 * (prefixes.len() as u32 + 1)
                }
            );
            if selected {
                for (i, &clock) in schedule(opcode).iter().enumerate().skip(2) {
                    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                    assert_eq!(step.instruction.cycles, clock);
                    assert_eq!(step.complete, i + 1 == schedule(opcode).len());
                }
            }
            assert!(!state.pending());
            assert_eq!(cpu, legacy.0, "{prefixes:?} {opcode:02x}");
            assert_eq!(bus.inner, legacy.1.inner);
            if let Some(output) = bus.output {
                let block = matches!(opcode, 0xa3 | 0xab | 0xb3 | 0xbb);
                assert_eq!(output.t_states_before, 0);
                assert_eq!(output.t_states, 4);
                let mut expected = output;
                expected.t_states_before += if block { 12 } else { 8 };
                assert_eq!(bounded.1.output, Some(expected));
                expected.t_states_before += 4 * prefixes.len() as u32;
                assert_eq!(legacy.1.output, Some(expected));
            } else {
                assert_eq!(bounded.1.output, None);
                assert_eq!(legacy.1.output, None);
            }
            if matches!(opcode, 0xb0..=0xb3 | 0xb8..=0xbb) {
                assert_eq!(cpu.regs().pc, ed as u16);
            }
        }
    }
    for (from, to) in [(0, 0x43), (0x43, 0), (0x45, 0x4b), (0x4b, 0x4d)] {
        let (mut cpu, mut bus) = fixture(None, from, 0x8000, 0x4000);
        bus.bytes[0x4002..0x4004].copy_from_slice(&[0, 0x80]);
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 4, false);
        bus.bytes[0x4001] = to;
        if to == 0 {
            phase(&mut cpu, &mut bus, &mut state, 4, true);
        } else {
            finish(&mut cpu, &mut bus, &mut state, to, 1);
            if to == 0x43 {
                assert_eq!(&bus.bytes[0x8000..0x8002], &[0x12, 0x10]);
            }
            if to == 0x4b {
                assert_eq!(cpu.regs().bc(), 0xa753);
            }
            if to == 0x4d {
                assert_eq!(cpu.regs().pc, 0);
            }
        }
    }
}
