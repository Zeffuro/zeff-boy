use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode, M68kBus};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryReport, PhysicalEntryState, PhysicalInspection,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

const FIELDS: [u16; 7] = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39];
const WORDS: [u32; 8] = [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555];
const LONGS: [u32; 8] = [
    0,
    1,
    0x8000,
    0x8000_0000,
    0x8000_0001,
    0xffff_ffff,
    0xffff_0000,
    0x5555_5555,
];
const BUDGET: u64 = u32::MAX as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Read(BusAccess, u16, u64, u32),
    Write(BusAccess, u16, u64, u32),
    Idle(u64, u32),
}

struct Device<'a> {
    memory: &'a mut MemoryBus,
    events: [Option<Event>; 8],
    event_count: usize,
    transfers: usize,
    clock: u64,
    host: Option<usize>,
    maximum: Option<usize>,
}

impl Device<'_> {
    fn event(&mut self, event: Event, start: CpuClocks, elapsed: u32) {
        assert_eq!(self.clock, start.0);
        self.events[self.event_count] = Some(event);
        self.event_count += 1;
        self.clock += u64::from(elapsed);
    }
    fn elapsed(&self) -> u32 {
        if self.maximum == Some(self.transfers) {
            u32::MAX
        } else {
            7
        }
    }
}

impl EntryBus for Device<'_> {
    type Error = ();
    fn read_entry(&mut self, _: BusAccess, _: CpuClocks) -> EntryReport<u16, ()> {
        panic!("unexpected entry read")
    }
    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), ()> {
        panic!("unexpected entry write")
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.event(Event::Idle(start.0, clocks), start, clocks);
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, ()> {
        assert_eq!(access.logical_address & 1, 0);
        let value = self.memory.read(access).unwrap();
        let elapsed = self.elapsed();
        let failed = self.host == Some(self.transfers);
        self.event(Event::Read(access, value, start.0, elapsed), start, elapsed);
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: if failed {
                InstructionOutcome::HostFailure(())
            } else {
                InstructionOutcome::Complete(value)
            },
        }
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert_eq!(access.logical_address & 1, 0);
        let elapsed = self.elapsed();
        let failed = self.host == Some(self.transfers);
        self.memory.write(access, value).unwrap();
        self.event(
            Event::Write(access, value, start.0, elapsed),
            start,
            elapsed,
        );
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: if failed {
                InstructionOutcome::HostFailure(())
            } else {
                InstructionOutcome::Complete(())
            },
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Reserve,
    Idle,
    Address(u32),
    Flags(u16),
    Read(BusAccess),
    Write(BusAccess, u16),
    Commit,
}

#[derive(Clone, Copy)]
enum Path {
    Complete,
    Host(usize),
    Blocked(usize),
    Odd,
}

fn access(address: u32, supervisor: bool, data: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: match (supervisor, data) {
            (false, false) => FunctionCode::UserProgram,
            (false, true) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorProgram,
            (true, true) => FunctionCode::SupervisorData,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: ByteLanes::Both,
    }
}

fn flags(value: u32, long: bool) -> u16 {
    let value = if long { value } else { value & 0xffff };
    let sign = if long { 0x8000_0000 } else { 0x8000 };
    (u16::from(value == 0) * 4) | (u16::from(value & sign != 0) * 8)
}

fn extensions(source: usize) -> usize {
    if source == 6 {
        2
    } else {
        usize::from(source >= 3)
    }
}

fn run(
    memory: &mut MemoryBus,
    form: (u16, bool, usize, usize),
    sample: (bool, u16, u32),
    path: Path,
) {
    let (family, long, source, register) = form;
    let (supervisor, ccr, value) = sample;
    let odd = matches!(path, Path::Odd);
    let ea = 0x3000 + u32::from(odd);
    let stride = if long { 4 } else { 2 };
    let ext = extensions(source);
    let anchor = 0x2002 + 2 * ext as u32;
    let opcode = family
        | if long { 0x80 } else { 0x40 }
        | FIELDS[source]
        | if source < 5 { register as u16 } else { 0 };
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.a.fill(0x4000);
    r.d.fill(0xaabb_ccdd);
    r.d[7] = 0x1000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.set_status_register(0x0500 | ccr | if supervisor { 0x2000 } else { 0 });
    if source < 5 {
        r.set_address_register(
            register,
            match source {
                2 => ea + stride,
                4 => 0x2000,
                _ => ea,
            },
        );
    }
    let extension = match source {
        0..=2 => 0x4e71,
        3 | 6 => 0,
        4 => 0x7800 | u16::from(odd),
        5 => ea as u16,
        _ => unreachable!(),
    };
    memory.word(0x2000, opcode);
    memory.word(0x2002, extension);
    memory.word(0x2004, if source == 6 { ea as u16 } else { 0x4e71 });
    memory.word(anchor as usize, 0x4e71);
    memory.word((anchor + 2) as usize, 0x7201);
    memory.word(
        (ea & !1) as usize,
        if long {
            (value >> 16) as u16
        } else {
            value as u16
        },
    );
    memory.word(
        ((ea + 2) & !1) as usize,
        if long { value as u16 } else { 0x5a5a },
    );
    let expected_operand = if long { value } else { value & 0xffff };
    let high = memory.read(access(ea & !1, supervisor, true)).unwrap();
    let low = memory
        .read(access((ea + 2) & !1, supervisor, true))
        .unwrap();
    assert_eq!(
        if long {
            u32::from(high) << 16 | u32::from(low)
        } else {
            u32::from(high)
        },
        expected_operand
    );
    let result = if family == 0x4200 {
        0
    } else if long {
        !value
    } else {
        !value & 0xffff
    };
    let mut actions = [Action::Reserve; 20];
    let mut length = 0;
    let mut push = |a| {
        actions[length] = a;
        length += 1;
    };
    if matches!(source, 2 | 4) {
        push(Action::Idle);
    }
    for at in 0..ext {
        push(Action::Read(access(
            0x2004 + 2 * at as u32,
            supervisor,
            false,
        )));
    }
    if source == 2 || source == 1 && !long {
        push(Action::Reserve);
        push(Action::Address(if source == 2 { ea } else { ea + stride }));
    }
    push(Action::Read(access(ea, supervisor, true)));
    if long {
        if source == 1 {
            push(Action::Reserve);
            push(Action::Address(ea + stride));
        }
        push(Action::Read(access(ea + 2, supervisor, true)));
    }
    push(Action::Reserve);
    push(Action::Flags(flags(result, false)));
    push(Action::Read(access(anchor + 2, supervisor, false)));
    if long {
        push(Action::Reserve);
        push(Action::Flags(flags(result, true)));
        push(Action::Write(
            access(ea + 2, supervisor, true),
            result as u16,
        ));
    }
    push(Action::Write(
        access(ea, supervisor, true),
        if long {
            (result >> 16) as u16
        } else {
            result as u16
        },
    ));
    push(Action::Commit);
    let mut start = 17;
    let mut maximum = None;
    if let Path::Blocked(target) = path {
        if target == 0 {
            start = u64::MAX - BUDGET + 1;
        } else {
            maximum = Some(target - 1);
            let mut prefix = 0;
            let mut index = 0;
            for a in &actions[..length] {
                match a {
                    Action::Idle => prefix += 2,
                    Action::Read(_) | Action::Write(_, _) => {
                        if index == target {
                            break;
                        }
                        prefix += if index == target - 1 { BUDGET } else { 7 };
                        index += 1;
                    }
                    _ => {}
                }
            }
            start = u64::MAX - prefix;
        }
    }
    let host = if let Path::Host(at) = path {
        Some(at)
    } else {
        None
    };
    let queue = [opcode, extension];
    let mut expected_memory = memory.0;
    let mut expected_events = [None; 8];
    let mut event_count = 0;
    let mut transfers = 0;
    let mut clock = start;
    let mut expected = r;
    let mut expected_queue = Some(queue);
    let mut fault = None;
    for action in &actions[..length] {
        match *action {
            Action::Reserve if clock.checked_add(BUDGET).is_none() => {
                fault = Some(InstructionFault::Entry(EntryFault::ClockOverflow));
                break;
            }
            Action::Reserve => {}
            Action::Idle => {
                expected_events[event_count] = Some(Event::Idle(clock, 2));
                event_count += 1;
                clock += 2;
            }
            Action::Address(address) => {
                expected.set_address_register(register, address);
            }
            Action::Flags(nzvc) => {
                expected.set_status_register((r.status_register() & !15) | nzvc);
            }
            Action::Commit => {
                expected.pc = anchor;
                expected_queue = Some([0x4e71, 0x7201]);
            }
            Action::Read(a) | Action::Write(a, _) => {
                if clock.checked_add(BUDGET).is_none() {
                    fault = Some(InstructionFault::Entry(EntryFault::ClockOverflow));
                    break;
                }
                if a.logical_address & 1 != 0 {
                    expected_queue = None;
                    fault = Some(InstructionFault::TransitionTimingUnavailable {
                        phase: InstructionPhase::OperandReadHigh,
                        cause: CriticalFault::AddressError {
                            access: a,
                            direction: Direction::Read,
                        },
                    });
                    break;
                }
                let offset = a.bus_address as usize;
                let elapsed = if maximum == Some(transfers) {
                    u32::MAX
                } else {
                    7
                };
                let direction = if let Action::Write(_, value) = *action {
                    expected_memory[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
                    expected_events[event_count] = Some(Event::Write(a, value, clock, elapsed));
                    Direction::Write
                } else {
                    let value =
                        u16::from_be_bytes([expected_memory[offset], expected_memory[offset + 1]]);
                    expected_events[event_count] = Some(Event::Read(a, value, clock, elapsed));
                    Direction::Read
                };
                event_count += 1;
                clock += u64::from(elapsed);
                let failed = host == Some(transfers);
                transfers += 1;
                if failed {
                    expected_queue = None;
                    fault = Some(InstructionFault::Entry(EntryFault::HostFailure {
                        access: a,
                        direction,
                        source: (),
                    }));
                    break;
                }
            }
        }
    }
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(start)).unwrap();
    let mut bus = Device {
        memory,
        events: [None; 8],
        event_count: 0,
        transfers: 0,
        clock: start,
        host,
        maximum,
    };
    let actual = black_box(cpu.step_physical_instruction(&mut bus));
    if let Some(fault) = &fault {
        assert_eq!(actual, Err(fault.clone()));
    } else {
        assert!(
            matches!(actual, Ok(PhysicalInstructionStep::Instruction(step))
            if step.opcode == opcode && step.clocks == clock - start && step.exception.is_none())
        );
    }
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), expected_queue);
    assert_eq!(cpu.clock().0, clock);
    assert_eq!(bus.clock, clock);
    assert_eq!(bus.transfers, transfers);
    assert_eq!(bus.event_count, event_count);
    assert_eq!(bus.events, expected_events);
    assert_eq!(bus.memory.0, expected_memory);
    assert!(!cpu.trace_pending());
    assert!(!cpu.stopped());
    if let Some(InstructionFault::TransitionTimingUnavailable { cause, .. }) = &fault {
        assert_eq!(
            cpu.physical_entry_state(),
            PhysicalEntryState::FaultUnqualified { cause: *cause }
        );
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
    } else {
        assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
        assert_eq!(
            cpu.physical_inspection(),
            PhysicalInspection::DiagnosticCompleteValues
        );
    }
    if fault.is_some() {
        let saved = cpu.clone();
        let guard = if matches!(path, Path::Odd) {
            TimingFault::PhysicalFaultUnqualified
        } else {
            TimingFault::Frozen
        };
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(guard))
        );
        assert_eq!(cpu, saved);
        assert_eq!(bus.events, expected_events);
        assert_eq!(bus.event_count, event_count);
        assert_eq!(bus.transfers, transfers);
        assert_eq!(bus.clock, clock);
        assert_eq!(bus.memory.0, expected_memory);
    }
}

pub(super) fn rmw_steps(memory: &mut MemoryBus) {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut main = 0;
    for _ in 0..5 {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for family in [0x4200, 0x4600] {
                    for long in [false, true] {
                        for source in 0..7 {
                            for register in 0..if source < 5 { 8 } else { 1 } {
                                for value in if long { LONGS } else { WORDS } {
                                    run(
                                        memory,
                                        (family, long, source, register),
                                        (supervisor, ccr, value),
                                        Path::Complete,
                                    );
                                    main += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(main, 430_080);
    let mut probes = 0;
    for _ in 0..100 {
        for supervisor in [false, true] {
            for family in [0x4200, 0x4600] {
                for long in [false, true] {
                    let value = if long { 0x8000 } else { 0x7fff };
                    for source in 0..7 {
                        let count = extensions(source) + if long { 5 } else { 3 };
                        for register in 0..if source < 5 { 8 } else { 1 } {
                            let form = (family, long, source, register);
                            let sample = (supervisor, 31, value);
                            run(memory, form, sample, Path::Complete);
                            probes += 1;
                            for at in 0..count {
                                run(memory, form, sample, Path::Host(at));
                                run(memory, form, sample, Path::Blocked(at));
                                probes += 2;
                            }
                        }
                        run(
                            memory,
                            (family, long, source, 0),
                            (supervisor, 31, value),
                            Path::Odd,
                        );
                        probes += 1;
                    }
                }
            }
        }
    }
    assert_eq!(probes, 338_400);
    assert_eq!(main + probes, 768_480);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
